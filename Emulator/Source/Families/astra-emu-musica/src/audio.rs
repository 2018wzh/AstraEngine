use crate::{
    scene::{error, read_asset},
    MusicaAudioCommand, MusicaMountedVfs,
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
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
mod device;
mod fade;
mod mixer;
mod preferences;
pub(crate) use preferences::AudioPreferences;
#[cfg(feature = "ffmpeg-vcpkg")]
mod movie;
use fade::FadeSnapshot;
use mixer::Mixer;
#[cfg(feature = "ffmpeg-vcpkg")]
pub(crate) use movie::MoviePcm;

pub(crate) const FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48000,
    channels: 2,
    format: PcmFormat::F32,
};
const MAX_FRAMES: usize = 32 * 1024 * 1024;
pub(crate) const COMMAND_CAPACITY: usize = 64;
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
    Apply(Vec<MusicaAudioCommand>),
    Snapshot(SyncSender<FamilyResult<Vec<SoundSnapshot>>>),
    Restore(Vec<SoundSnapshot>, SyncSender<FamilyResult<()>>),
    Suspend(bool),
    Preferences(AudioPreferences),
    Duration(u32, SyncSender<FamilyResult<u32>>),
}
pub(crate) struct Audio {
    #[cfg(feature = "ffmpeg-vcpkg")]
    movie: Arc<Mutex<Option<Arc<MoviePcm>>>>,
    commands: SyncSender<Command>,
    sink: Option<Arc<AudioSinkBox>>,
    stop: Arc<AtomicBool>,
    backpressured: AtomicBool,
    failure: Arc<Mutex<Option<FamilyError>>>,
    worker: Option<JoinHandle<()>>,
}
impl Audio {
    pub fn start(archive: Arc<MusicaMountedVfs>, sink: Option<AudioSinkBox>) -> FamilyResult<Self> {
        if let Some(sink) = &sink {
            sink.configure(FORMAT).into_result()?;
        }
        let sink = sink.map(Arc::new);
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let (tx, rx) = sync_channel(COMMAND_CAPACITY);
        let (ready_tx, ready_rx) = sync_channel(1);
        let (s, c, f) = (sink.clone(), stop.clone(), failure.clone());
        #[cfg(feature = "ffmpeg-vcpkg")]
        let movie = Arc::new(Mutex::new(None));
        #[cfg(feature = "ffmpeg-vcpkg")]
        let movie_worker = movie.clone();
        let worker = thread::Builder::new()
            .name("musica-audio".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        archive,
                        s.clone(),
                        c.clone(),
                        &rx,
                        &ready_tx,
                        #[cfg(feature = "ffmpeg-vcpkg")]
                        movie_worker,
                    )
                }));
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some(error(
                        "ASTRA_EMU_MUSICA_AUDIO_PANIC",
                        "audio worker panicked",
                    )),
                };
                if let Some(cause) = &error {
                    let _ = ready_tx.try_send(Err(cause.clone()));
                }
                if error.is_some() {
                    if let Ok(mut slot) = f.lock() {
                        if slot.is_none() {
                            *slot = error;
                        }
                    }
                }
            })
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_AUDIO_THREAD",
                    "audio worker could not start",
                )
            })?;
        if let Err(cause) = ready_rx.recv().unwrap_or_else(|_| {
            Err(error(
                "ASTRA_EMU_MUSICA_AUDIO_START",
                "audio worker exited during initialization",
            ))
        }) {
            stop.store(true, Ordering::Release);
            let _ = worker.join();
            return Err(cause);
        }
        Ok(Self {
            #[cfg(feature = "ffmpeg-vcpkg")]
            movie,
            commands: tx,
            sink,
            stop,
            backpressured: AtomicBool::new(false),
            failure,
            worker: Some(worker),
        })
    }
    pub fn check(&self) -> FamilyResult<()> {
        self.failure
            .lock()
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_AUDIO_STATE",
                    "audio worker state poisoned",
                )
            })?
            .clone()
            .map_or(Ok(()), Err)
    }
    fn send(&self, command: Command) -> FamilyResult<()> {
        self.check()?;
        if self.stop.load(Ordering::Acquire) {
            return Err(cancelled_error());
        }
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(command)) => {
                self.backpressured.store(true, Ordering::Release);
                self.commands.send(command).map_err(|_| self.unavailable())
            }
            Err(TrySendError::Disconnected(_)) => Err(self.unavailable()),
        }
    }
    fn unavailable(&self) -> FamilyError {
        self.check().err().unwrap_or_else(|| {
            if self.stop.load(Ordering::Acquire) {
                cancelled_error()
            } else {
                error(
                    "ASTRA_EMU_MUSICA_AUDIO_QUEUE",
                    "audio command queue is closed",
                )
            }
        })
    }
    pub(crate) fn take_backpressure(&self) -> bool {
        self.backpressured.swap(false, Ordering::AcqRel)
    }
    pub fn apply(&self, commands: Vec<MusicaAudioCommand>) -> FamilyResult<()> {
        if commands.is_empty() {
            self.check()
        } else {
            self.send(Command::Apply(commands))
        }
    }
    pub fn set_preferences(&self, preferences: AudioPreferences) -> FamilyResult<()> {
        preferences.gains()?;
        self.send(Command::Preferences(preferences))
    }
    pub fn suspend(&self, value: bool) -> FamilyResult<()> {
        self.send(Command::Suspend(value))
    }
    pub(crate) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
    pub fn duration(&self, stream: u32) -> FamilyResult<Receiver<FamilyResult<u32>>> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Duration(stream, tx))?;
        Ok(rx)
    }
    pub fn snapshot(&self) -> FamilyResult<Vec<SoundSnapshot>> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Snapshot(tx))?;
        rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
            if let Err(cause) = self.check() {
                return cause;
            }
            if self.stop.load(Ordering::Acquire) {
                return cancelled_error();
            }
            self.fail_request(error(
                "ASTRA_EMU_MUSICA_AUDIO_SNAPSHOT",
                "audio snapshot did not complete",
            ))
        })?
    }
    pub fn restore(&self, snapshot: Vec<SoundSnapshot>) -> FamilyResult<()> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Restore(snapshot, tx))?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(|_| {
            if let Err(cause) = self.check() {
                return cause;
            }
            if self.stop.load(Ordering::Acquire) {
                return cancelled_error();
            }
            self.fail_request(error(
                "ASTRA_EMU_MUSICA_AUDIO_RESTORE",
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
        if let Some(sink) = &self.sink {
            if let Err(cancel_error) = sink.cancel().into_result() {
                return cancel_error;
            }
        }
        failure
    }
    pub fn shutdown(&mut self) -> FamilyResult<()> {
        if self.worker.is_none() {
            return self.check();
        }
        self.stop.store(true, Ordering::Release);
        let cancel = self
            .sink
            .as_ref()
            .map(|sink| sink.cancel().into_result())
            .transpose();
        let joined = self
            .worker
            .take()
            .map(|w| w.join())
            .transpose()
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_AUDIO_JOIN",
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
    archive: Arc<MusicaMountedVfs>,
    sink: Option<Arc<AudioSinkBox>>,
    stop: Arc<AtomicBool>,
    commands: &Receiver<Command>,
    ready: &SyncSender<FamilyResult<()>>,
    #[cfg(feature = "ffmpeg-vcpkg")] movie: Arc<Mutex<Option<Arc<MoviePcm>>>>,
) -> FamilyResult<()> {
    let mut mixer = Mixer::new()?;
    let mut device = if sink.is_none() {
        Some(device::DeviceOutput::open(stop.clone())?)
    } else {
        None
    };
    let _ = ready.send(Ok(()));
    let mut samples = vec![0.0; 512 * 2];
    let mut suspended = false;
    'running: while !stop.load(Ordering::Acquire) {
        for command in commands.try_iter().take(COMMAND_CAPACITY) {
            if stop.load(Ordering::Acquire) {
                break 'running;
            }
            match command {
                Command::Apply(list) => {
                    for command in list {
                        if stop.load(Ordering::Acquire) {
                            break 'running;
                        }
                        if mixer.apply(command, &archive, &stop)?.is_none() {
                            break 'running;
                        }
                    }
                }
                Command::Snapshot(reply) => {
                    let _ = reply.send(Ok(mixer.snapshot()));
                }
                Command::Restore(snapshot, reply) => {
                    match mixer.restore(snapshot, &archive, &stop) {
                        Ok(Some(())) => {
                            let _ = reply.send(Ok(()));
                        }
                        Ok(None) => {
                            let _ = reply.send(Err(cancelled_error()));
                            break 'running;
                        }
                        Err(cause) => {
                            let _ = reply.send(Err(cause.clone()));
                            return Err(cause);
                        }
                    }
                }
                Command::Suspend(value) => suspended = value,
                Command::Preferences(value) => mixer.set_preferences(value)?,
                Command::Duration(stream, reply) => {
                    let result = mixer.duration_ms(stream);
                    let _ = reply.send(result.clone());
                    result?;
                }
            }
        }
        samples.fill(0.0);
        if !suspended {
            mixer.render(&mut samples);
        }
        #[cfg(feature = "ffmpeg-vcpkg")]
        let movie_mix = if !suspended {
            let track = movie
                .lock()
                .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_STATE", "movie audio state poisoned"))?
                .clone();
            track
                .map(|track| track.mix(&mut samples).map(|position| (track, position)))
                .transpose()?
        } else {
            None
        };
        let status = if let Some(device) = &mut device {
            let Some(next) = device.write(samples, &stop)? else {
                break;
            };
            samples = next;
            AudioWriteStatus::Accepted
        } else {
            let status = sink
                .as_ref()
                .unwrap()
                .write(PcmChunk::F32(samples.into()))
                .into_result()?;
            samples = vec![0.0; 512 * 2];
            status
        };
        match status {
            AudioWriteStatus::Accepted =>
            {
                #[cfg(feature = "ffmpeg-vcpkg")]
                if let Some((track, position)) = movie_mix {
                    track.accept(position);
                }
            }
            _ if stop.load(Ordering::Acquire) => break,
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_AUDIO_CLOSED",
                    "host audio queue closed unexpectedly",
                ))
            }
        }
    }
    if let Some(device) = device {
        device.check()?;
    }
    Ok(())
}
fn cancelled_error() -> FamilyError {
    error("ASTRA_EMU_AUDIO_CANCELLED", "audio operation cancelled")
}
fn validate_params(volume: f32, pan: f32) -> FamilyResult<()> {
    if !volume.is_finite()
        || !(0.0..=1.0).contains(&volume)
        || !pan.is_finite()
        || !(-1.0..=1.0).contains(&pan)
    {
        Err(error(
            "ASTRA_EMU_MUSICA_AUDIO_PARAMS",
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
