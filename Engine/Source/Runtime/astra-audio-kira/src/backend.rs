use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Instant,
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
    #[error("deterministic audio sample count overflowed")]
    DeterministicSampleCount,
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
    deterministic_renderer: Option<Renderer>,
    deterministic_endpoint: Option<Box<dyn AudioOutputLane>>,
    deterministic_chunk: Vec<f32>,
    deterministic_channels: u16,
    deterministic_requested_samples: u64,
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
            self.join_worker()?;
        }
        Ok(())
    }

    fn join_worker(&mut self) -> Result<(), AstraChunkBackendError> {
        let worker = self
            .worker
            .take()
            .ok_or(AstraChunkBackendError::WorkerStart)?;
        worker
            .join()
            .map_err(|_| AstraChunkBackendError::WorkerStart)??;
        Ok(())
    }

    pub fn advance_fixed_tick(&mut self) -> Result<(), AstraChunkBackendError> {
        if self.deterministic_renderer.is_none() {
            return Ok(());
        }
        self.deterministic_requested_samples = self
            .deterministic_requested_samples
            .checked_add(self.deterministic_samples_per_tick)
            .ok_or(AstraChunkBackendError::DeterministicSampleCount)?;
        let target = self.deterministic_requested_samples / self.chunk_samples * self.chunk_samples;
        while self.telemetry.submitted_samples.load(Ordering::Acquire) < target {
            let endpoint = self
                .deterministic_endpoint
                .as_mut()
                .ok_or(AstraChunkBackendError::InvalidSettings)?;
            let renderer = self
                .deterministic_renderer
                .as_mut()
                .ok_or(AstraChunkBackendError::InvalidSettings)?;
            let wait_started = Instant::now();
            endpoint.wait_for_capacity(self.chunk_samples as usize, &self.stop)?;
            self.telemetry
                .consumed_samples
                .store(endpoint.consumed_samples(), Ordering::Release);
            self.telemetry
                .underflow_count
                .store(endpoint.underflow_count(), Ordering::Relaxed);
            self.telemetry
                .submit_wait_ns
                .fetch_add(elapsed_ns(wait_started), Ordering::Relaxed);
            let render_started = Instant::now();
            renderer.on_start_processing();
            renderer.process(&mut self.deterministic_chunk, self.deterministic_channels);
            self.telemetry
                .render_ns
                .fetch_add(elapsed_ns(render_started), Ordering::Relaxed);
            self.telemetry
                .rendered_samples
                .fetch_add(self.chunk_samples, Ordering::Release);
            let chunk = std::mem::take(&mut self.deterministic_chunk);
            self.deterministic_chunk = endpoint.submit(chunk)?;
            if self.deterministic_chunk.len() as u64 != self.chunk_samples {
                return Err(AstraChunkBackendError::Endpoint(PlatformError::new(
                    astra_platform::PlatformErrorCode::IntegrityMismatch,
                    "audio.lane.recycle",
                    "endpoint returned an allocation with the wrong length",
                )));
            }
            self.telemetry
                .consumed_samples
                .store(endpoint.consumed_samples(), Ordering::Release);
            self.telemetry
                .underflow_count
                .store(endpoint.underflow_count(), Ordering::Relaxed);
            self.telemetry
                .submitted_samples
                .fetch_add(self.chunk_samples, Ordering::Release);
        }
        Ok(())
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
        Ok((
            Self {
                settings: Some(settings),
                stop: Arc::new(AtomicBool::new(false)),
                telemetry: Arc::new(SharedTelemetry::default()),
                worker: None,
                deterministic_renderer: None,
                deterministic_endpoint: None,
                deterministic_chunk: Vec::new(),
                deterministic_channels: 0,
                deterministic_requested_samples: 0,
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
            endpoint,
            deterministic_fixed_tick_hz,
        } = self
            .settings
            .take()
            .ok_or(AstraChunkBackendError::InvalidSettings)?;
        let sample_count = chunk_frames
            .checked_mul(usize::from(channels))
            .ok_or(AstraChunkBackendError::InvalidSettings)?;
        if deterministic_fixed_tick_hz.is_some() {
            self.deterministic_renderer = Some(renderer);
            self.deterministic_endpoint = Some(endpoint);
            self.deterministic_chunk = vec![0.0; sample_count];
            self.deterministic_channels = channels;
            return Ok(());
        }
        let mut endpoint = endpoint;
        let stop = Arc::clone(&self.stop);
        let telemetry = Arc::clone(&self.telemetry);
        let worker = thread::Builder::new()
            .name("astra-kira-audio".into())
            .spawn(move || {
                (|| {
                    let mut chunk = vec![0.0; sample_count];
                    while !stop.load(Ordering::Acquire) {
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
                    }
                    Ok(())
                })()
            })
            .map_err(|_| AstraChunkBackendError::WorkerStart)?;
        self.worker = Some(worker);
        Ok(())
    }
}

impl Drop for AstraChunkBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
