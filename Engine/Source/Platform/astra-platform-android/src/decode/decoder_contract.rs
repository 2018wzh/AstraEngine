use super::*;

pub(super) const CALLBACK_QUEUE_CAPACITY: usize = 64;

pub(super) const CALLBACK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) const DECODE_TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub(super) const MAX_VIDEO_FRAMES: u64 = 10_000;

pub(crate) fn prepare_scratch_root(root: &Path) -> Result<(), PlatformError> {
    std::fs::create_dir_all(root).map_err(|_| {
        decode_error(
            PlatformErrorCode::Io,
            "decode.scratch.prepare",
            "app-private MediaCodec scratch directory could not be created",
        )
    })?;
    for entry in std::fs::read_dir(root).map_err(|_| {
        decode_error(
            PlatformErrorCode::Io,
            "decode.scratch.prepare",
            "app-private MediaCodec scratch directory could not be inspected",
        )
    })? {
        let entry = entry.map_err(|_| media_codec_error("decode.scratch.inspect"))?;
        let file_type = entry
            .file_type()
            .map_err(|_| media_codec_error("decode.scratch.inspect"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_file() && name.starts_with("decode-") && name.ends_with(".media") {
            std::fs::remove_file(entry.path())
                .map_err(|_| media_codec_error("decode.scratch.recover"))?;
        }
    }
    Ok(())
}

pub(super) enum DecodeWork {
    Decode(
        PlatformDecodeRequest,
        tokio::sync::oneshot::Sender<Result<DecodeOutput, PlatformError>>,
    ),
    Close(tokio::sync::oneshot::Sender<Result<(), PlatformError>>),
}

pub(crate) struct AndroidDecodeWorker {
    pub(super) tx: mpsc::SyncSender<DecodeWork>,
    pub(super) join: Option<std::thread::JoinHandle<()>>,
}

impl AndroidDecodeWorker {
    pub(crate) fn new(
        kind: DecodeKind,
        temp_root: PathBuf,
        max_output_bytes: usize,
    ) -> Result<Self, PlatformError> {
        let resource =
            WorkerDecodeResource(DecodeResource::new(kind, temp_root, max_output_bytes)?);
        let (tx, rx) = mpsc::sync_channel(1);
        let join = std::thread::Builder::new()
            .name("astra-mediacodec".to_string())
            .spawn(move || {
                decode_worker_loop(resource, rx);
            })
            .map_err(|_| media_codec_error("decode.worker.start"))?;
        Ok(Self {
            tx,
            join: Some(join),
        })
    }

    pub(crate) fn submit(
        &self,
        request: PlatformDecodeRequest,
        reply: tokio::sync::oneshot::Sender<Result<DecodeOutput, PlatformError>>,
    ) {
        if let Err(error) = self.tx.try_send(DecodeWork::Decode(request, reply)) {
            let (code, work) = match error {
                mpsc::TrySendError::Full(work) => (PlatformErrorCode::QueueOverflow, work),
                mpsc::TrySendError::Disconnected(work) => (PlatformErrorCode::InvalidState, work),
            };
            if let DecodeWork::Decode(_, reply) = work {
                let _ = reply.send(Err(decode_error(
                    code,
                    "decode.submit",
                    "MediaCodec worker queue is unavailable",
                )));
            }
        }
    }

    pub(crate) fn close(mut self, reply: tokio::sync::oneshot::Sender<Result<(), PlatformError>>) {
        match self.tx.send(DecodeWork::Close(reply)) {
            Ok(()) => {
                if let Some(join) = self.join.take() {
                    let _ = join.join();
                }
            }
            Err(error) => {
                if let DecodeWork::Close(reply) = error.0 {
                    let _ = reply.send(Err(media_codec_error("decode.close")));
                }
            }
        }
    }
}

impl Drop for AndroidDecodeWorker {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let (reply, _result) = tokio::sync::oneshot::channel();
            let _ = self.tx.send(DecodeWork::Close(reply));
            let _ = join.join();
        }
    }
}

pub(crate) struct DecodeResource {
    pub(super) kind: DecodeKind,
    pub(super) next_sequence: u64,
    pub(super) temp_root: PathBuf,
    pub(super) max_output_bytes: usize,
    pub(super) video_stream: Option<AndroidVideoStream>,
}

