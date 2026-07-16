#![cfg(target_os = "android")]

use std::{
    fs::{File, OpenOptions},
    io::Write,
    mem::MaybeUninit,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use astra_core::Hash256;
use astra_media::{DecodeProvider, DecodedVideoFrame, DecodedVideoStream};
use astra_platform::{
    DecodeKind, DecodeOutput, PlatformDecodeRequest, PlatformError, PlatformErrorCode,
};
use ndk::media::{
    image_reader::{AcquireResult, Image, ImageFormat, ImageReader},
    media_codec::{AsyncNotifyCallback, BufferInfo, MediaCodec, MediaCodecDirection},
    media_format::MediaFormat,
};

const CALLBACK_QUEUE_CAPACITY: usize = 64;
const CALLBACK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DECODE_TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_VIDEO_FRAMES: u64 = 10_000;

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

enum DecodeWork {
    Decode(
        PlatformDecodeRequest,
        tokio::sync::oneshot::Sender<Result<DecodeOutput, PlatformError>>,
    ),
    Close(tokio::sync::oneshot::Sender<Result<(), PlatformError>>),
}

pub(crate) struct AndroidDecodeWorker {
    tx: mpsc::SyncSender<DecodeWork>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl AndroidDecodeWorker {
    pub(crate) fn new(
        kind: DecodeKind,
        temp_root: PathBuf,
        max_output_bytes: usize,
    ) -> Result<Self, PlatformError> {
        let mut resource = DecodeResource::new(kind, temp_root, max_output_bytes)?;
        let (tx, rx) = mpsc::sync_channel(1);
        let join = std::thread::Builder::new()
            .name("astra-mediacodec".to_string())
            .spawn(move || {
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
    kind: DecodeKind,
    next_sequence: u64,
    temp_root: PathBuf,
    max_output_bytes: usize,
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
        })
    }

    pub(crate) fn decode(
        &mut self,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        if request.sequence != self.next_sequence
            || request.kind != self.kind
            || request.bytes.is_empty()
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
        let output = match request.kind {
            DecodeKind::Image => decode_image(request)?,
            DecodeKind::Audio | DecodeKind::Video => {
                decode_media_codec(&self.temp_root, request, self.max_output_bytes)?
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

fn decode_image(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
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
        astra_media::DecodeOutput::CpuBuffer {
            bytes,
            format,
            hash,
        } => Ok(DecodeOutput::CpuBuffer {
            format,
            bytes,
            hash: hash.to_string(),
        }),
        astra_media::DecodeOutput::MediaSurfaceToken(_) => Err(decode_error(
            PlatformErrorCode::InvalidState,
            "decode.image",
            "verified image decoder returned a native token",
        )),
    }
}

fn decode_media_codec(
    temp_root: &Path,
    request: PlatformDecodeRequest,
    max_output_bytes: usize,
) -> Result<DecodeOutput, PlatformError> {
    let input = TempInput::create(temp_root, &request.bytes)?;
    let mut extractor = Extractor::new(&input.file)?;
    let (track_index, mut track_format, mime) = extractor.select_track(request.kind)?;
    validate_codec_name(&request.codec, &mime, request.kind)?;
    let duration_us = track_format
        .i64("durationUs")
        .and_then(|value| u64::try_from(value).ok());
    let width = track_format
        .i32("width")
        .and_then(|value| u32::try_from(value).ok());
    let height = track_format
        .i32("height")
        .and_then(|value| u32::try_from(value).ok());
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

    let mut image_reader = None;
    let mut image_rx = None;
    let output_window = if request.kind == DecodeKind::Video {
        let width = width
            .filter(|value| *value > 0)
            .ok_or_else(|| media_codec_error("decode.mediacodec.video_dimensions"))?;
        let height = height
            .filter(|value| *value > 0)
            .ok_or_else(|| media_codec_error("decode.mediacodec.video_dimensions"))?;
        let frame_bytes = usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| media_codec_error("decode.mediacodec.video_dimensions"))?;
        if frame_bytes > max_output_bytes {
            return Err(decode_budget_error());
        }
        track_format.set_i32("color-format", 0x7f42_0888);
        let (image_tx, receiver) = mpsc::sync_channel(CALLBACK_QUEUE_CAPACITY);
        let image_overflow = Arc::clone(&overflow);
        let mut reader = ImageReader::new(
            i32::try_from(width)
                .map_err(|_| media_codec_error("decode.mediacodec.video_dimensions"))?,
            i32::try_from(height)
                .map_err(|_| media_codec_error("decode.mediacodec.video_dimensions"))?,
            ImageFormat::YUV_420_888,
            4,
        )
        .map_err(|_| media_codec_error("decode.mediacodec.image_reader"))?;
        reader
            .set_image_listener(Box::new(move |_| {
                if image_tx.try_send(()).is_err() {
                    image_overflow.store(true, Ordering::Release);
                }
            }))
            .map_err(|_| media_codec_error("decode.mediacodec.image_listener"))?;
        let window = reader
            .window()
            .map_err(|_| media_codec_error("decode.mediacodec.image_surface"))?;
        image_rx = Some(receiver);
        image_reader = Some(reader);
        Some(window)
    } else {
        None
    };

    codec
        .configure(
            &track_format,
            output_window.as_ref(),
            MediaCodecDirection::Decoder,
        )
        .map_err(|_| media_codec_error("decode.mediacodec.configure"))?;
    codec
        .start()
        .map_err(|_| media_codec_error("decode.mediacodec.start"))?;
    let decode_result = drive_codec(
        &codec,
        &mut extractor,
        callback_rx,
        overflow,
        image_reader.as_ref(),
        image_rx.as_ref(),
        request.kind,
        max_output_bytes,
    );
    let stop_result = codec.stop();
    let _ = codec.set_async_notify_callback(None);
    stop_result.map_err(|_| media_codec_error("decode.mediacodec.stop"))?;
    let decoded = decode_result?;
    match decoded {
        RawDecoded::Audio {
            bytes,
            sample_rate: output_sample_rate,
            channels: output_channels,
        } => {
            let sample_rate = output_sample_rate
                .or(sample_rate)
                .ok_or_else(|| media_codec_error("decode.mediacodec.audio_format"))?;
            let channels = output_channels
                .or(channels)
                .ok_or_else(|| media_codec_error("decode.mediacodec.audio_format"))?;
            if bytes.is_empty() || bytes.len() % (usize::from(channels) * 2) != 0 {
                return Err(media_codec_error("decode.mediacodec.audio_alignment"));
            }
            let hash = Hash256::from_sha256(&bytes).to_string();
            Ok(DecodeOutput::CpuBuffer {
                format: format!("pcm_s16le:{sample_rate}:{channels}"),
                bytes,
                hash,
            })
        }
        RawDecoded::Video(mut frames) => {
            finalize_video_frames(&mut frames, duration_us)?;
            let total_duration = frames
                .last()
                .and_then(|frame| frame.pts_us.checked_add(frame.duration_us))
                .ok_or_else(|| media_codec_error("decode.mediacodec.video_duration"))?;
            let stream = DecodedVideoStream {
                schema: astra_media::DECODED_VIDEO_STREAM_SCHEMA.to_string(),
                duration_us: total_duration,
                frames,
            };
            let bytes = stream
                .encode(MAX_VIDEO_FRAMES, max_output_bytes as u64)
                .map_err(|_| decode_budget_error())?;
            let hash = Hash256::from_sha256(&bytes).to_string();
            Ok(DecodeOutput::CpuBuffer {
                format: format!("postcard:{}", astra_media::DECODED_VIDEO_STREAM_SCHEMA),
                bytes,
                hash,
            })
        }
    }
}

enum CodecEvent {
    Input(usize),
    Output(usize, BufferInfo),
    FormatChanged {
        sample_rate: Option<u32>,
        channels: Option<u16>,
    },
    Error,
}

fn codec_callback(tx: SyncSender<CodecEvent>, overflow: Arc<AtomicBool>) -> AsyncNotifyCallback {
    let input_tx = tx.clone();
    let input_overflow = Arc::clone(&overflow);
    let output_tx = tx.clone();
    let output_overflow = Arc::clone(&overflow);
    let format_tx = tx.clone();
    let format_overflow = Arc::clone(&overflow);
    AsyncNotifyCallback {
        on_input_available: Some(Box::new(move |index| {
            if input_tx.try_send(CodecEvent::Input(index)).is_err() {
                input_overflow.store(true, Ordering::Release);
            }
        })),
        on_output_available: Some(Box::new(move |index, info| {
            if output_tx
                .try_send(CodecEvent::Output(index, *info))
                .is_err()
            {
                output_overflow.store(true, Ordering::Release);
            }
        })),
        on_format_changed: Some(Box::new(move |format| {
            let sample_rate = format
                .i32("sample-rate")
                .and_then(|value| u32::try_from(value).ok());
            let channels = format
                .i32("channel-count")
                .and_then(|value| u16::try_from(value).ok());
            if format_tx
                .try_send(CodecEvent::FormatChanged {
                    sample_rate,
                    channels,
                })
                .is_err()
            {
                format_overflow.store(true, Ordering::Release);
            }
        })),
        on_error: Some(Box::new(move |_, _, _| {
            if tx.try_send(CodecEvent::Error).is_err() {
                overflow.store(true, Ordering::Release);
            }
        })),
    }
}

enum RawDecoded {
    Audio {
        bytes: Vec<u8>,
        sample_rate: Option<u32>,
        channels: Option<u16>,
    },
    Video(Vec<DecodedVideoFrame>),
}

#[allow(clippy::too_many_arguments)]
fn drive_codec(
    codec: &MediaCodec,
    extractor: &mut Extractor,
    events: Receiver<CodecEvent>,
    overflow: Arc<AtomicBool>,
    image_reader: Option<&ImageReader>,
    image_events: Option<&Receiver<()>>,
    kind: DecodeKind,
    max_output_bytes: usize,
) -> Result<RawDecoded, PlatformError> {
    let started = Instant::now();
    let total_deadline = started + DECODE_TOTAL_TIMEOUT;
    let mut progress_deadline = started + CALLBACK_IDLE_TIMEOUT;
    let mut input_eos = false;
    let mut output_eos = false;
    let mut audio = Vec::new();
    let mut output_sample_rate = None;
    let mut output_channels = None;
    let mut video = Vec::new();
    let mut pending_video_pts = std::collections::VecDeque::new();
    while !output_eos || !pending_video_pts.is_empty() {
        if overflow.load(Ordering::Acquire) {
            return Err(decode_error(
                PlatformErrorCode::QueueOverflow,
                "decode.mediacodec.callback",
                "MediaCodec callback queue overflowed",
            ));
        }
        let drained_images = drain_images(
            image_reader,
            image_events,
            &mut pending_video_pts,
            &mut video,
            max_output_bytes,
        )?;
        if drained_images != 0 {
            progress_deadline = Instant::now() + CALLBACK_IDLE_TIMEOUT;
        }
        if output_eos && pending_video_pts.is_empty() {
            break;
        }
        let now = Instant::now();
        let remaining = progress_deadline
            .min(total_deadline)
            .saturating_duration_since(now);
        if remaining.is_zero() {
            return Err(media_codec_error("decode.mediacodec.timeout"));
        }
        match events.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(CodecEvent::Input(index)) if !input_eos => {
                progress_deadline = Instant::now() + CALLBACK_IDLE_TIMEOUT;
                let buffer = codec
                    .input_buffer(index)
                    .ok_or_else(|| media_codec_error("decode.mediacodec.input_buffer"))?;
                let read = extractor.read_sample(buffer)?;
                if let Some((size, pts_us, flags)) = read {
                    codec
                        .queue_input_buffer_by_index(index, 0, size, pts_us, flags)
                        .map_err(|_| media_codec_error("decode.mediacodec.queue_input"))?;
                    extractor.advance()?;
                } else {
                    codec
                        .queue_input_buffer_by_index(
                            index,
                            0,
                            0,
                            0,
                            ndk_sys::AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                        )
                        .map_err(|_| media_codec_error("decode.mediacodec.queue_eos"))?;
                    input_eos = true;
                }
            }
            Ok(CodecEvent::Input(_)) => {}
            Ok(CodecEvent::Output(index, info)) => {
                progress_deadline = Instant::now() + CALLBACK_IDLE_TIMEOUT;
                let size = usize::try_from(info.size())
                    .map_err(|_| media_codec_error("decode.mediacodec.output_size"))?;
                let offset = usize::try_from(info.offset())
                    .map_err(|_| media_codec_error("decode.mediacodec.output_offset"))?;
                if size > 0 {
                    if kind == DecodeKind::Audio {
                        let buffer = codec
                            .output_buffer(index)
                            .ok_or_else(|| media_codec_error("decode.mediacodec.output_buffer"))?;
                        let end = offset.checked_add(size).ok_or_else(decode_budget_error)?;
                        let bytes = buffer
                            .get(offset..end)
                            .ok_or_else(|| media_codec_error("decode.mediacodec.output_bounds"))?;
                        if audio.len().saturating_add(bytes.len()) > max_output_bytes {
                            return Err(decode_budget_error());
                        }
                        audio.extend_from_slice(bytes);
                        codec
                            .release_output_buffer_by_index(index, false)
                            .map_err(|_| media_codec_error("decode.mediacodec.release_output"))?;
                    } else {
                        let pts = u64::try_from(info.presentation_time_us())
                            .map_err(|_| media_codec_error("decode.mediacodec.output_pts"))?;
                        pending_video_pts.push_back(pts);
                        codec
                            .release_output_buffer_by_index(index, true)
                            .map_err(|_| media_codec_error("decode.mediacodec.render_output"))?;
                    }
                } else {
                    codec
                        .release_output_buffer_by_index(index, false)
                        .map_err(|_| media_codec_error("decode.mediacodec.release_output"))?;
                }
                output_eos = info.flags() & ndk_sys::AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0;
            }
            Ok(CodecEvent::FormatChanged {
                sample_rate,
                channels,
            }) => {
                output_sample_rate = sample_rate.or(output_sample_rate);
                output_channels = channels.or(output_channels);
                progress_deadline = Instant::now() + CALLBACK_IDLE_TIMEOUT;
            }
            Ok(CodecEvent::Error) => {
                return Err(media_codec_error("decode.mediacodec.callback_error"))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(media_codec_error("decode.mediacodec.callback_disconnected"));
            }
        }
    }
    if !input_eos || !output_eos {
        return Err(media_codec_error("decode.mediacodec.eos"));
    }
    match kind {
        DecodeKind::Audio => Ok(RawDecoded::Audio {
            bytes: audio,
            sample_rate: output_sample_rate,
            channels: output_channels,
        }),
        DecodeKind::Video => Ok(RawDecoded::Video(video)),
        DecodeKind::Image => unreachable!("image decoding is handled separately"),
    }
}

fn drain_images(
    reader: Option<&ImageReader>,
    events: Option<&Receiver<()>>,
    pending_pts: &mut std::collections::VecDeque<u64>,
    frames: &mut Vec<DecodedVideoFrame>,
    max_output_bytes: usize,
) -> Result<usize, PlatformError> {
    let (Some(reader), Some(events)) = (reader, events) else {
        return Ok(0);
    };
    let initial_frame_count = frames.len();
    while events.try_recv().is_ok() {
        match reader
            .acquire_next_image()
            .map_err(|_| media_codec_error("decode.mediacodec.acquire_image"))?
        {
            AcquireResult::Image(image) => {
                let pts_us = pending_pts
                    .pop_front()
                    .ok_or_else(|| media_codec_error("decode.mediacodec.image_without_pts"))?;
                let (width, height, bgra8) = yuv420_to_bgra(&image)?;
                let prior_bytes = frames
                    .iter()
                    .try_fold(0usize, |total, frame| total.checked_add(frame.bgra8.len()))
                    .ok_or_else(decode_budget_error)?;
                if prior_bytes.saturating_add(bgra8.len()) > max_output_bytes
                    || frames.len() as u64 >= MAX_VIDEO_FRAMES
                {
                    return Err(decode_budget_error());
                }
                let content_hash = Hash256::from_sha256(&bgra8);
                frames.push(DecodedVideoFrame {
                    sequence: frames.len() as u64 + 1,
                    pts_us,
                    duration_us: 0,
                    width,
                    height,
                    bgra8,
                    content_hash,
                });
            }
            AcquireResult::NoBufferAvailable => break,
            AcquireResult::MaxImagesAcquired => {
                return Err(media_codec_error("decode.mediacodec.max_images"));
            }
        }
    }
    Ok(frames.len() - initial_frame_count)
}

fn yuv420_to_bgra(image: &Image) -> Result<(u32, u32, Vec<u8>), PlatformError> {
    if image
        .format()
        .map_err(|_| media_codec_error("decode.mediacodec.image_format"))?
        != ImageFormat::YUV_420_888
        || image
            .number_of_planes()
            .map_err(|_| media_codec_error("decode.mediacodec.image_planes"))?
            != 3
    {
        return Err(media_codec_error("decode.mediacodec.image_format"));
    }
    let crop = image
        .crop_rect()
        .map_err(|_| media_codec_error("decode.mediacodec.image_crop"))?;
    let width = u32::try_from(crop.right - crop.left)
        .map_err(|_| media_codec_error("decode.mediacodec.image_crop"))?;
    let height = u32::try_from(crop.bottom - crop.top)
        .map_err(|_| media_codec_error("decode.mediacodec.image_crop"))?;
    if width == 0 || height == 0 {
        return Err(media_codec_error("decode.mediacodec.image_crop"));
    }
    let y = plane(image, 0)?;
    let u = plane(image, 1)?;
    let v = plane(image, 2)?;
    let mut output = vec![
        0_u8;
        usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| decode_budget_error())?
    ];
    for row in 0..usize::try_from(height).map_err(|_| decode_budget_error())? {
        for column in 0..usize::try_from(width).map_err(|_| decode_budget_error())? {
            let source_x = usize::try_from(crop.left)
                .map_err(|_| media_codec_error("decode.mediacodec.image_crop"))?
                + column;
            let source_y = usize::try_from(crop.top)
                .map_err(|_| media_codec_error("decode.mediacodec.image_crop"))?
                + row;
            let y_value = plane_value(&y, source_x, source_y)? as f32;
            let u_value = plane_value(&u, source_x / 2, source_y / 2)? as f32 - 128.0;
            let v_value = plane_value(&v, source_x / 2, source_y / 2)? as f32 - 128.0;
            let c = (y_value - 16.0).max(0.0) * 1.164;
            let r = (c + 1.596 * v_value).round().clamp(0.0, 255.0) as u8;
            let g = (c - 0.392 * u_value - 0.813 * v_value)
                .round()
                .clamp(0.0, 255.0) as u8;
            let b = (c + 2.017 * u_value).round().clamp(0.0, 255.0) as u8;
            let offset = (row * width as usize + column) * 4;
            output[offset..offset + 4].copy_from_slice(&[b, g, r, 255]);
        }
    }
    Ok((width, height, output))
}

struct Plane<'a> {
    bytes: &'a [u8],
    row_stride: usize,
    pixel_stride: usize,
}

fn plane(image: &Image, index: i32) -> Result<Plane<'_>, PlatformError> {
    Ok(Plane {
        bytes: image
            .plane_data(index)
            .map_err(|_| media_codec_error("decode.mediacodec.plane_data"))?,
        row_stride: usize::try_from(
            image
                .plane_row_stride(index)
                .map_err(|_| media_codec_error("decode.mediacodec.plane_stride"))?,
        )
        .map_err(|_| media_codec_error("decode.mediacodec.plane_stride"))?,
        pixel_stride: usize::try_from(
            image
                .plane_pixel_stride(index)
                .map_err(|_| media_codec_error("decode.mediacodec.plane_stride"))?,
        )
        .map_err(|_| media_codec_error("decode.mediacodec.plane_stride"))?,
    })
}

