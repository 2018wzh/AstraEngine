#![cfg(target_os = "android")]

use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};

use astra_platform::{
    AudioDeviceFormat, AudioFocusState, AudioMeter, AudioOutputRequest, AudioOutputState,
    AudioOutputStatus, AudioPacket, PlatformError, PlatformErrorCode,
};
use astra_platform_common::{
    AudioQueueTelemetryReader, NativeAudioConsumer, NativeAudioProducer, NativeAudioQueue,
};
use oboe::{
    AudioApi, AudioOutputCallback, AudioOutputStream, AudioStream, AudioStreamAsync,
    AudioStreamBase, AudioStreamBuilder, AudioStreamSafe, ContentType, DataCallbackResult, Mono,
    Output, PerformanceMode, SharingMode, Stereo, Usage,
};

pub(crate) struct AndroidAudioResource {
    stream: AndroidAudioStream,
    producer: NativeAudioProducer,
    telemetry: AudioQueueTelemetryReader,
    meter: Arc<CallbackMeter>,
    disconnected: Arc<AtomicBool>,
    gain_bits: Arc<AtomicU32>,
    channels: u16,
    max_buffered_frames: usize,
    next_sequence: u64,
    submitted_samples: u64,
    paused: bool,
}

enum AndroidAudioStream {
    Mono(AudioStreamAsync<Output, MonoCallback>),
    Stereo(AudioStreamAsync<Output, StereoCallback>),
}

impl AndroidAudioStream {
    fn request_start(&mut self) -> Result<(), PlatformError> {
        let status = match self {
            Self::Mono(stream) => stream.request_start(),
            Self::Stereo(stream) => stream.request_start(),
        };
        status.map_err(|_| audio_error("audio.resume", "AAudio stream could not start"))
    }

    fn request_pause(&mut self) -> Result<(), PlatformError> {
        let status = match self {
            Self::Mono(stream) => stream.request_pause(),
            Self::Stereo(stream) => stream.request_pause(),
        };
        status.map_err(|_| audio_error("audio.pause", "AAudio stream could not pause"))
    }

    fn request_stop(&mut self) -> Result<(), PlatformError> {
        let status = match self {
            Self::Mono(stream) => stream.request_stop(),
            Self::Stereo(stream) => stream.request_stop(),
        };
        status.map_err(|_| audio_error("audio.close", "AAudio stream could not stop"))
    }

    fn actual_api(&self) -> AudioApi {
        match self {
            Self::Mono(stream) => stream.get_audio_api(),
            Self::Stereo(stream) => stream.get_audio_api(),
        }
    }
}

struct CallbackState {
    consumer: NativeAudioConsumer,
    meter: Arc<CallbackMeter>,
    disconnected: Arc<AtomicBool>,
    gain_bits: Arc<AtomicU32>,
}

struct MonoCallback(CallbackState);
struct StereoCallback(CallbackState);

impl AudioOutputCallback for MonoCallback {
    type FrameType = (f32, Mono);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        output: &mut [f32],
    ) -> DataCallbackResult {
        self.0.meter.begin_callback();
        let mut underflow = false;
        for sample in output {
            let value = self.0.consumer.pop_sample().unwrap_or_else(|| {
                underflow = true;
                0.0
            }) * f32::from_bits(self.0.gain_bits.load(Ordering::Relaxed));
            *sample = value;
            self.0.meter.record(value);
        }
        if underflow {
            self.0.consumer.record_underflow();
        }
        DataCallbackResult::Continue
    }

    fn on_error_after_close(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        _error: oboe::Error,
    ) {
        self.0.disconnected.store(true, Ordering::Release);
    }
}

impl AudioOutputCallback for StereoCallback {
    type FrameType = (f32, Stereo);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        output: &mut [(f32, f32)],
    ) -> DataCallbackResult {
        self.0.meter.begin_callback();
        let mut underflow = false;
        for (left, right) in output {
            let first = self.0.consumer.pop_sample().unwrap_or_else(|| {
                underflow = true;
                0.0
            }) * f32::from_bits(self.0.gain_bits.load(Ordering::Relaxed));
            let second = self.0.consumer.pop_sample().unwrap_or_else(|| {
                underflow = true;
                0.0
            }) * f32::from_bits(self.0.gain_bits.load(Ordering::Relaxed));
            *left = first;
            *right = second;
            self.0.meter.record(first);
            self.0.meter.record(second);
        }
        if underflow {
            self.0.consumer.record_underflow();
        }
        DataCallbackResult::Continue
    }

    fn on_error_after_close(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        _error: oboe::Error,
    ) {
        self.0.disconnected.store(true, Ordering::Release);
    }
}

