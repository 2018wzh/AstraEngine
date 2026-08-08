use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use astra_platform::{AudioOutputLane, PlatformError};
use kira::backend::{Backend, Renderer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioChunkTelemetry {
    pub rendered_samples: u64,
    pub submitted_samples: u64,
    pub consumed_samples: u64,
    pub underflow_count: u64,
    pub render_ns: u64,
    pub submit_wait_ns: u64,
}

pub struct AstraChunkBackendSettings {
    pub sample_rate: u32,
    pub channels: u16,
    pub chunk_frames: usize,
    pub endpoint: Box<dyn AudioOutputLane>,
    pub deterministic_fixed_tick_hz: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum AstraChunkBackendError {
    #[error("audio chunk backend settings are invalid")]
    InvalidSettings,
    #[error(transparent)]
    Endpoint(#[from] PlatformError),
    #[error("audio chunk worker could not be started")]
    WorkerStart,
    #[error("deterministic audio chunk worker missed the fixed-tick deadline")]
    DeterministicDeadline,
}

#[derive(Default)]
struct DeterministicClockState {
    requested_samples: u64,
}

#[derive(Default)]
struct DeterministicClock {
    state: Mutex<DeterministicClockState>,
    wake: Condvar,
}

#[derive(Default)]
struct SharedTelemetry {
    rendered_samples: AtomicU64,
    submitted_samples: AtomicU64,
    consumed_samples: AtomicU64,
    underflow_count: AtomicU64,
    render_ns: AtomicU64,
    submit_wait_ns: AtomicU64,
}

pub struct AstraChunkBackend {
    settings: Option<AstraChunkBackendSettings>,
    stop: Arc<AtomicBool>,
    telemetry: Arc<SharedTelemetry>,
    worker: Option<JoinHandle<Result<(), PlatformError>>>,
    deterministic_clock: Option<Arc<DeterministicClock>>,
    deterministic_samples_per_tick: u64,
    chunk_samples: u64,
}

impl AstraChunkBackend {
    #[must_use]
    pub fn telemetry(&self) -> AudioChunkTelemetry {
        AudioChunkTelemetry {
            rendered_samples: self.telemetry.rendered_samples.load(Ordering::Relaxed),
            submitted_samples: self.telemetry.submitted_samples.load(Ordering::Relaxed),
            consumed_samples: self.telemetry.consumed_samples.load(Ordering::Acquire),
            underflow_count: self.telemetry.underflow_count.load(Ordering::Relaxed),
            render_ns: self.telemetry.render_ns.load(Ordering::Relaxed),
            submit_wait_ns: self.telemetry.submit_wait_ns.load(Ordering::Relaxed),
        }
    }

    pub fn take_worker_error(&mut self) -> Result<(), AstraChunkBackendError> {
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let worker = self.worker.take().expect("finished worker must exist");
            worker
                .join()
                .map_err(|_| AstraChunkBackendError::WorkerStart)??;
        }
        Ok(())
    }

    pub fn advance_fixed_tick(&mut self) -> Result<(), AstraChunkBackendError> {
        let Some(clock) = self.deterministic_clock.clone() else {
            return Ok(());
        };
        self.take_worker_error()?;
        let target = {
            let mut state = clock
                .state
                .lock()
                .map_err(|_| AstraChunkBackendError::DeterministicDeadline)?;
            state.requested_samples = state
                .requested_samples
                .checked_add(self.deterministic_samples_per_tick)
                .ok_or(AstraChunkBackendError::DeterministicDeadline)?;
            let target = state.requested_samples / self.chunk_samples * self.chunk_samples;
            clock.wake.notify_all();
            target
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut state = clock
            .state
            .lock()
            .map_err(|_| AstraChunkBackendError::DeterministicDeadline)?;
        while self.telemetry.submitted_samples.load(Ordering::Acquire) < target {
            let now = Instant::now();
            if now >= deadline {
                return Err(AstraChunkBackendError::DeterministicDeadline);
            }
            let (next, timeout) = clock
                .wake
                .wait_timeout(state, deadline.duration_since(now))
                .map_err(|_| AstraChunkBackendError::DeterministicDeadline)?;
            state = next;
            if timeout.timed_out()
                && self.telemetry.submitted_samples.load(Ordering::Acquire) < target
            {
                return Err(AstraChunkBackendError::DeterministicDeadline);
            }
        }
        drop(state);
        self.take_worker_error()
    }
}

impl Backend for AstraChunkBackend {
    type Settings = AstraChunkBackendSettings;
    type Error = AstraChunkBackendError;

    fn setup(
        settings: Self::Settings,
        _internal_buffer_size: usize,
    ) -> Result<(Self, u32), Self::Error> {
        if settings.sample_rate == 0
            || settings.channels == 0
            || settings.chunk_frames == 0
            || settings
                .deterministic_fixed_tick_hz
                .is_some_and(|hz| hz == 0 || !settings.sample_rate.is_multiple_of(hz))
        {
            return Err(AstraChunkBackendError::InvalidSettings);
        }
        let sample_rate = settings.sample_rate;
        let chunk_samples = settings
            .chunk_frames
            .checked_mul(usize::from(settings.channels))
            .ok_or(AstraChunkBackendError::InvalidSettings)? as u64;
        let deterministic_samples_per_tick = settings
            .deterministic_fixed_tick_hz
            .map(|hz| {
                u64::from(settings.sample_rate / hz)
                    .checked_mul(u64::from(settings.channels))
                    .ok_or(AstraChunkBackendError::InvalidSettings)
            })
            .transpose()?
            .unwrap_or(0);
        let deterministic_clock = settings
            .deterministic_fixed_tick_hz
            .map(|_| Arc::new(DeterministicClock::default()));
        Ok((
            Self {
                settings: Some(settings),
                stop: Arc::new(AtomicBool::new(false)),
                telemetry: Arc::new(SharedTelemetry::default()),
                worker: None,
                deterministic_clock,
                deterministic_samples_per_tick,
                chunk_samples,
            },
            sample_rate,
        ))
    }

    fn start(&mut self, mut renderer: Renderer) -> Result<(), Self::Error> {
        let AstraChunkBackendSettings {
            sample_rate: _,
            channels,
            chunk_frames,
            mut endpoint,
            deterministic_fixed_tick_hz: _,
        } = self
            .settings
            .take()
            .ok_or(AstraChunkBackendError::InvalidSettings)?;
        let sample_count = chunk_frames
            .checked_mul(usize::from(channels))
            .ok_or(AstraChunkBackendError::InvalidSettings)?;
        let stop = Arc::clone(&self.stop);
        let telemetry = Arc::clone(&self.telemetry);
        let deterministic_clock = self.deterministic_clock.clone();
        let worker = thread::Builder::new()
            .name("astra-kira-audio".into())
            .spawn(move || {
                let mut chunk = vec![0.0; sample_count];
                while !stop.load(Ordering::Acquire) {
                    if let Some(clock) = &deterministic_clock {
                        let mut state = clock.state.lock().map_err(|_| {
                            PlatformError::new(
                                astra_platform::PlatformErrorCode::InvalidState,
                                "audio.fixed_tick.wait",
                                "deterministic audio clock lock is poisoned",
                            )
                        })?;
                        while telemetry.submitted_samples.load(Ordering::Acquire)
                            + sample_count as u64
                            > state.requested_samples
                            && !stop.load(Ordering::Acquire)
                        {
                            state = clock.wake.wait(state).map_err(|_| {
                                PlatformError::new(
                                    astra_platform::PlatformErrorCode::InvalidState,
                                    "audio.fixed_tick.wait",
                                    "deterministic audio clock lock is poisoned",
                                )
                            })?;
                        }
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    let wait_started = Instant::now();
                    endpoint.wait_for_capacity(sample_count, &stop)?;
                    telemetry
                        .consumed_samples
                        .store(endpoint.consumed_samples(), Ordering::Release);
                    telemetry
                        .underflow_count
                        .store(endpoint.underflow_count(), Ordering::Relaxed);
                    telemetry
                        .submit_wait_ns
                        .fetch_add(elapsed_ns(wait_started), Ordering::Relaxed);
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let render_started = Instant::now();
                    renderer.on_start_processing();
                    renderer.process(&mut chunk, channels);
                    telemetry
                        .render_ns
                        .fetch_add(elapsed_ns(render_started), Ordering::Relaxed);
                    telemetry
                        .rendered_samples
                        .fetch_add(sample_count as u64, Ordering::Release);
                    chunk = endpoint.submit(chunk)?;
                    telemetry
                        .consumed_samples
                        .store(endpoint.consumed_samples(), Ordering::Release);
                    telemetry
                        .underflow_count
                        .store(endpoint.underflow_count(), Ordering::Relaxed);
                    if chunk.len() != sample_count {
                        return Err(PlatformError::new(
                            astra_platform::PlatformErrorCode::IntegrityMismatch,
                            "audio.lane.recycle",
                            "endpoint returned an allocation with the wrong length",
                        ));
                    }
                    telemetry
                        .submitted_samples
                        .fetch_add(sample_count as u64, Ordering::Relaxed);
                    if let Some(clock) = &deterministic_clock {
                        clock.wake.notify_all();
                    }
                }
                Ok(())
            })
            .map_err(|_| AstraChunkBackendError::WorkerStart)?;
        self.worker = Some(worker);
        Ok(())
    }
}

impl Drop for AstraChunkBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(clock) = &self.deterministic_clock {
            clock.wake.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