fn plane_value(plane: &Plane<'_>, x: usize, y: usize) -> Result<u8, PlatformError> {
    let offset = y
        .checked_mul(plane.row_stride)
        .and_then(|offset| {
            x.checked_mul(plane.pixel_stride)
                .and_then(|x| offset.checked_add(x))
        })
        .ok_or_else(|| media_codec_error("decode.mediacodec.plane_bounds"))?;
    plane
        .bytes
        .get(offset)
        .copied()
        .ok_or_else(|| media_codec_error("decode.mediacodec.plane_bounds"))
}

fn finalize_video_frames(
    frames: &mut [DecodedVideoFrame],
    declared_duration: Option<u64>,
) -> Result<(), PlatformError> {
    if frames.is_empty() {
        return Err(media_codec_error("decode.mediacodec.video_empty"));
    }
    for index in 0..frames.len().saturating_sub(1) {
        frames[index].duration_us = frames[index + 1]
            .pts_us
            .checked_sub(frames[index].pts_us)
            .filter(|duration| *duration > 0)
            .ok_or_else(|| media_codec_error("decode.mediacodec.video_pts"))?;
    }
    let last = frames.len() - 1;
    let fallback = frames
        .get(last.saturating_sub(1))
        .map(|frame| frame.duration_us)
        .filter(|duration| *duration > 0)
        .unwrap_or(1);
    frames[last].duration_us = declared_duration
        .and_then(|duration| duration.checked_sub(frames[last].pts_us))
        .filter(|duration| *duration > 0)
        .unwrap_or(fallback);
    Ok(())
}

