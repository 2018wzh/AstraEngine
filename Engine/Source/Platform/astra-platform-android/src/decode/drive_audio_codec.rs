use super::*;

pub(super) fn drive_audio_codec(
    codec: &MediaCodec,
    extractor: &mut Extractor,
    events: Receiver<CodecEvent>,
    overflow: Arc<AtomicBool>,
    max_output_bytes: usize,
) -> Result<RawDecodedAudio, PlatformError> {
    let started = Instant::now();
    let total_deadline = started + DECODE_TOTAL_TIMEOUT;
    let mut progress_deadline = started + CALLBACK_IDLE_TIMEOUT;
    let mut input_eos = false;
    let mut output_eos = false;
    let mut audio = Vec::<i16>::new();
    let mut output_sample_rate = None;
    let mut output_channels = None;
    while !output_eos {
        if overflow.load(Ordering::Acquire) {
            return Err(decode_error(
                PlatformErrorCode::QueueOverflow,
                "decode.mediacodec.callback",
                "MediaCodec callback queue overflowed",
            ));
        }
        let now = Instant::now();
        let remaining = progress_deadline
            .min(total_deadline)
            .saturating_duration_since(now);
        if remaining.is_zero() {
            return Err(media_codec_error("decode.mediacodec.timeout"));
        }
        // The callback channel is the wake source.  The absolute progress and
        // total deadlines above are the only timers: capping every receive to
        // an arbitrary 100 ms interval would turn an event-driven MediaCodec
        // worker back into a periodic poller under a quiet codec.
        match events.recv_timeout(remaining) {
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
                    let buffer = codec
                        .output_buffer(index)
                        .ok_or_else(|| media_codec_error("decode.mediacodec.output_buffer"))?;
                    let end = offset.checked_add(size).ok_or_else(decode_budget_error)?;
                    let bytes = buffer
                        .get(offset..end)
                        .ok_or_else(|| media_codec_error("decode.mediacodec.output_bounds"))?;
                    if !bytes.len().is_multiple_of(std::mem::size_of::<i16>())
                        || audio
                            .len()
                            .saturating_add(bytes.len() / std::mem::size_of::<i16>())
                            .saturating_mul(std::mem::size_of::<i16>())
                            > max_output_bytes
                    {
                        return Err(decode_budget_error());
                    }
                    audio.extend(
                        bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|sample| i16::from_le_bytes([sample[0], sample[1]])),
                    );
                    codec
                        .release_output_buffer_by_index(index, false)
                        .map_err(|_| media_codec_error("decode.mediacodec.release_output"))?;
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
            Ok(CodecEvent::Image) => {
                return Err(media_codec_error("decode.mediacodec.audio_image"));
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
    Ok(RawDecodedAudio {
        samples: audio,
        sample_rate: output_sample_rate,
        channels: output_channels,
    })
}

pub(super) fn yuv420_to_bgra(image: &Image) -> Result<(u32, u32, Vec<u8>), PlatformError> {
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

pub(super) struct Plane<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) row_stride: usize,
    pub(super) pixel_stride: usize,
}

pub(super) fn plane(image: &Image, index: i32) -> Result<Plane<'_>, PlatformError> {
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

pub(super) fn plane_value(plane: &Plane<'_>, x: usize, y: usize) -> Result<u8, PlatformError> {
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

pub(super) struct Extractor {
    pub(super) raw: NonNull<ndk_sys::AMediaExtractor>,
}

impl Extractor {
    pub(super) fn new(file: &File) -> Result<Self, PlatformError> {
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

    pub(super) fn select_track(
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

    pub(super) fn select(&mut self, index: usize) -> Result<(), PlatformError> {
        let status = unsafe { ndk_sys::AMediaExtractor_selectTrack(self.raw.as_ptr(), index) };
        (status == ndk_sys::media_status_t::AMEDIA_OK)
            .then_some(())
            .ok_or_else(|| media_codec_error("decode.extractor.select_track"))
    }

    pub(super) fn read_sample(
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

    pub(super) fn advance(&mut self) -> Result<(), PlatformError> {
        let _ = unsafe { ndk_sys::AMediaExtractor_advance(self.raw.as_ptr()) };
        Ok(())
    }
}

impl Drop for Extractor {
    fn drop(&mut self) {
        unsafe { ndk_sys::AMediaExtractor_delete(self.raw.as_ptr()) };
    }
}

pub(super) struct TempInput {
    pub(super) file: File,
    pub(super) path: PathBuf,
}

impl TempInput {
    pub(super) fn create(root: &Path, bytes: &[u8]) -> Result<Self, PlatformError> {
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

pub(super) fn validate_codec_name(
    codec: &str,
    mime: &str,
    kind: DecodeKind,
) -> Result<(), PlatformError> {
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

pub(super) fn decode_budget_error() -> PlatformError {
    decode_error(
        PlatformErrorCode::QueueOverflow,
        "decode.mediacodec.output_budget",
        "decoded media exceeds the profile-bound output budget",
    )
}

pub(super) fn media_codec_error(operation: &'static str) -> PlatformError {
    decode_error(
        PlatformErrorCode::ProviderUnavailable,
        operation,
        "Android MediaCodec operation failed",
    )
}

pub(super) fn decode_error(
    code: PlatformErrorCode,
    operation: &'static str,
    message: &'static str,
) -> PlatformError {
    PlatformError::new(code, operation, message)
}
