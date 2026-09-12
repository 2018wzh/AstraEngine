use crate::{
    scene::{error, read_asset},
    MinoriAudioCommand, MinoriMountedVfs,
};
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};
use kira::{
    backend::{Backend, Renderer},
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        PlaybackState,
    },
    AudioManager, AudioManagerSettings, Decibels, Panning, Tween,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
pub(crate) mod decode;

pub(crate) const FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48000,
    channels: 2,
    format: PcmFormat::F32,
};
const MAX_FRAMES: usize = 32 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SoundSnapshot {
    pub id: u32,
    pub uri: String,
    pub position: f64,
    pub volume: f32,
    pub pan: f32,
    pub repeat: bool,
    pub playing: bool,
}
enum Command {
    Apply(Vec<MinoriAudioCommand>),
    Snapshot(SyncSender<FamilyResult<Vec<SoundSnapshot>>>),
    Restore(Vec<SoundSnapshot>, SyncSender<FamilyResult<()>>),
    Suspend(bool),
}
pub(crate) struct Audio {
    commands: SyncSender<Command>,
    sink: Arc<AudioSinkBox>,
    stop: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<FamilyError>>>,
    worker: Option<JoinHandle<()>>,
}
impl Audio {
    pub fn start(archive: Arc<MinoriMountedVfs>, sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(FORMAT).into_result()?;
        let sink = Arc::new(sink);
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let (tx, rx) = sync_channel(64);
        let (s, c, f) = (sink.clone(), stop.clone(), failure.clone());
        let worker = thread::Builder::new()
            .name("minori-audio".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(archive, s.clone(), c.clone(), rx)
                }));
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some(error(
                        "ASTRA_EMU_MINORI_AUDIO_PANIC",
                        "audio worker panicked",
                    )),
                };
                if !c.load(Ordering::Acquire) {
                    if let Ok(mut slot) = f.lock() {
                        *slot = error;
                    }
                }
            })
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_AUDIO_THREAD",
                    "audio worker could not start",
                )
            })?;
        Ok(Self {
            commands: tx,
            sink,
            stop,
            failure,
            worker: Some(worker),
        })
    }
    pub fn check(&self) -> FamilyResult<()> {
        self.failure
            .lock()
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_AUDIO_STATE",
                    "audio worker state poisoned",
                )
            })?
            .clone()
            .map_or(Ok(()), Err)
    }
    fn send(&self, command: Command) -> FamilyResult<()> {
        self.check()?;
        self.commands.try_send(command).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_AUDIO_QUEUE",
                "audio command queue is full or closed",
            )
        })
    }
    pub fn apply(&self, commands: Vec<MinoriAudioCommand>) -> FamilyResult<()> {
        self.send(Command::Apply(commands))
    }
    pub fn suspend(&self, value: bool) -> FamilyResult<()> {
        self.send(Command::Suspend(value))
    }
    pub fn snapshot(&self) -> FamilyResult<Vec<SoundSnapshot>> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Snapshot(tx))?;
        rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_AUDIO_SNAPSHOT",
                "audio snapshot did not complete",
            )
        })?
    }
    pub fn restore(&self, snapshot: Vec<SoundSnapshot>) -> FamilyResult<()> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Restore(snapshot, tx))?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_AUDIO_RESTORE",
                "audio restore did not complete",
            )
        })?
    }
    pub fn shutdown(&mut self) -> FamilyResult<()> {
        if self.worker.is_none() {
            return self.check();
        }
        self.stop.store(true, Ordering::Release);
        let cancel = self.sink.cancel().into_result();
        let joined = self
            .worker
            .take()
            .map(|w| w.join())
            .transpose()
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_AUDIO_JOIN",
                    "audio worker panicked during shutdown",
                )
            });
        cancel?;
        joined?;
        self.check()
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
struct CpuBackend {
    renderer: Option<Renderer>,
}
impl Backend for CpuBackend {
    type Settings = ();
    type Error = std::convert::Infallible;
    fn setup(_: (), _: usize) -> Result<(Self, u32), Self::Error> {
        Ok((Self { renderer: None }, FORMAT.sample_rate))
    }
    fn start(&mut self, r: Renderer) -> Result<(), Self::Error> {
        self.renderer = Some(r);
        Ok(())
    }
}
struct Sound {
    data: StaticSoundData,
    handle: Option<StaticSoundHandle>,
    state: SoundSnapshot,
}
struct Mixer {
    manager: AudioManager<CpuBackend>,
    sounds: BTreeMap<u32, Sound>,
}
impl Mixer {
    fn new() -> FamilyResult<Self> {
        Ok(Self {
            manager: AudioManager::new(AudioManagerSettings {
                backend_settings: (),
                ..Default::default()
            })
            .map_err(|_| error("ASTRA_EMU_MINORI_MIXER", "mixer could not start"))?,
            sounds: BTreeMap::new(),
        })
    }
    fn load(
        &mut self,
        id: u32,
        uri: &str,
        archive: &MinoriMountedVfs,
        stop: &AtomicBool,
    ) -> FamilyResult<()> {
        if self.sounds.len() >= 64 && !self.sounds.contains_key(&id) {
            return Err(error(
                "ASTRA_EMU_MINORI_AUDIO_STREAMS",
                "too many audio streams",
            ));
        }
        let used = self
            .sounds
            .values()
            .map(|s| s.data.frames.len())
            .sum::<usize>();
        let bytes = read_asset(archive, uri, 64 * 1024 * 1024)?;
        let data = decode::decode(bytes, MAX_FRAMES.saturating_sub(used), stop)?;
        if let Some(mut previous) = self.sounds.remove(&id) {
            if let Some(h) = &mut previous.handle {
                h.stop(immediate());
            }
        }
        self.sounds.insert(
            id,
            Sound {
                data,
                handle: None,
                state: SoundSnapshot {
                    id,
                    uri: uri.into(),
                    position: 0.0,
                    volume: 1.0,
                    pan: 0.0,
                    repeat: false,
                    playing: false,
                },
            },
        );
        Ok(())
    }
    fn play(
        &mut self,
        id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade: u32,
    ) -> FamilyResult<()> {
        validate_params(volume, pan)?;
        let sound = self.sounds.get_mut(&id).ok_or_else(|| {
            error(
                "ASTRA_EMU_MINORI_AUDIO_UNKNOWN",
                "audio stream is not loaded",
            )
        })?;
        if let Some(handle) = &mut sound.handle {
            handle.stop(immediate());
        }
        let mut data = sound.data.volume(db(volume)).panning(Panning(pan));
        if repeat {
            data = data.loop_region(0.0..);
        }
        data.settings.fade_in_tween = Some(tween(fade));
        sound.handle = Some(
            self.manager
                .play(data)
                .map_err(|_| error("ASTRA_EMU_MINORI_AUDIO_PLAY", "mixer rejected sound"))?,
        );
        sound.state.volume = volume;
        sound.state.pan = pan;
        sound.state.repeat = repeat;
        sound.state.playing = true;
        Ok(())
    }
    fn apply(
        &mut self,
        command: MinoriAudioCommand,
        archive: &MinoriMountedVfs,
        stop: &AtomicBool,
    ) -> FamilyResult<()> {
        match command {
            MinoriAudioCommand::LoadResource {
                stream_id,
                resource_uri,
                ..
            } => self.load(stream_id, &resource_uri, archive, stop),
            MinoriAudioCommand::Play {
                stream_id,
                volume,
                pan,
                repeat,
                fade_in_ms,
                ..
            } => self.play(stream_id, volume, pan, repeat, fade_in_ms),
            MinoriAudioCommand::Stop {
                stream_id, fade_ms, ..
            } => {
                if let Some(s) = self.sounds.get_mut(&stream_id) {
                    if let Some(h) = &mut s.handle {
                        h.stop(tween(fade_ms));
                    }
                    s.state.playing = false;
                }
                Ok(())
            }
            MinoriAudioCommand::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
                ..
            } => {
                validate_params(volume, pan)?;
                let s = self.sounds.get_mut(&stream_id).ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MINORI_AUDIO_UNKNOWN",
                        "audio stream is not loaded",
                    )
                })?;
                if let Some(h) = &mut s.handle {
                    h.set_volume(db(volume), immediate());
                    h.set_panning(Panning(pan), immediate());
                    if repeat {
                        h.set_loop_region(0.0..)
                    } else {
                        h.set_loop_region(None)
                    }
                }
                s.state.volume = volume;
                s.state.pan = pan;
                s.state.repeat = repeat;
                Ok(())
            }
        }
    }
    fn snapshot(&self) -> Vec<SoundSnapshot> {
        self.sounds
            .values()
            .map(|s| {
                let mut result = s.state.clone();
                if let Some(h) = &s.handle {
                    result.position = h.position();
                    result.playing = h.state() != PlaybackState::Stopped;
                }
                result
            })
            .collect()
    }
    fn restore(
        &mut self,
        snapshot: Vec<SoundSnapshot>,
        archive: &MinoriMountedVfs,
        stop: &AtomicBool,
    ) -> FamilyResult<()> {
        if snapshot.len() > 64 {
            return Err(error(
                "ASTRA_EMU_MINORI_AUDIO_SNAPSHOT",
                "snapshot contains too many sounds",
            ));
        }
        let mut next = Self::new()?;
        for s in snapshot {
            if next.sounds.contains_key(&s.id) || !s.position.is_finite() || s.position < 0.0 {
                return Err(error(
                    "ASTRA_EMU_MINORI_AUDIO_SNAPSHOT",
                    "snapshot sound identity or cursor is invalid",
                ));
            }
            validate_params(s.volume, s.pan)?;
            next.load(s.id, &s.uri, archive, stop)?;
            if s.playing {
                next.play(s.id, s.volume, s.pan, s.repeat, 0)?;
                next.sounds
                    .get_mut(&s.id)
                    .unwrap()
                    .handle
                    .as_mut()
                    .unwrap()
                    .seek_to(s.position);
            }
            let id = s.id;
            next.sounds.get_mut(&id).unwrap().state = s;
        }
        *self = next;
        Ok(())
    }
}
fn run(
    archive: Arc<MinoriMountedVfs>,
    sink: Arc<AudioSinkBox>,
    stop: Arc<AtomicBool>,
    commands: Receiver<Command>,
) -> FamilyResult<()> {
    let mut mixer = Mixer::new()?;
    let mut suspended = false;
    while !stop.load(Ordering::Acquire) {
        for command in commands.try_iter().take(64) {
            match command {
                Command::Apply(list) => {
                    for command in list {
                        mixer.apply(command, &archive, &stop)?
                    }
                }
                Command::Snapshot(reply) => {
                    let _ = reply.send(Ok(mixer.snapshot()));
                }
                Command::Restore(snapshot, reply) => {
                    let result = mixer.restore(snapshot, &archive, &stop);
                    let _ = reply.send(result.clone());
                    result?;
                }
                Command::Suspend(value) => suspended = value,
            }
        }
        let mut samples = vec![0.0; 512 * 2];
        if !suspended {
            let r = mixer.manager.backend_mut().renderer.as_mut().unwrap();
            r.on_start_processing();
            r.process(&mut samples, 2);
        }
        match sink.write(PcmChunk::F32(samples.into())).into_result()? {
            AudioWriteStatus::Accepted => {}
            _ if stop.load(Ordering::Acquire) => break,
            _ => {
                return Err(error(
                    "ASTRA_EMU_MINORI_AUDIO_CLOSED",
                    "host audio queue closed unexpectedly",
                ))
            }
        }
    }
    Ok(())
}
fn validate_params(volume: f32, pan: f32) -> FamilyResult<()> {
    if !volume.is_finite()
        || !(0.0..=1.0).contains(&volume)
        || !pan.is_finite()
        || !(-1.0..=1.0).contains(&pan)
    {
        Err(error(
            "ASTRA_EMU_MINORI_AUDIO_PARAMS",
            "audio volume or pan is invalid",
        ))
    } else {
        Ok(())
    }
}
fn db(volume: f32) -> Decibels {
    if volume <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * volume.log10())
    }
}
fn tween(ms: u32) -> Tween {
    Tween {
        duration: Duration::from_millis(ms.into()),
        ..Default::default()
    }
}
fn immediate() -> Tween {
    tween(0)
}
