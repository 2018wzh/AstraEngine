use super::*;

impl AudioResource {
    pub(super) fn new(
        request: AudioOutputRequest,
        audio_wake: AudioWakeRegistration,
    ) -> Result<
        (
            Self,
            astra_platform_common::NativeAudioProducer,
            AudioDeviceFormat,
        ),
        PlatformError,
    > {
        if request.sample_rate == 0 || request.channels == 0 || request.max_buffered_frames == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.open",
                "audio output format and queue capacity must be non-zero",
            ));
        }
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            host_error(
                "audio.open",
                "CoreAudio default output device is unavailable",
            )
        })?;
        let requested_rate = request.sample_rate;
        let supported = device
            .supported_output_configs()
            .map_err(|_| host_error("audio.open", "CoreAudio output format enumeration failed"))?
            .filter(|range| {
                range.channels() == request.channels
                    && range.min_sample_rate() <= requested_rate
                    && range.max_sample_rate() >= requested_rate
                    && sample_format_rank(range.sample_format()).is_some()
            })
            .map(|range| range.with_sample_rate(requested_rate))
            .min_by_key(|config| sample_format_rank(config.sample_format()))
            .ok_or_else(|| {
                host_error(
                    "audio.open",
                    "CoreAudio has no exact supported format for the requested rate and channels",
                )
            })?;
        let config: cpal::StreamConfig = supported.clone().into();
        let chunk_samples = request
            .chunk_frames
            .checked_mul(usize::from(request.channels))
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.open",
                    "audio output queue capacity overflows",
                )
            })?;
        let chunk_capacity = request.max_buffered_frames.div_ceil(request.chunk_frames);
        let (producer, consumer, _queue_telemetry) =
            astra_platform_common::NativeAudioQueue::create(
                chunk_capacity,
                chunk_samples,
                audio_wake.clone(),
            )?;
        let stream_error = Arc::new(AtomicBool::new(false));
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                let error = Arc::clone(&stream_error);
                let wake = audio_wake.clone();
                let error_wake = audio_wake.clone();
                let mut consumer = consumer;
                device.build_output_stream(
                    &config,
                    move |output: &mut [f32], _| {
                        let _ = consumer.fill_output_f32(output);
                        wake.notify();
                    },
                    move |stream_error_value| {
                        set_stream_error(&error, stream_error_value);
                        error_wake.notify();
                    },
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let error = Arc::clone(&stream_error);
                let wake = audio_wake.clone();
                let error_wake = audio_wake.clone();
                let mut consumer = consumer;
                device.build_output_stream(
                    &config,
                    move |output: &mut [i16], _| {
                        let _ = consumer.fill_output_i16(output);
                        wake.notify();
                    },
                    move |stream_error_value| {
                        set_stream_error(&error, stream_error_value);
                        error_wake.notify();
                    },
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let error = Arc::clone(&stream_error);
                let wake = audio_wake.clone();
                let error_wake = audio_wake.clone();
                let mut consumer = consumer;
                device.build_output_stream(
                    &config,
                    move |output: &mut [u16], _| {
                        let _ = consumer.fill_output_u16(output);
                        wake.notify();
                    },
                    move |stream_error_value| {
                        set_stream_error(&error, stream_error_value);
                        error_wake.notify();
                    },
                    None,
                )
            }
            _ => {
                return Err(host_error(
                    "audio.open",
                    "CoreAudio sample format is unsupported",
                ));
            }
        }
        .map_err(|_| host_error("audio.open", "CoreAudio output stream creation failed"))?;
        if !request.start_paused {
            stream
                .play()
                .map_err(|_| host_error("audio.open", "CoreAudio output stream could not start"))?;
        }
        Ok((
            Self {
                stream,
                #[cfg(feature = "platform-test-driver")]
                stream_error,
                paused: request.start_paused,
            },
            producer,
            AudioDeviceFormat {
                sample_rate: request.sample_rate,
                channels: request.channels,
            },
        ))
    }

    pub(super) fn pause(&mut self) -> Result<(), PlatformError> {
        if self.paused {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.pause",
                "CoreAudio output is already paused",
            ));
        }
        self.stream
            .pause()
            .map_err(|_| host_error("audio.pause", "CoreAudio output could not pause"))?;
        self.paused = true;
        Ok(())
    }

    pub(super) fn resume(&mut self) -> Result<(), PlatformError> {
        if !self.paused {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.resume",
                "CoreAudio output is not paused",
            ));
        }
        self.stream
            .play()
            .map_err(|_| host_error("audio.resume", "CoreAudio output could not resume"))?;
        self.paused = false;
        Ok(())
    }

    #[cfg(feature = "platform-test-driver")]
    pub(super) fn inject_device_loss(&mut self) {
        self.stream_error.store(true, Ordering::Release);
    }
}

pub(super) fn sample_format_rank(format: cpal::SampleFormat) -> Option<u8> {
    match format {
        cpal::SampleFormat::F32 => Some(0),
        cpal::SampleFormat::I16 => Some(1),
        cpal::SampleFormat::U16 => Some(2),
        _ => None,
    }
}

pub(super) fn set_stream_error(error: &AtomicBool, _value: cpal::StreamError) {
    error.store(true, Ordering::Release);
}

pub(super) struct DecodeResource {
    pub(super) kind: DecodeKind,
    pub(super) next_sequence: u64,
}

impl DecodeResource {
    pub(super) fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
        Ok(Self {
            kind,
            next_sequence: 1,
        })
    }

    pub(super) fn decode(
        &mut self,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        if request.sequence != self.next_sequence
            || request.kind != self.kind
            || request.bytes.is_empty()
            || !supported_codec(request.kind, &request.codec)
            || !request.description.is_empty()
            || request.sample_rate.is_some()
            || request.channels.is_some()
            || request.coded_width.is_some()
            || request.coded_height.is_some()
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence, kind, codec, payload, or metadata is invalid",
            ));
        }
        let next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence overflowed",
            )
        })?;
        let output = match request.kind {
            DecodeKind::Image => decode_image(request)?,
            DecodeKind::Audio | DecodeKind::Video => decode_avfoundation(request)?,
        };
        self.next_sequence = next_sequence;
        Ok(output)
    }
}

pub(super) fn supported_codec(kind: DecodeKind, codec: &str) -> bool {
    match kind {
        DecodeKind::Image => matches!(codec, "png" | "jpeg" | "jpg" | "webp"),
        DecodeKind::Audio => matches!(codec, "mp3" | "aac" | "opus" | "mp4" | "webm"),
        DecodeKind::Video => matches!(codec, "h264" | "vp9" | "mp4" | "webm"),
    }
}

pub(super) fn decode_image(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
    let result = astra_media::ImageDecodeProvider
        .decode(&astra_media::DecodeRequest {
            kind: astra_media::DecodeKind::Image,
            codec: request.codec,
            bytes: request.bytes,
            profile: "macos-release".to_string(),
        })
        .map_err(media_decode_error)?;
    match result.output {
        MediaDecodeOutput::CpuBuffer { bytes, format } => {
            Ok(DecodeOutput::CpuBuffer { format, bytes })
        }
        MediaDecodeOutput::AudioPcmI16 { .. } | MediaDecodeOutput::AudioPcmF32 { .. } => Err(
            host_error("decode.submit", "image decoder returned audio PCM"),
        ),
        MediaDecodeOutput::MediaSurfaceToken(_) => Err(host_error(
            "decode.submit",
            "image decoder returned an unsupported external media surface",
        )),
    }
}
