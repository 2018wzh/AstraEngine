use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[path = "audio_conversion.rs"]
mod audio_conversion;
#[path = "audio_device.rs"]
mod audio_device;
#[path = "audio_progress.rs"]
mod audio_progress;
pub(crate) use audio_device::AudioDeviceKind;
use audio_device::AudioOutputDevice;

use abi_stable::type_level::downcasting::TD_Opaque;
#[cfg(test)]
use astra_emu_family_api::PcmFormat;
use astra_emu_family_api::{
    AudioPlaybackClock, AudioPlaybackProgress, AudioSink, AudioSinkBox, AudioSink_TO,
    AudioWriteStatus, PcmChunk, PcmFormatSpec, MAX_AUDIO_SAMPLES_PER_CHUNK,
};
#[cfg(test)]
use audio_conversion::{convert_chunk, convert_samples};
use audio_conversion::{pcm_chunk_samples, AudioConverter, OutputFormat};
use audio_progress::{PlaybackCounters, PlaybackTracker};
use crossbeam_channel::{bounded, select, Receiver, Sender};

const AUDIO_QUEUE_CHUNKS: usize = 4;
const MAX_AUDIO_OUTPUT_SAMPLES: usize = MAX_AUDIO_SAMPLES_PER_CHUNK;

/// Host-owned audio output. The family only sees the ABI sink; it never sees
/// CPAL, the device callback, or a native handle. `configure` is lazy because
/// the family declares its source format during open.
pub(crate) struct HostAudioExecutor {
    state: Arc<AudioState>,
}

struct AudioState {
    device_kind: AudioDeviceKind,
    wake: Option<astra_emu_manager::HostWake>,
    cancelled: AtomicBool,
    closed: AtomicBool,
    stream_failed: AtomicBool,
    progress_failed: AtomicBool,
    progress: Arc<PlaybackCounters>,
    configured: Mutex<Option<OutputFormat>>,
    producer: Mutex<Option<Sender<Vec<f32>>>>,
    cancel_waiters: Mutex<Vec<Sender<()>>>,
    stream: Mutex<Option<AudioOutputDevice>>,
    converter: Mutex<Option<AudioConverter>>,
}

type CancelWaiter = (Sender<()>, Receiver<()>);

struct HostAudioSink {
    state: Arc<AudioState>,
}

struct DeviceConsumer {
    receiver: Receiver<Vec<f32>>,
    current: Option<Vec<f32>>,
    cursor: usize,
    channels: usize,
    tracker: Option<PlaybackTracker>,
    last_callback: Option<cpal::StreamInstant>,
}

