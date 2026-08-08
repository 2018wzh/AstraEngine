#![cfg(target_os = "android")]

use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use astra_platform::{
    AudioDeviceFormat, AudioFocusState, AudioOutputRequest, AudioWakeRegistration, PlatformError,
    PlatformErrorCode,
};
use astra_platform_common::{NativeAudioConsumer, NativeAudioProducer, NativeAudioQueue};
use oboe::{
    AudioApi, AudioOutputCallback, AudioOutputStream, AudioStream, AudioStreamAsync,
    AudioStreamBase, AudioStreamBuilder, AudioStreamSafe, ContentType, DataCallbackResult, Mono,
    Output, PerformanceMode, SharingMode, Stereo, Usage,
};

pub(crate) struct AndroidAudioResource {
    stream: AndroidAudioStream,
    disconnected: Arc<AtomicBool>,
    gain_bits: Arc<AtomicU32>,
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
    disconnected: Arc<AtomicBool>,
    gain_bits: Arc<AtomicU32>,
    audio_wake: AudioWakeRegistration,
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
        let filled = self.0.consumer.pop_samples(output);
        let gain = f32::from_bits(self.0.gain_bits.load(Ordering::Relaxed));
        for sample in &mut output[..filled] {
            *sample *= gain;
        }
        output[filled..].fill(0.0);
        if filled != output.len() {
            self.0.consumer.record_underflow();
        }
        self.0.audio_wake.notify();
        DataCallbackResult::Continue
    }

    fn on_error_after_close(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        _error: oboe::Error,
    ) {
        self.0.disconnected.store(true, Ordering::Release);
        self.0.audio_wake.notify();
    }
}

impl AudioOutputCallback for StereoCallback {
    type FrameType = (f32, Stereo);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        output: &mut [(f32, f32)],
    ) -> DataCallbackResult {
        let gain = f32::from_bits(self.0.gain_bits.load(Ordering::Relaxed));
        let mut scratch = [0.0_f32; 2048];
        let mut written_frames = 0;
        while written_frames < output.len() {
            let requested_frames = (scratch.len() / 2).min(output.len() - written_frames);
            let requested_samples = requested_frames * 2;
            let filled = self
                .0
                .consumer
                .pop_samples(&mut scratch[..requested_samples]);
            let complete_frames = filled / 2;
            for (target, frame) in output[written_frames..written_frames + complete_frames]
                .iter_mut()
                .zip(scratch[..complete_frames * 2].chunks_exact(2))
            {
                target.0 = frame[0] * gain;
                target.1 = frame[1] * gain;
            }
            written_frames += complete_frames;
            if filled != requested_samples {
                break;
            }
        }
        output[written_frames..].fill((0.0, 0.0));
        if written_frames != output.len() {
            self.0.consumer.record_underflow();
        }
        self.0.audio_wake.notify();
        DataCallbackResult::Continue
    }

    fn on_error_after_close(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        _error: oboe::Error,
    ) {
        self.0.disconnected.store(true, Ordering::Release);
        self.0.audio_wake.notify();
    }
}

impl AndroidAudioResource {
    pub(crate) fn new(
        request: AudioOutputRequest,
        audio_wake: AudioWakeRegistration,
    ) -> Result<(Self, NativeAudioProducer, AudioDeviceFormat), PlatformError> {
        if request.sample_rate == 0
            || !matches!(request.channels, 1 | 2)
            || request.chunk_frames == 0
            || request.max_buffered_frames == 0
        {
            return Err(audio_error(
                "audio.open",
                "AAudio requires a non-zero rate, mono/stereo channels, and bounded queue",
            ));
        }
        let chunk_samples = request
            .chunk_frames
            .checked_mul(usize::from(request.channels))
            .ok_or_else(|| audio_error("audio.open", "audio queue capacity overflows"))?;
        let chunk_capacity = request.max_buffered_frames.div_ceil(request.chunk_frames);
        let (producer, consumer, _telemetry) =
            NativeAudioQueue::create(chunk_capacity, chunk_samples, audio_wake.clone())?;
        let disconnected = Arc::new(AtomicBool::new(false));
        let gain_bits = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let callback_state = CallbackState {
            consumer,
            disconnected: Arc::clone(&disconnected),
            gain_bits: Arc::clone(&gain_bits),
            audio_wake: audio_wake.clone(),
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
            disconnected,
            gain_bits,
            paused: false,
        };
        resource.stream.request_start()?;
        Ok((
            resource,
            producer,
            AudioDeviceFormat {
                sample_rate: request.sample_rate,
                channels: request.channels,
            },
        ))
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

fn audio_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}
