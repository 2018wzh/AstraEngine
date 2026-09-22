use crate::{NativeAudioProducer, NativeAudioQueue};
use astra_platform::{AudioOutputRequest, AudioWakeRegistration, PlatformError, PlatformErrorCode};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

enum Command {
    Pause(mpsc::SyncSender<()>),
    Resume,
    Stop,
}

/// Explicit test output. Consumes the native PCM queue on a sample clock without a device.
/// This selection is process-local and must never serve as a device-error fallback.
pub struct NullAudioDevice {
    commands: mpsc::SyncSender<Command>,
    worker: Option<thread::JoinHandle<()>>,
    wake: AudioWakeRegistration,
}

impl NullAudioDevice {
    pub fn open(
        request: AudioOutputRequest,
        wake: AudioWakeRegistration,
    ) -> Result<(Self, NativeAudioProducer), PlatformError> {
        if request.capture_samples {
            return Err(error("null output does not capture samples"));
        }
        if request.sample_rate == 0
            || request.channels == 0
            || request.chunk_frames == 0
            || request.max_buffered_frames == 0
        {
            return Err(error("null output format and capacity must be non-zero"));
        }
        let samples = request
            .chunk_frames
            .checked_mul(usize::from(request.channels))
            .ok_or_else(|| error("null output chunk overflow"))?;
        let (producer, mut consumer, _) = NativeAudioQueue::create(
            request.max_buffered_frames.div_ceil(request.chunk_frames),
            samples,
            wake.clone(),
        )?;
        let (commands, receiver) = mpsc::sync_channel(1);
        let worker_wake = wake.clone();
        let worker = thread::Builder::new()
            .name("astra-test-null-audio".into())
            .spawn(move || {
                let mut output = vec![0.0; samples];
                let mut paused = request.start_paused;
                let mut origin = Instant::now();
                let mut chunk = 1_u64;
                loop {
                    let command = if paused {
                        receiver.recv().ok()
                    } else {
                        let nanos =
                            u128::from(chunk) * request.chunk_frames as u128 * 1_000_000_000
                                / u128::from(request.sample_rate);
                        let Ok(nanos) = u64::try_from(nanos) else {
                            break;
                        };
                        match receiver.recv_timeout(
                            (origin + Duration::from_nanos(nanos))
                                .saturating_duration_since(Instant::now()),
                        ) {
                            Ok(command) => Some(command),
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                consumer.fill_output_f32(&mut output);
                                worker_wake.notify();
                                chunk += 1;
                                continue;
                            }
                        }
                    };
                    match command {
                        Some(Command::Pause(ack)) => {
                            paused = true;
                            let _ = ack.send(());
                        }
                        Some(Command::Resume) => {
                            paused = false;
                            origin = Instant::now();
                            chunk = 1;
                        }
                        Some(Command::Stop) | None => break,
                    }
                }
                worker_wake.notify();
            })
            .map_err(|_| error("null output worker could not start"))?;
        Ok((
            Self {
                commands,
                worker: Some(worker),
                wake,
            },
            producer,
        ))
    }

    pub fn pause(&self) -> Result<(), PlatformError> {
        let (ack, receive) = mpsc::sync_channel(1);
        self.commands
            .send(Command::Pause(ack))
            .map_err(|_| error("null output worker closed"))?;
        receive
            .recv()
            .map_err(|_| error("null output worker closed"))
    }

    pub fn resume(&self) -> Result<(), PlatformError> {
        self.commands
            .send(Command::Resume)
            .map_err(|_| error("null output worker closed"))
    }
}

impl Drop for NullAudioDevice {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.wake.notify();
    }
}

fn error(message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, "audio.test_null", message)
}
