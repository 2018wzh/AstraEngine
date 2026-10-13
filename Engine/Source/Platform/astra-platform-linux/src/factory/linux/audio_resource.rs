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
        let host = cpal::host_from_id(cpal::HostId::Alsa)
            .map_err(|_| host_error("audio.open", "ALSA host is unavailable"))?;
        let device = host
            .default_output_device()
            .ok_or_else(|| host_error("audio.open", "ALSA default output device is unavailable"))?;
        let requested_rate = request.sample_rate;
        let supported = device
            .supported_output_configs()
            .map_err(|_| host_error("audio.open", "ALSA output format enumeration failed"))?
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
                    "ALSA has no exact supported format for the requested rate and channels",
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
                    "ALSA sample format is unsupported",
                ));
            }
        }
        .map_err(|_| host_error("audio.open", "ALSA output stream creation failed"))?;
        if !request.start_paused {
            stream
                .play()
                .map_err(|_| host_error("audio.open", "ALSA output stream could not start"))?;
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
                "ALSA output is already paused",
            ));
        }
        self.stream
            .pause()
            .map_err(|_| host_error("audio.pause", "ALSA output could not pause"))?;
        self.paused = true;
        Ok(())
    }

    pub(super) fn resume(&mut self) -> Result<(), PlatformError> {
        if !self.paused {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.resume",
                "ALSA output is not paused",
            ));
        }
        self.stream
            .play()
            .map_err(|_| host_error("audio.resume", "ALSA output could not resume"))?;
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

pub(super) fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}

pub(super) fn default_roots(package_id: &str) -> Option<super::super::HostRoots> {
    let project = directories::ProjectDirs::from("com", "AstraEngine", package_id)?;
    Some(super::super::HostRoots {
        save_base: project.data_dir().join("SavedGames"),
        bundle_root: std::env::current_exe().ok()?.parent()?.to_path_buf(),
    })
}
