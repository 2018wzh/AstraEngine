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
mod fade;
mod mixer;
use fade::FadeSnapshot;
use mixer::Mixer;

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
    pub fade: Option<FadeSnapshot>,
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
            self.fail_request(error(
                "ASTRA_EMU_MINORI_AUDIO_SNAPSHOT",
                "audio snapshot did not complete",
            ))
        })?
    }
    pub fn restore(&self, snapshot: Vec<SoundSnapshot>) -> FamilyResult<()> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Restore(snapshot, tx))?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(|_| {
            self.fail_request(error(
                "ASTRA_EMU_MINORI_AUDIO_RESTORE",
                "audio restore did not complete",
            ))
        })?
    }
    fn fail_request(&self, failure: FamilyError) -> FamilyError {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut slot) = self.failure.lock() {
            if slot.is_none() {
                *slot = Some(failure.clone());
            }
        }
        if let Err(cancel_error) = self.sink.cancel().into_result() {
            return cancel_error;
        }
        failure
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
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            match command {
                Command::Apply(list) => {
                    for command in list {
                        if stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
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
            mixer.render(&mut samples);
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
        Decibels((20.0 * volume.log10()).max(Decibels::SILENCE.0))
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