impl HostAudioExecutor {
    pub(crate) fn new(
        device_kind: AudioDeviceKind,
        wake: Option<astra_emu_manager::HostWake>,
    ) -> Self {
        Self {
            state: Arc::new(AudioState {
                device_kind,
                wake,
                cancelled: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                stream_failed: AtomicBool::new(false),
                progress_failed: AtomicBool::new(false),
                progress: Arc::new(PlaybackCounters::default()),
                configured: Mutex::new(None),
                producer: Mutex::new(None),
                cancel_waiters: Mutex::new(Vec::new()),
                stream: Mutex::new(None),
                converter: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn sink(&self) -> AudioSinkBox {
        AudioSink_TO::from_value(
            HostAudioSink {
                state: Arc::clone(&self.state),
            },
            TD_Opaque,
        )
    }

    /// Stop the device before releasing the family session. The cancellation
    /// signal disconnects blocked writes; no fixed tick or callback is needed.
    pub(crate) fn close(&self) -> Result<(), String> {
        self.state.cancelled.store(true, Ordering::Release);
        self.state.closed.store(true, Ordering::Release);
        wake_cancel_waiters(&self.state)?;
        self.state
            .producer
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())?
            .take();
        let stream = self
            .state
            .stream
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_LOCK".to_owned())?
            .take();
        if let Some(stream) = stream {
            stream.close()?;
        }
        self.check_health()
    }

    pub(crate) fn check_health(&self) -> Result<(), String> {
        if self.state.progress_failed.load(Ordering::Acquire) {
            Err("ASTRA_EMU_AUDIO_PROGRESS_INVALID".into())
        } else if self.state.stream_failed.load(Ordering::Acquire) {
            Err("ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED".into())
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn is_configured(&self) -> bool {
        self.state
            .configured
            .lock()
            .expect("audio state lock")
            .is_some()
    }
}

impl Drop for HostAudioExecutor {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl HostAudioSink {
    fn configure_inner(&self, source: PcmFormatSpec) -> Result<(), String> {
        source.validate().map_err(|error| error.to_string())?;
        if self.state.cancelled.load(Ordering::Acquire) || self.state.closed.load(Ordering::Acquire)
        {
            return Err("ASTRA_EMU_AUDIO_CLOSED".into());
        }
        // Hold the configuration guard through publication: one sink has one
        // source format and one device timeline, even with competing callers.
        let mut configured = self
            .state
            .configured
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONFIG_LOCK".to_owned())?;
        if configured.is_some() {
            return Err("ASTRA_EMU_AUDIO_ALREADY_CONFIGURED".into());
        }

        let (producer, receiver) = bounded(AUDIO_QUEUE_CHUNKS);
        let consumer = DeviceConsumer {
            receiver,
            current: None,
            cursor: 0,
            channels: 1,
            tracker: None,
            last_callback: None,
        };
        let (output, stream) = AudioOutputDevice::open(
            self.state.device_kind,
            source,
            consumer,
            Arc::clone(&self.state),
        )?;
        let converter = AudioConverter::new(output)?;
        self.state
            .progress
            .resampler_delay
            .store(converter.output_delay() as u64, Ordering::Release);

        *self
            .state
            .converter
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONVERTER_LOCK".to_owned())? = Some(converter);
        *self
            .state
            .producer
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())? = Some(producer);
        let mut active_stream = self
            .state
            .stream
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_STREAM_LOCK".to_owned())?;
        if self.state.cancelled.load(Ordering::Acquire) || self.state.closed.load(Ordering::Acquire)
        {
            stream.close()?;
            return Err("ASTRA_EMU_AUDIO_CLOSED".into());
        }
        *active_stream = Some(stream);
        *configured = Some(output);
        tracing::info!(
            event = "astra.emu.audio.configured",
            device_kind = self.state.device_kind.as_str(),
            source_rate = source.sample_rate,
            source_channels = source.channels,
            device_rate = output.device_rate,
            device_channels = output.device_channels,
        );
        Ok(())
    }

    fn write_inner(&self, chunk: PcmChunk) -> Result<AudioWriteStatus, String> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return Ok(AudioWriteStatus::Cancelled);
        }
        if self.state.closed.load(Ordering::Acquire) {
            return Ok(AudioWriteStatus::Closed);
        }
        let output = (*self
            .state
            .configured
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONFIG_LOCK".to_owned())?)
        .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?;
        chunk
            .validate(output.source)
            .map_err(|error| error.to_string())?;
        let source_frames = (chunk.sample_count() / usize::from(output.source.channels)) as u64;
        // Serialize conversion through queue publication, preserving the PCM
        // order even if a Family uses multiple writers.
        let mut converter = self
            .state
            .converter
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONVERTER_LOCK".to_owned())?;
        self.state.progress.check_accept(source_frames)?;
        let samples = converter
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?
            .push(pcm_chunk_samples(chunk))?;
        if samples.is_empty() {
            // Rubato may retain this entire input block until its next window.
            // Empty output is accepted source PCM, not a device queue entry.
            if self.state.cancelled.load(Ordering::Acquire) {
                return Ok(AudioWriteStatus::Cancelled);
            }
            self.state.progress.accept(source_frames);
            return Ok(AudioWriteStatus::Accepted);
        }
        let producer = self
            .state
            .producer
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?;
        // `select!` waits on either queue capacity or cancellation. It never
        // polls the queue with a timer, so Family workers stop promptly when
        // the Manager closes the session.
        let Some((cancel_tx, cancel_rx)) = register_cancel_waiter(&self.state)? else {
            return Ok(AudioWriteStatus::Cancelled);
        };
        let result = select! {
            send(producer, samples) -> result => result.map(|_| AudioWriteStatus::Accepted).map_err(|_| "ASTRA_EMU_AUDIO_QUEUE_CLOSED".into()),
            recv(cancel_rx) -> _ => Ok(AudioWriteStatus::Cancelled),
        };
        if matches!(result, Ok(AudioWriteStatus::Accepted)) {
            self.state.progress.accept(source_frames);
        }
        unregister_cancel_waiter(&self.state, &cancel_tx)?;
        result
    }
}

impl AudioSink for HostAudioSink {
    fn configure(&self, format: PcmFormatSpec) -> astra_emu_family_api::FfiFamilyResult<()> {
        self.configure_inner(format)
            .map_or_else(
                |error| Err(audio_error("ASTRA_EMU_AUDIO_CONFIG", error)),
                |_| Ok(()),
            )
            .into()
    }

    fn write(&self, chunk: PcmChunk) -> astra_emu_family_api::FfiFamilyResult<AudioWriteStatus> {
        self.write_inner(chunk)
            .map_err(|error| {
                // A converter/queue failure may have retained or discarded PCM.
                // End this timeline; no later query may claim it was played.
                self.state.cancelled.store(true, Ordering::Release);
                wake_cancel_waiters_best_effort(&self.state);
                audio_error("ASTRA_EMU_AUDIO_WRITE", error)
            })
            .into()
    }

    fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire) || self.state.closed.load(Ordering::Acquire)
    }

    fn cancel(&self) -> astra_emu_family_api::FfiFamilyResult<()> {
        self.state.cancelled.store(true, Ordering::Release);
        wake_cancel_waiters(&self.state)
            .map_or_else(
                |error| Err(audio_error("ASTRA_EMU_AUDIO_CANCEL", error)),
                |_| Ok(()),
            )
            .into()
    }

    fn playback_progress(&self) -> astra_emu_family_api::FfiFamilyResult<AudioPlaybackProgress> {
        self.progress_inner()
            .map_err(|code| audio_error(code, "audio playback progress is unavailable".into()))
            .into()
    }
}

impl HostAudioSink {
    fn progress_inner(&self) -> Result<AudioPlaybackProgress, &'static str> {
        let state = &self.state;
        if state.progress_failed.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_AUDIO_PROGRESS_INVALID");
        }
        if state.stream_failed.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED");
        }
        if state.closed.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_AUDIO_CLOSED");
        }
        if state.cancelled.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_AUDIO_CANCELLED");
        }
        if state
            .configured
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_CONFIG_LOCK")?
            .is_none()
        {
            return Err("ASTRA_EMU_AUDIO_NOT_CONFIGURED");
        }
        let clock = match state.device_kind {
            AudioDeviceKind::Null => AudioPlaybackClock::TestConsumption,
            AudioDeviceKind::Native if cfg!(target_os = "windows") => {
                AudioPlaybackClock::DeviceTimestamp
            }
            AudioDeviceKind::Native => return Err("ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED"),
        };
        Ok(state.progress.snapshot(clock))
    }
}
fn audio_error(code: &str, message: String) -> astra_emu_family_api::FamilyError {
    astra_emu_family_api::FamilyError::new(code, message)
}