// SAFETY: the NDK MediaCodec/MediaExtractor/ImageReader types are !Send because
// the NDK objects are not internally synchronized, not because of thread
// affinity. This wrapper moves them to the dedicated astra-mediacodec worker
// thread before first use, and every access stays on that thread for the
// lifetime of the resource. The wrapper must be passed to `decode_worker_loop`
// as a whole value; destructuring it inside the closure would capture the inner
// NDK objects directly and bypass this impl.
pub(super) struct WorkerDecodeResource(pub(super) DecodeResource);

unsafe impl Send for WorkerDecodeResource {}

pub(super) fn decode_worker_loop(resource: WorkerDecodeResource, rx: mpsc::Receiver<DecodeWork>) {
    let WorkerDecodeResource(mut resource) = resource;
    while let Ok(work) = rx.recv() {
        match work {
            DecodeWork::Decode(request, reply) => {
                let _ = reply.send(resource.decode(request));
            }
            DecodeWork::Close(reply) => {
                let _ = reply.send(Ok(()));
                break;
            }
        }
    }
}

impl DecodeResource {
    pub(crate) fn new(
        kind: DecodeKind,
        temp_root: PathBuf,
        max_output_bytes: usize,
    ) -> Result<Self, PlatformError> {
        std::fs::create_dir_all(&temp_root).map_err(|_| {
            decode_error(
                PlatformErrorCode::Io,
                "decode.open",
                "app-private MediaCodec scratch directory could not be created",
            )
        })?;
        Ok(Self {
            kind,
            next_sequence: 1,
            temp_root,
            max_output_bytes,
            video_stream: None,
        })
    }

    pub(crate) fn decode(
        &mut self,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        if request.sequence != self.next_sequence
            || request.kind != self.kind
            || !request.description.is_empty()
            || request.sample_rate.is_some()
            || request.channels.is_some()
            || request.coded_width.is_some()
            || request.coded_height.is_some()
        {
            return Err(decode_error(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence, kind, payload, or metadata is invalid",
            ));
        }
        let output = match request.stream_action {
            DecodeStreamAction::OneShot => {
                if request.bytes.is_empty() || self.video_stream.is_some() {
                    return Err(media_codec_error("decode.submit.one_shot"));
                }
                match request.kind {
                    DecodeKind::Image => decode_image(request)?,
                    DecodeKind::Audio => {
                        decode_audio_media_codec(&self.temp_root, request, self.max_output_bytes)?
                    }
                    DecodeKind::Video => {
                        return Err(media_codec_error("decode.video.stream_required"));
                    }
                }
            }
            DecodeStreamAction::Start => {
                if request.kind != DecodeKind::Video
                    || request.bytes.is_empty()
                    || self.video_stream.is_some()
                {
                    return Err(media_codec_error("decode.video.stream.start"));
                }
                let stream = AndroidVideoStream::open(
                    &self.temp_root,
                    &request.codec,
                    request.bytes.as_slice(),
                    self.max_output_bytes,
                )?;
                let output = DecodeOutput::VideoStreamStart {
                    duration_estimate_us: Some(stream.duration_us),
                    frame_count: None,
                    decoded_byte_count: None,
                };
                self.video_stream = Some(stream);
                output
            }
            DecodeStreamAction::Next => {
                if request.kind != DecodeKind::Video || !request.bytes.is_empty() {
                    return Err(media_codec_error("decode.video.stream.next"));
                }
                let result = self
                    .video_stream
                    .as_mut()
                    .ok_or_else(|| media_codec_error("decode.video.stream.missing"))?
                    .next_output();
                if result.is_err() {
                    self.video_stream.take();
                }
                result?
            }
        };
        self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            decode_error(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence overflowed",
            )
        })?;
        Ok(output)
    }
}

