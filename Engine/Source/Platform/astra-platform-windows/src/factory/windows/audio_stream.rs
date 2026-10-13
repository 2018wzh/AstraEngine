use super::*;

pub(super) enum AudioStream {
    Device(cpal::Stream),
    TestNull(astra_platform_common::NullAudioDevice),
}

pub(super) struct AudioResource {
    pub(super) stream: AudioStream,
    #[cfg(feature = "platform-test-driver")]
    pub(super) stream_error: Arc<AtomicBool>,
    pub(super) paused: bool,
}

impl AudioResource {
    pub(super) fn new(
        request: AudioOutputRequest,
        audio_wake: AudioWakeRegistration,
        test_null_audio: bool,
    ) -> Result<
        (
            Self,
            astra_platform_common::NativeAudioProducer,
            AudioDeviceFormat,
        ),
        PlatformError,
    > {
        if request.sample_rate == 0
            || request.channels == 0
            || request.max_buffered_frames == 0
            || request.chunk_frames == 0
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.open",
                "audio output format and queue capacity must be non-zero",
            ));
        }
        if test_null_audio {
            let format = AudioDeviceFormat {
                sample_rate: request.sample_rate,
                channels: request.channels,
            };
            let paused = request.start_paused;
            let (stream, producer) =
                astra_platform_common::NullAudioDevice::open(request, audio_wake)?;
            return Ok((
                Self {
                    stream: AudioStream::TestNull(stream),
                    #[cfg(feature = "platform-test-driver")]
                    stream_error: Arc::new(AtomicBool::new(false)),
                    paused,
                },
                producer,
                format,
            ));
        }
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            host_error("audio.open", "WASAPI default output device is unavailable")
        })?;
        let requested_rate = request.sample_rate;
        let supported = device
            .supported_output_configs()
            .map_err(|_| host_error("audio.open", "WASAPI output format enumeration failed"))?
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
                    "WASAPI has no exact supported format for the requested rate and channels",
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
                    "WASAPI sample format is unsupported",
                ));
            }
        }
        .map_err(|_| host_error("audio.open", "WASAPI output stream creation failed"))?;
        if !request.start_paused {
            stream
                .play()
                .map_err(|_| host_error("audio.open", "WASAPI output stream could not start"))?;
        }
        Ok((
            Self {
                stream: AudioStream::Device(stream),
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
                "WASAPI output is already paused",
            ));
        }
        match &self.stream {
            AudioStream::Device(stream) => stream
                .pause()
                .map_err(|_| host_error("audio.pause", "WASAPI output could not pause"))?,
            AudioStream::TestNull(stream) => stream.pause()?,
        }
        self.paused = true;
        Ok(())
    }

    pub(super) fn resume(&mut self) -> Result<(), PlatformError> {
        if !self.paused {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.resume",
                "WASAPI output is not paused",
            ));
        }
        match &self.stream {
            AudioStream::Device(stream) => stream
                .play()
                .map_err(|_| host_error("audio.resume", "WASAPI output could not resume"))?,
            AudioStream::TestNull(stream) => stream.resume()?,
        }
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
    pub(super) requests: Option<std_mpsc::SyncSender<DecodeWork>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) worker: Option<thread::JoinHandle<()>>,
}

pub(super) struct DecodeWork {
    pub(super) request: PlatformDecodeRequest,
    pub(super) reply: oneshot::Sender<Result<DecodeOutput, PlatformError>>,
}

pub(super) struct DecodeWorkerState {
    pub(super) kind: DecodeKind,
    pub(super) provider: astra_media::WindowsMediaFoundationDecodeProvider,
    pub(super) next_sequence: u64,
    pub(super) video_stream: Option<WindowsVideoStreamState>,
    pub(super) audio_stream: Option<astra_media::WindowsAudioStreamDecoder>,
}

pub(super) struct WindowsVideoStreamState {
    pub(super) max_frames: u64,
    pub(super) max_decoded_byte_count: u64,
    pub(super) decoder: astra_media::WindowsVideoStreamDecoder,
    pub(super) pending: Option<astra_media::DecodedVideoFrame>,
    pub(super) frame_count: u64,
    pub(super) decoded_byte_count: u64,
    pub(super) end_emitted: bool,
}

impl DecodeResource {
    pub(super) fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
        let (request_tx, request_rx) = std_mpsc::sync_channel::<DecodeWork>(2);
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("astra-windows-decode".to_owned())
            .spawn(move || {
                let mut state = match DecodeWorkerState::new(kind) {
                    Ok(state) => {
                        let _ = ready_tx.send(Ok(()));
                        state
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                while !worker_stop.load(Ordering::Acquire) {
                    let Ok(work) = request_rx.recv() else {
                        return;
                    };
                    let result = state.decode(work.request);
                    let _ = work.reply.send(result);
                }
            })
            .map_err(|_| host_error("decode.open", "decode worker could not start"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                requests: Some(request_tx),
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                drop(request_tx);
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                drop(request_tx);
                let _ = worker.join();
                Err(host_error(
                    "decode.open",
                    "decode worker stopped before initialization",
                ))
            }
        }
    }

    pub(super) fn submit(
        &self,
        request: PlatformDecodeRequest,
        reply: oneshot::Sender<Result<DecodeOutput, PlatformError>>,
    ) {
        let Some(requests) = self.requests.as_ref() else {
            let _ = reply.send(Err(host_error("decode.submit", "decode worker is closed")));
            return;
        };
        match requests.try_send(DecodeWork { request, reply }) {
            Ok(()) => {}
            Err(std_mpsc::TrySendError::Full(work)) => {
                let _ = work.reply.send(Err(host_error(
                    "decode.submit",
                    "decode worker queue is full",
                )));
            }
            Err(std_mpsc::TrySendError::Disconnected(work)) => {
                let _ = work
                    .reply
                    .send(Err(host_error("decode.submit", "decode worker is closed")));
            }
        }
    }
}

impl Drop for DecodeResource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Close the sender before joining so a worker waiting for the next
        // request observes channel closure immediately.
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                tracing::error!(
                    event = "platform.windows.decode.worker_panic",
                    diagnostic_code = "ASTRA_PLATFORM_DECODE_WORKER_PANIC",
                    "Windows decode worker panicked during shutdown"
                );
            }
        }
    }
}