impl AndroidAudioResource {
    pub(crate) fn new(request: AudioOutputRequest) -> Result<Self, PlatformError> {
        if request.sample_rate == 0
            || !matches!(request.channels, 1 | 2)
            || request.max_buffered_frames == 0
        {
            return Err(audio_error(
                "audio.open",
                "AAudio requires a non-zero rate, mono/stereo channels, and bounded queue",
            ));
        }
        let capacity = request
            .max_buffered_frames
            .checked_mul(usize::from(request.channels))
            .ok_or_else(|| audio_error("audio.open", "audio queue capacity overflows"))?;
        let (producer, consumer, telemetry) = NativeAudioQueue::create(capacity)?;
        let meter = Arc::new(CallbackMeter::default());
        let disconnected = Arc::new(AtomicBool::new(false));
        let gain_bits = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let callback_state = CallbackState {
            consumer,
            meter: Arc::clone(&meter),
            disconnected: Arc::clone(&disconnected),
            gain_bits: Arc::clone(&gain_bits),
        };
        let rate = i32::try_from(request.sample_rate)
            .map_err(|_| audio_error("audio.open", "sample rate exceeds AAudio range"))?;
        let stream = match request.channels {
            1 => AndroidAudioStream::Mono(
                AudioStreamBuilder::default()
                    .set_output()
                    .set_mono()
                    .set_f32()
                    .set_sample_rate(rate)
                    .set_audio_api(AudioApi::AAudio)
                    .set_sharing_mode(SharingMode::Shared)
                    .set_performance_mode(PerformanceMode::LowLatency)
                    .set_usage(Usage::Game)
                    .set_content_type(ContentType::Music)
                    .set_callback(MonoCallback(callback_state))
                    .open_stream()
                    .map_err(|_| audio_error("audio.open", "AAudio mono stream creation failed"))?,
            ),
            2 => AndroidAudioStream::Stereo(
                AudioStreamBuilder::default()
                    .set_output()
                    .set_stereo()
                    .set_f32()
                    .set_sample_rate(rate)
                    .set_audio_api(AudioApi::AAudio)
                    .set_sharing_mode(SharingMode::Shared)
                    .set_performance_mode(PerformanceMode::LowLatency)
                    .set_usage(Usage::Game)
                    .set_content_type(ContentType::Music)
                    .set_callback(StereoCallback(callback_state))
                    .open_stream()
                    .map_err(|_| {
                        audio_error("audio.open", "AAudio stereo stream creation failed")
                    })?,
            ),
            _ => unreachable!("validated above"),
        };
        if stream.actual_api() != AudioApi::AAudio {
            return Err(PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "audio.open",
                "Oboe selected a non-AAudio backend for the release profile",
            ));
        }
        let mut resource = Self {
            stream,
            producer,
            telemetry,
            meter,
            disconnected,
            gain_bits,
            channels: request.channels,
            max_buffered_frames: request.max_buffered_frames,
            next_sequence: 1,
            submitted_samples: 0,
            paused: false,
        };
        resource.stream.request_start()?;
        Ok(resource)
    }

    pub(crate) fn submit(&mut self, packet: AudioPacket) -> Result<(), PlatformError> {
        self.ensure_connected("audio.submit")?;
        if self.paused || packet.sequence != self.next_sequence || packet.channels != self.channels
        {
            return Err(audio_error(
                "audio.submit",
                "audio packet sequence, channels, or lifecycle is invalid",
            ));
        }
        if packet.samples.is_empty()
            || !packet
                .samples
                .len()
                .is_multiple_of(usize::from(self.channels))
        {
            return Err(audio_error(
                "audio.submit",
                "audio packet is empty or not frame aligned",
            ));
        }
        self.producer.push_samples(&packet.samples)?;
        self.submitted_samples = self
            .submitted_samples
            .checked_add(packet.samples.len() as u64)
            .ok_or_else(|| audio_error("audio.submit", "submitted sample counter overflows"))?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| audio_error("audio.submit", "audio sequence counter overflows"))?;
        Ok(())
    }

    pub(crate) fn state(&self) -> Result<AudioOutputState, PlatformError> {
        self.ensure_connected("audio.query")?;
        let telemetry = self.telemetry.snapshot();
        let queued_samples = self
            .submitted_samples
            .saturating_sub(telemetry.sample_count);
        Ok(AudioOutputState {
            queued_frames: usize::try_from(queued_samples / u64::from(self.channels))
                .unwrap_or(usize::MAX)
                .min(self.max_buffered_frames),
            callback_count: self.meter.callback_count.load(Ordering::Acquire),
            submitted_samples: self.submitted_samples,
            consumed_samples: telemetry.sample_count,
            underflow_count: telemetry.underflow_count,
            meter: self.meter.snapshot(),
        })
    }

    pub(crate) fn status(&self) -> Result<AudioOutputStatus, PlatformError> {
        let state = self.state()?;
        Ok(AudioOutputStatus {
            submitted_frames: state.submitted_samples / u64::from(self.channels),
            played_frames: state.consumed_samples / u64::from(self.channels),
            buffered_frames: state.queued_frames as u64,
            underflow_count: state.underflow_count,
            meter: state.meter,
        })
    }

    pub(crate) fn drain(&self) -> Result<AudioMeter, PlatformError> {
        let state = self.state()?;
        if state.consumed_samples < state.submitted_samples {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.drain",
                "AAudio queue has not drained yet",
            ));
        }
        Ok(state.meter)
    }

    pub(crate) fn pause(&mut self) -> Result<(), PlatformError> {
        self.ensure_connected("audio.pause")?;
        if !self.paused {
            self.stream.request_pause()?;
            self.paused = true;
        }
        Ok(())
    }

    pub(crate) fn resume(&mut self) -> Result<(), PlatformError> {
        self.ensure_connected("audio.resume")?;
        if self.paused {
            self.stream.request_start()?;
            self.paused = false;
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) -> Result<(), PlatformError> {
        self.stream.request_stop()
    }

    pub(crate) fn apply_focus(&mut self, focus: AudioFocusState) -> Result<(), PlatformError> {
        match focus {
            AudioFocusState::Gained => {
                self.gain_bits.store(1.0_f32.to_bits(), Ordering::Release);
                self.resume()
            }
            AudioFocusState::Duck => {
                self.gain_bits.store(0.2_f32.to_bits(), Ordering::Release);
                Ok(())
            }
            AudioFocusState::Lost | AudioFocusState::LostTransient => {
                self.gain_bits.store(0.0_f32.to_bits(), Ordering::Release);
                self.pause()
            }
        }
    }

    fn ensure_connected(&self, operation: &'static str) -> Result<(), PlatformError> {
        if self.disconnected.load(Ordering::Acquire) {
            return Err(PlatformError::new(
                PlatformErrorCode::DeviceLost,
                operation,
                "AAudio device was disconnected",
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct CallbackMeter {
    callback_count: AtomicU64,
    sample_count: AtomicU64,
    peak_bits: AtomicU32,
    sum_squares_bits: AtomicU64,
}

impl CallbackMeter {
    fn begin_callback(&self) {
        self.callback_count.fetch_add(1, Ordering::Release);
    }

    fn record(&self, sample: f32) {
        let magnitude_bits = sample.abs().to_bits();
        let mut peak = self.peak_bits.load(Ordering::Relaxed);
        while magnitude_bits > peak {
            match self.peak_bits.compare_exchange_weak(
                peak,
                magnitude_bits,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
        let contribution = f64::from(sample) * f64::from(sample);
        let mut sum = self.sum_squares_bits.load(Ordering::Relaxed);
        loop {
            let next = f64::from_bits(sum) + contribution;
            match self.sum_squares_bits.compare_exchange_weak(
                sum,
                next.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => sum = actual,
            }
        }
        self.sample_count.fetch_add(1, Ordering::Release);
    }

    fn snapshot(&self) -> AudioMeter {
        let samples = self.sample_count.load(Ordering::Acquire);
        let rms = if samples == 0 {
            0.0
        } else {
            (f64::from_bits(self.sum_squares_bits.load(Ordering::Acquire)) / samples as f64).sqrt()
                as f32
        };
        AudioMeter {
            sample_count: samples,
            peak_dbfs: amplitude_dbfs(f32::from_bits(self.peak_bits.load(Ordering::Acquire))),
            rms_dbfs: amplitude_dbfs(rms),
        }
    }
}

fn amplitude_dbfs(value: f32) -> f32 {
    if value <= 0.0 {
        -120.0
    } else {
        20.0 * value.log10()
    }
}

fn audio_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

pub(crate) fn preferred_output_format() -> Result<AudioDeviceFormat, PlatformError> {
    let (_producer, consumer, _telemetry) = NativeAudioQueue::create(2)?;
    let callback = StereoCallback(CallbackState {
        consumer,
        meter: Arc::new(CallbackMeter::default()),
        disconnected: Arc::new(AtomicBool::new(false)),
        gain_bits: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
    });
    let stream = AudioStreamBuilder::default()
        .set_output()
        .set_stereo()
        .set_f32()
        .set_audio_api(AudioApi::AAudio)
        .set_sharing_mode(SharingMode::Shared)
        .set_performance_mode(PerformanceMode::LowLatency)
        .set_usage(Usage::Game)
        .set_content_type(ContentType::Music)
        .set_callback(callback)
        .open_stream()
        .map_err(|_| audio_error("audio.format", "AAudio format probe stream could not open"))?;
    if stream.get_audio_api() != AudioApi::AAudio {
        return Err(PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "audio.format",
            "Oboe format probe selected a non-AAudio backend",
        ));
    }
    let sample_rate = u32::try_from(stream.get_sample_rate())
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| audio_error("audio.format", "AAudio reported an invalid sample rate"))?;
    let channels = u16::try_from(stream.get_channel_count() as i32)
        .ok()
        .filter(|value| matches!(*value, 1 | 2))
        .ok_or_else(|| audio_error("audio.format", "AAudio reported an invalid channel count"))?;
    Ok(AudioDeviceFormat {
        sample_rate,
        channels,
    })
}