pub(super) fn decode_image(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
    let provider = astra_media::ImageDecodeProvider;
    if !provider.capability().codecs.contains(&request.codec) {
        return Err(decode_error(
            PlatformErrorCode::ProviderUnavailable,
            "decode.image",
            "image codec is not supported by the verified decoder",
        ));
    }
    let output = provider
        .decode(&astra_media::DecodeRequest {
            kind: astra_media::DecodeKind::Image,
            codec: request.codec,
            bytes: request.bytes,
            profile: "android-release".to_string(),
        })
        .map_err(|_| {
            decode_error(
                PlatformErrorCode::IntegrityMismatch,
                "decode.image",
                "verified image decoding failed",
            )
        })?;
    match output.output {
        astra_media::DecodeOutput::CpuBuffer { bytes, format } => {
            Ok(DecodeOutput::CpuBuffer { format, bytes })
        }
        astra_media::DecodeOutput::AudioPcmI16 { .. }
        | astra_media::DecodeOutput::AudioPcmF32 { .. } => Err(decode_error(
            PlatformErrorCode::InvalidState,
            "decode.image",
            "verified image decoder returned audio PCM",
        )),
        astra_media::DecodeOutput::MediaSurfaceToken(_) => Err(decode_error(
            PlatformErrorCode::InvalidState,
            "decode.image",
            "verified image decoder returned a native token",
        )),
    }
}

pub(super) fn decode_audio_media_codec(
    temp_root: &Path,
    request: PlatformDecodeRequest,
    max_output_bytes: usize,
) -> Result<DecodeOutput, PlatformError> {
    if request.kind != DecodeKind::Audio {
        return Err(media_codec_error("decode.mediacodec.audio_kind"));
    }
    let input = TempInput::create(temp_root, &request.bytes)?;
    let mut extractor = Extractor::new(&input.file)?;
    let (track_index, track_format, mime) = extractor.select_track(request.kind)?;
    validate_codec_name(&request.codec, &mime, request.kind)?;
    let sample_rate = track_format
        .i32("sample-rate")
        .and_then(|value| u32::try_from(value).ok());
    let channels = track_format
        .i32("channel-count")
        .and_then(|value| u16::try_from(value).ok());
    extractor.select(track_index)?;

    let (callback_tx, callback_rx) = mpsc::sync_channel(CALLBACK_QUEUE_CAPACITY);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback = codec_callback(callback_tx, Arc::clone(&overflow));
    let mut codec = MediaCodec::from_decoder_type(&mime).ok_or_else(|| {
        decode_error(
            PlatformErrorCode::ProviderUnavailable,
            "decode.mediacodec.open",
            "MediaCodec decoder is unavailable for the selected MIME type",
        )
    })?;
    codec
        .set_async_notify_callback(Some(callback))
        .map_err(|_| media_codec_error("decode.mediacodec.callback"))?;

    codec
        .configure(&track_format, None, MediaCodecDirection::Decoder)
        .map_err(|_| media_codec_error("decode.mediacodec.configure"))?;
    codec
        .start()
        .map_err(|_| media_codec_error("decode.mediacodec.start"))?;
    let decode_result = drive_audio_codec(
        &codec,
        &mut extractor,
        callback_rx,
        overflow,
        max_output_bytes,
    );
    let stop_result = codec.stop();
    let _ = codec.set_async_notify_callback(None);
    stop_result.map_err(|_| media_codec_error("decode.mediacodec.stop"))?;
    let decoded = decode_result?;
    let sample_rate = decoded
        .sample_rate
        .or(sample_rate)
        .ok_or_else(|| media_codec_error("decode.mediacodec.audio_format"))?;
    let channels = decoded
        .channels
        .or(channels)
        .ok_or_else(|| media_codec_error("decode.mediacodec.audio_format"))?;
    if decoded.samples.is_empty() || decoded.samples.len() % usize::from(channels) != 0 {
        return Err(media_codec_error("decode.mediacodec.audio_alignment"));
    }
    Ok(DecodeOutput::AudioPcmI16 {
        sample_rate,
        channels,
        samples: decoded.samples,
    })
}

pub(super) struct AndroidVideoStream {
    pub(super) _input: TempInput,
    pub(super) extractor: Extractor,
    pub(super) codec: MediaCodec,
    pub(super) events: Receiver<CodecEvent>,
    pub(super) overflow: Arc<AtomicBool>,
    pub(super) image_reader: ImageReader,
    pub(super) pending_pts: std::collections::VecDeque<u64>,
    pub(super) pending_frame: Option<DecodedVideoFrame>,
    pub(super) ready_frame: Option<DecodedVideoFrame>,
    pub(super) input_eos: bool,
    pub(super) output_eos: bool,
    pub(super) end_emitted: bool,
    pub(super) duration_us: u64,
    pub(super) frame_count: u64,
    pub(super) decoded_byte_count: u64,
    pub(super) max_decoded_byte_count: u64,
    pub(super) started: Instant,
    pub(super) progress_deadline: Instant,
}
