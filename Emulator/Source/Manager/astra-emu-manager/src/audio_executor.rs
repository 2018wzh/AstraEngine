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
use audio_conversion::convert_chunk;
use audio_conversion::{AudioConverter, OutputFormat, OutputPcm};
use audio_progress::{PlaybackCounters, PlaybackTracker};
use crossbeam_channel::{bounded, Receiver, Sender};

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
    producer: Mutex<Option<Sender<OutputPcm>>>,
    cancel_sender: parking_lot::Mutex<Option<Sender<()>>>,
    cancel_receiver: Receiver<()>,
    retired: parking_lot::Mutex<Option<rtrb::Consumer<OutputPcm>>>,
    receiver_guard: parking_lot::Mutex<Option<Receiver<OutputPcm>>>,
    consumer_done: parking_lot::Mutex<Option<Receiver<()>>>,
    stream: Mutex<Option<AudioOutputDevice>>,
    converter: Mutex<Option<AudioConverter>>,
}


struct HostAudioSink {
    state: Arc<AudioState>,
}

struct DeviceConsumer {
    receiver: Receiver<OutputPcm>,
    current: Option<OutputPcm>,
    retired: rtrb::Producer<OutputPcm>,
    cursor: usize,
    channels: usize,
    tracker: Option<PlaybackTracker>,
    last_callback: Option<cpal::StreamInstant>,
    // Last field: notify close only after queue and retirement producers drop.
    _completion: ConsumerCompletion,
}

impl HostAudioExecutor {
    pub(crate) fn new(
        device_kind: AudioDeviceKind,
        wake: Option<astra_emu_manager::HostWake>,
    ) -> Self {
        let (cancel_sender, cancel_receiver) = bounded(0);
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
                cancel_sender: parking_lot::Mutex::new(Some(cancel_sender)),
                cancel_receiver,
                retired: parking_lot::Mutex::new(None),
                receiver_guard: parking_lot::Mutex::new(None),
                consumer_done: parking_lot::Mutex::new(None),
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

    /// Cancel blocked writes, stop the device and reclaim its CPU owners off callback.
    pub(crate) fn close(&self) -> Result<(), String> {
        self.state.cancelled.store(true, Ordering::Release);
        self.state.closed.store(true, Ordering::Release);
        cancel_writes(&self.state);
        // A poisoned ownership lock is still an error, but cannot skip teardown.
        let producer_error = match self.state.producer.lock() {
            Ok(mut producer) => { producer.take(); None }
            Err(poison) => {
                poison.into_inner().take();
                Some("ASTRA_EMU_AUDIO_PRODUCER_LOCK".to_owned())
            }
        };
        let (stream, stream_lock_error) = match self.state.stream.lock() {
            Ok(mut stream) => (stream.take(), None),
            Err(poison) => (poison.into_inner().take(), Some("ASTRA_EMU_AUDIO_STREAM_LOCK".to_owned())),
        };
        let stream_error = stream.and_then(|stream| stream.close().err());
        let done_error = self.state.consumer_done.lock().take()
            .and_then(|done| done.recv().err())
            .map(|_| "ASTRA_EMU_AUDIO_CONSUMER_SHUTDOWN".to_owned());
        // Serialize with the last admitted producer before freeing queued owners.
        let (mut converter, converter_error) = match self.state.converter.lock() {
            Ok(converter) => (converter, None),
            Err(poison) => (poison.into_inner(), Some("ASTRA_EMU_AUDIO_CONVERTER_LOCK".to_owned())),
        };
        converter.take();
        reclaim_retired(&self.state);
        if let Some(receiver) = self.state.receiver_guard.lock().take() {
            while receiver.try_recv().is_ok() {}
        }
        [producer_error, stream_lock_error, stream_error, done_error, converter_error, self.check_health().err()]
            .into_iter().flatten().reduce(|first, next| format!("{first}; {next}"))
            .map_or(Ok(()), Err)
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
        // At most queue + current + one serial blocked producer can retire
        // between producer reclamations. The callback never frees a PCM owner.
        let (retired, reclaimer) = rtrb::RingBuffer::new(AUDIO_QUEUE_CHUNKS + 2);
        let (completion, done) = bounded(1);
        *self.state.retired.lock() = Some(reclaimer);
        *self.state.receiver_guard.lock() = Some(receiver.clone());
        *self.state.consumer_done.lock() = Some(done);
        let consumer = DeviceConsumer {
            receiver,
            current: None,
            retired,
            cursor: 0,
            channels: 1,
            tracker: None,
            last_callback: None,
            _completion: ConsumerCompletion(Some(completion)),
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
        if self.state.cancelled.load(Ordering::Acquire) {
            return Ok(AudioWriteStatus::Cancelled);
        }
        reclaim_retired(&self.state);
        self.state.progress.check_accept(source_frames)?;
        let samples = converter
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_NOT_CONFIGURED".to_owned())?
            .push_chunk(chunk)?;
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
        // Disconnecting the session's one-shot signal wakes every blocked writer.
        // No per-chunk waiter channel/allocation; mixer-local cancellation stays
        // in its Family scope and never disconnects this session signal.
        let result = crossbeam_channel::select_biased! {
            recv(self.state.cancel_receiver) -> _ => Ok(AudioWriteStatus::Cancelled),
            send(producer, samples) -> result => result.map(|_| AudioWriteStatus::Accepted).map_err(|_| "ASTRA_EMU_AUDIO_QUEUE_CLOSED".into()),
        };
        if matches!(result, Ok(AudioWriteStatus::Accepted)) {
            self.state.progress.accept(source_frames);
        }
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
                cancel_writes(&self.state);
                audio_error("ASTRA_EMU_AUDIO_WRITE", error)
            })
            .into()
    }

    fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire) || self.state.closed.load(Ordering::Acquire)
    }

    fn cancel(&self) -> astra_emu_family_api::FfiFamilyResult<()> {
        self.state.cancelled.store(true, Ordering::Release);
        cancel_writes(&self.state);
        Ok(()).into()
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

fn cancel_writes(state: &AudioState) {
    state.cancel_sender.lock().take();
}

fn reclaim_retired(state: &AudioState) {
    let mut retired = state.retired.lock();
    if let Some(retired) = retired.as_mut() {
        while retired.pop().is_ok() {}
    }
}

struct ConsumerCompletion(Option<Sender<()>>);
impl Drop for ConsumerCompletion {
    fn drop(&mut self) {
        if let Some(done) = self.0.take() {
            let _ = done.try_send(());
        }
    }
}

impl Drop for DeviceConsumer {
    fn drop(&mut self) {
        if let Some(current) = self.current.take() {
            self.retired.push(current).unwrap_or_else(|_| {
                panic!("audio retirement capacity invariant violated")
            });
        }
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
            if let Some(completed) = self.current.take() {
                self.retired.push(completed).unwrap_or_else(|_| {
                    panic!("audio retirement capacity invariant violated")
                });
            }
            match self.receiver.try_recv() {
                Ok(block) => {
                    self.current = Some(block);
                    self.cursor = 0;
                }
                Err(_) => {
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