fn register_cancel_waiter(state: &AudioState) -> Result<Option<CancelWaiter>, String> {
    let (sender, receiver) = bounded(1);
    let mut waiters = state
        .cancel_waiters
        .lock()
        .map_err(|_| "ASTRA_EMU_AUDIO_CANCEL_LOCK".to_owned())?;
    if state.cancelled.load(Ordering::Acquire) || state.closed.load(Ordering::Acquire) {
        return Ok(None);
    }
    waiters.push(sender.clone());
    Ok(Some((sender, receiver)))
}

fn unregister_cancel_waiter(state: &AudioState, sender: &Sender<()>) -> Result<(), String> {
    let mut waiters = state
        .cancel_waiters
        .lock()
        .map_err(|_| "ASTRA_EMU_AUDIO_CANCEL_LOCK".to_owned())?;
    waiters.retain(|waiter| !waiter.same_channel(sender));
    Ok(())
}

fn wake_cancel_waiters(state: &AudioState) -> Result<(), String> {
    let waiters = state
        .cancel_waiters
        .lock()
        .map_err(|_| "ASTRA_EMU_AUDIO_CANCEL_LOCK".to_owned())?;
    for waiter in waiters.iter() {
        let _ = waiter.try_send(());
    }
    Ok(())
}

fn wake_cancel_waiters_best_effort(state: &AudioState) {
    if let Err(error) = wake_cancel_waiters(state) {
        tracing::error!(
            event = "astra.emu.audio.cancel_waiters_failed",
            diagnostic_code = "ASTRA_EMU_AUDIO_CANCEL_LOCK",
            error_kind = %error,
        );
    }
}