struct Extractor {
    raw: NonNull<ndk_sys::AMediaExtractor>,
}

impl Extractor {
    fn new(file: &File) -> Result<Self, PlatformError> {
        let raw = NonNull::new(unsafe { ndk_sys::AMediaExtractor_new() })
            .ok_or_else(|| media_codec_error("decode.extractor.open"))?;
        let length = file
            .metadata()
            .map_err(|_| media_codec_error("decode.extractor.metadata"))?
            .len();
        let status = unsafe {
            ndk_sys::AMediaExtractor_setDataSourceFd(
                raw.as_ptr(),
                file.as_raw_fd(),
                0,
                i64::try_from(length).map_err(|_| decode_budget_error())?,
            )
        };
        if status != ndk_sys::media_status_t::AMEDIA_OK {
            unsafe { ndk_sys::AMediaExtractor_delete(raw.as_ptr()) };
            return Err(media_codec_error("decode.extractor.data_source"));
        }
        Ok(Self { raw })
    }

    fn select_track(
        &mut self,
        kind: DecodeKind,
    ) -> Result<(usize, MediaFormat, String), PlatformError> {
        let prefix = match kind {
            DecodeKind::Audio => "audio/",
            DecodeKind::Video => "video/",
            DecodeKind::Image => return Err(media_codec_error("decode.extractor.kind")),
        };
        let count = unsafe { ndk_sys::AMediaExtractor_getTrackCount(self.raw.as_ptr()) };
        let mut selected = None;
        for index in 0..count {
            let raw_format = NonNull::new(unsafe {
                ndk_sys::AMediaExtractor_getTrackFormat(self.raw.as_ptr(), index)
            })
            .ok_or_else(|| media_codec_error("decode.extractor.track_format"))?;
            let mut format = unsafe { MediaFormat::from_ptr(raw_format) };
            let mime = format.str("mime").map(str::to_string);
            if mime.as_deref().is_some_and(|mime| mime.starts_with(prefix)) {
                if selected.is_some() {
                    return Err(media_codec_error("decode.extractor.ambiguous_track"));
                }
                selected = mime.map(|mime| (index, format, mime));
            }
        }
        selected.ok_or_else(|| media_codec_error("decode.extractor.track_missing"))
    }

    fn select(&mut self, index: usize) -> Result<(), PlatformError> {
        let status = unsafe { ndk_sys::AMediaExtractor_selectTrack(self.raw.as_ptr(), index) };
        (status == ndk_sys::media_status_t::AMEDIA_OK)
            .then_some(())
            .ok_or_else(|| media_codec_error("decode.extractor.select_track"))
    }

    fn read_sample(
        &mut self,
        buffer: &mut [MaybeUninit<u8>],
    ) -> Result<Option<(usize, u64, u32)>, PlatformError> {
        let read = unsafe {
            ndk_sys::AMediaExtractor_readSampleData(
                self.raw.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if read < 0 {
            return Ok(None);
        }
        let pts = unsafe { ndk_sys::AMediaExtractor_getSampleTime(self.raw.as_ptr()) };
        let pts =
            u64::try_from(pts).map_err(|_| media_codec_error("decode.extractor.sample_pts"))?;
        let flags = unsafe { ndk_sys::AMediaExtractor_getSampleFlags(self.raw.as_ptr()) };
        Ok(Some((read as usize, pts, flags)))
    }

    fn advance(&mut self) -> Result<(), PlatformError> {
        let _ = unsafe { ndk_sys::AMediaExtractor_advance(self.raw.as_ptr()) };
        Ok(())
    }
}

impl Drop for Extractor {
    fn drop(&mut self) {
        unsafe { ndk_sys::AMediaExtractor_delete(self.raw.as_ptr()) };
    }
}

struct TempInput {
    file: File,
    path: PathBuf,
}

impl TempInput {
    fn create(root: &Path, bytes: &[u8]) -> Result<Self, PlatformError> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let epoch_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| media_codec_error("decode.scratch.clock"))?
            .as_nanos();
        let mut created = None;
        for _ in 0..16 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!(
                "decode-{}-{epoch_ns}-{id}.media",
                std::process::id()
            ));
            match OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
            {
                Ok(file) => {
                    created = Some((path, file));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(media_codec_error("decode.scratch.create")),
            }
        }
        let (path, mut file) =
            created.ok_or_else(|| media_codec_error("decode.scratch.collision"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| media_codec_error("decode.scratch.write"))?;
        Ok(Self { file, path })
    }
}