impl DeviceConsumer {
    fn configure_tracking(&mut self, output: OutputFormat, state: &AudioState) {
        self.channels = usize::from(output.device_channels);
        if state.device_kind == AudioDeviceKind::Null || cfg!(target_os = "windows") {
            self.tracker = Some(PlaybackTracker::new(
                Arc::clone(&state.progress),
                output.source.sample_rate,
                output.device_rate,
            ));
        }
    }

    fn observe_device(&mut self, info: &cpal::OutputCallbackInfo) -> Result<(), &'static str> {
        if let Some(tracker) = &mut self.tracker {
            let timestamp = info.timestamp();
            if self.last_callback.is_some_and(|previous| {
                timestamp
                    .callback
                    .checked_duration_since(previous)
                    .is_none()
            }) {
                return Err("ASTRA_EMU_AUDIO_PROGRESS_TIMESTAMP_REGRESSED");
            }
            self.last_callback = Some(timestamp.callback);
            let delay = timestamp
                .playback
                .checked_duration_since(timestamp.callback)
                .ok_or("ASTRA_EMU_AUDIO_PROGRESS_TIMESTAMP")?;
            tracker.observe_device_delay(delay)?;
        }
        Ok(())
    }

    fn observe_null(&mut self) -> Result<(), &'static str> {
        if let Some(tracker) = &mut self.tracker {
            tracker.observe_test_consumption()?;
        }
        Ok(())
    }

    fn next(&mut self) -> Option<f32> {
        loop {
            if let Some(current) = self.current.as_ref() {
                if self.cursor < current.len() {
                    let value = current[self.cursor];
                    self.cursor += 1;
                    return Some(value);
                }
            }
            match self.receiver.try_recv() {
                Ok(block) => {
                    self.current = Some(block);
                    self.cursor = 0;
                }
                Err(_) => {
                    self.current = None;
                    return None;
                }
            }
        }
    }

    fn fill_f32(&mut self, output: &mut [f32]) -> Result<(), &'static str> {
        self.fill(output, |sample| sample)
    }

    fn fill<T: Copy>(
        &mut self,
        output: &mut [T],
        convert: impl Fn(f32) -> T,
    ) -> Result<(), &'static str> {
        output.fill(convert(0.0));
        if !output.len().is_multiple_of(self.channels) {
            return Err("ASTRA_EMU_AUDIO_PROGRESS_ALIGNMENT");
        }
        for frame in output.chunks_exact_mut(self.channels) {
            let first = self.next();
            frame[0] = convert(first.unwrap_or(0.0));
            if first.is_some() {
                for sample in &mut frame[1..] {
                    let value = self.next().ok_or("ASTRA_EMU_AUDIO_PROGRESS_ALIGNMENT")?;
                    *sample = convert(value);
                }
            }
            if let Some(tracker) = &mut self.tracker {
                tracker.submitted_frame(first.is_some())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "audio_executor_tests.rs"]
mod tests;