impl Drop for TempInput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn validate_codec_name(codec: &str, mime: &str, kind: DecodeKind) -> Result<(), PlatformError> {
    let accepted = match kind {
        DecodeKind::Audio => matches!(codec, "aac" | "m4a" | "mp3" | "ogg" | "opus" | "wav"),
        DecodeKind::Video => matches!(
            codec,
            "mp4" | "webm" | "h264" | "hevc" | "vp8" | "vp9" | "av1"
        ),
        DecodeKind::Image => false,
    };
    if !accepted || !mime.is_ascii() || !mime.contains('/') {
        return Err(decode_error(
            PlatformErrorCode::ProviderUnavailable,
            "decode.mediacodec.codec",
            "encoded media codec or extracted MIME type is unsupported",
        ));
    }
    Ok(())
}

fn decode_budget_error() -> PlatformError {
    decode_error(
        PlatformErrorCode::QueueOverflow,
        "decode.mediacodec.output_budget",
        "decoded media exceeds the profile-bound output budget",
    )
}

fn media_codec_error(operation: &'static str) -> PlatformError {
    decode_error(
        PlatformErrorCode::ProviderUnavailable,
        operation,
        "Android MediaCodec operation failed",
    )
}

fn decode_error(
    code: PlatformErrorCode,
    operation: &'static str,
    message: &'static str,
) -> PlatformError {
    PlatformError::new(code, operation, message)
}
