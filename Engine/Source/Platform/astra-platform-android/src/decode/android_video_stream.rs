use super::*;

impl AndroidVideoStream {
    pub(super) fn open(
        temp_root: &Path,
        codec_name: &str,
        encoded: &[u8],
        max_output_bytes: usize,
    ) -> Result<Self, PlatformError> {
        let input = TempInput::create(temp_root, encoded)?;
        let mut extractor = Extractor::new(&input.file)?;
        let (track_index, mut format, mime) = extractor.select_track(DecodeKind::Video)?;
        validate_codec_name(codec_name, &mime, DecodeKind::Video)?;
        let duration_us = format
            .i64("durationUs")
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| media_codec_error("decode.mediacodec.video_duration"))?;
        let width = format
            .i32("width")
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| media_codec_error("decode.mediacodec.video_dimensions"))?;
        let height = format
            .i32("height")
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| media_codec_error("decode.mediacodec.video_dimensions"))?;
        let frame_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(decode_budget_error)?;
        let max_decoded_byte_count =
            u64::try_from(max_output_bytes).map_err(|_| decode_budget_error())?;
        if frame_bytes > max_decoded_byte_count {
            return Err(decode_budget_error());
        }
        extractor.select(track_index)?;
        format.set_i32("color-format", 0x7f42_0888);

        let (event_tx, events) = mpsc::sync_channel(CALLBACK_QUEUE_CAPACITY);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback = codec_callback(event_tx.clone(), Arc::clone(&overflow));
        let mut codec = MediaCodec::from_decoder_type(&mime).ok_or_else(|| {
            decode_error(
                PlatformErrorCode::ProviderUnavailable,
                "decode.mediacodec.open",
                "MediaCodec decoder is unavailable for the selected video MIME type",
            )
        })?;
        codec
            .set_async_notify_callback(Some(callback))
            .map_err(|_| media_codec_error("decode.mediacodec.callback"))?;

        let image_overflow = Arc::clone(&overflow);
        let mut image_reader = ImageReader::new(
            i32::try_from(width)
                .map_err(|_| media_codec_error("decode.mediacodec.video_dimensions"))?,
            i32::try_from(height)
                .map_err(|_| media_codec_error("decode.mediacodec.video_dimensions"))?,
            ImageFormat::YUV_420_888,
            4,
        )
        .map_err(|_| media_codec_error("decode.mediacodec.image_reader"))?;
        image_reader
            .set_image_listener(Box::new(move |_| {
                if event_tx.try_send(CodecEvent::Image).is_err() {
                    image_overflow.store(true, Ordering::Release);
                }
            }))
            .map_err(|_| media_codec_error("decode.mediacodec.image_listener"))?;
        let window = image_reader
            .window()
            .map_err(|_| media_codec_error("decode.mediacodec.image_surface"))?;
        codec
            .configure(&format, Some(&window), MediaCodecDirection::Decoder)
            .map_err(|_| media_codec_error("decode.mediacodec.configure"))?;
        codec
            .start()
            .map_err(|_| media_codec_error("decode.mediacodec.start"))?;
        let started = Instant::now();
        Ok(Self {
            _input: input,
            extractor,
            codec,
            events,
            overflow,
            image_reader,
            pending_pts: std::collections::VecDeque::new(),
            pending_frame: None,
            ready_frame: None,
            input_eos: false,
            output_eos: false,
            end_emitted: false,
            duration_us,
            frame_count: 0,
            decoded_byte_count: 0,
            max_decoded_byte_count,
            started,
            progress_deadline: started + CALLBACK_IDLE_TIMEOUT,
        })
    }

    pub(super) fn next_output(&mut self) -> Result<DecodeOutput, PlatformError> {
        if self.end_emitted {
            return Err(media_codec_error("decode.video.stream.ended"));
        }
        loop {
            if let Some(mut frame) = self.ready_frame.take() {
                self.frame_count = self
                    .frame_count
                    .checked_add(1)
                    .ok_or_else(decode_budget_error)?;
                if self.frame_count > MAX_VIDEO_FRAMES {
                    return Err(decode_budget_error());
                }
                frame.sequence = self.frame_count;
                self.decoded_byte_count = self
                    .decoded_byte_count
                    .checked_add(frame.bgra8.len() as u64)
                    .ok_or_else(decode_budget_error)?;
                if self.decoded_byte_count > self.max_decoded_byte_count {
                    return Err(decode_budget_error());
                }
                return Ok(DecodeOutput::VideoFrame {
                    sequence: frame.sequence,
                    pts_us: frame.pts_us,
                    duration_us: frame.duration_us,
                    width: frame.width,
                    height: frame.height,
                    bgra8: frame.bgra8,
                });
            }
            if self.output_eos && self.pending_pts.is_empty() {
                if let Some(mut frame) = self.pending_frame.take() {
                    frame.duration_us = self
                        .duration_us
                        .checked_sub(frame.pts_us)
                        .filter(|duration| *duration > 0)
                        .ok_or_else(|| media_codec_error("decode.mediacodec.video_duration"))?;
                    self.ready_frame = Some(frame);
                    continue;
                }
                self.end_emitted = true;
                return Ok(DecodeOutput::VideoStreamEnd {
                    frame_count: self.frame_count,
                    decoded_byte_count: self.decoded_byte_count,
                });
            }
            if self.overflow.load(Ordering::Acquire) {
                return Err(decode_error(
                    PlatformErrorCode::QueueOverflow,
                    "decode.mediacodec.callback",
                    "MediaCodec callback queue overflowed",
                ));
            }
            let now = Instant::now();
            let total_deadline = self.started + DECODE_TOTAL_TIMEOUT;
            let remaining = self
                .progress_deadline
                .min(total_deadline)
                .saturating_duration_since(now);
            if remaining.is_zero() {
                return Err(media_codec_error("decode.mediacodec.timeout"));
            }
            let event = self
                .events
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => {
                        media_codec_error("decode.mediacodec.timeout")
                    }
                    mpsc::RecvTimeoutError::Disconnected => {
                        media_codec_error("decode.mediacodec.callback_disconnected")
                    }
                })?;
            self.progress_deadline = Instant::now() + CALLBACK_IDLE_TIMEOUT;
            self.handle_event(event)?;
        }
    }

    pub(super) fn handle_event(&mut self, event: CodecEvent) -> Result<(), PlatformError> {
        match event {
            CodecEvent::Input(index) if !self.input_eos => {
                let buffer = self
                    .codec
                    .input_buffer(index)
                    .ok_or_else(|| media_codec_error("decode.mediacodec.input_buffer"))?;
                if let Some((size, pts_us, flags)) = self.extractor.read_sample(buffer)? {
                    self.codec
                        .queue_input_buffer_by_index(index, 0, size, pts_us, flags)
                        .map_err(|_| media_codec_error("decode.mediacodec.queue_input"))?;
                    self.extractor.advance()?;
                } else {
                    self.codec
                        .queue_input_buffer_by_index(
                            index,
                            0,
                            0,
                            0,
                            ndk_sys::AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                        )
                        .map_err(|_| media_codec_error("decode.mediacodec.queue_eos"))?;
                    self.input_eos = true;
                }
            }
            CodecEvent::Input(_) | CodecEvent::FormatChanged { .. } => {}
            CodecEvent::Output(index, info) => {
                let size = usize::try_from(info.size())
                    .map_err(|_| media_codec_error("decode.mediacodec.output_size"))?;
                if size > 0 {
                    let pts_us = u64::try_from(info.presentation_time_us())
                        .map_err(|_| media_codec_error("decode.mediacodec.output_pts"))?;
                    self.pending_pts.push_back(pts_us);
                    self.codec
                        .release_output_buffer_by_index(index, true)
                        .map_err(|_| media_codec_error("decode.mediacodec.render_output"))?;
                } else {
                    self.codec
                        .release_output_buffer_by_index(index, false)
                        .map_err(|_| media_codec_error("decode.mediacodec.release_output"))?;
                }
                self.output_eos =
                    info.flags() & ndk_sys::AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0;
            }
            CodecEvent::Image => self.acquire_image()?,
            CodecEvent::Error => {
                return Err(media_codec_error("decode.mediacodec.callback_error"));
            }
        }
        Ok(())
    }

    pub(super) fn acquire_image(&mut self) -> Result<(), PlatformError> {
        let image = match self
            .image_reader
            .acquire_next_image()
            .map_err(|_| media_codec_error("decode.mediacodec.acquire_image"))?
        {
            AcquireResult::Image(image) => image,
            AcquireResult::NoBufferAvailable => return Ok(()),
            AcquireResult::MaxImagesAcquired => {
                return Err(media_codec_error("decode.mediacodec.max_images"));
            }
        };
        let pts_us = self
            .pending_pts
            .pop_front()
            .ok_or_else(|| media_codec_error("decode.mediacodec.image_without_pts"))?;
        let (width, height, bgra8) = yuv420_to_bgra(&image)?;
        let frame = DecodedVideoFrame {
            sequence: 0,
            pts_us,
            duration_us: 0,
            width,
            height,
            bgra8: bgra8.into(),
        };
        if let Some(mut previous) = self.pending_frame.replace(frame) {
            previous.duration_us = pts_us
                .checked_sub(previous.pts_us)
                .filter(|duration| *duration > 0)
                .ok_or_else(|| media_codec_error("decode.mediacodec.video_pts"))?;
            if self.ready_frame.replace(previous).is_some() {
                return Err(media_codec_error("decode.mediacodec.frame_backlog"));
            }
        }
        Ok(())
    }
}

impl Drop for AndroidVideoStream {
    fn drop(&mut self) {
        let _ = self.codec.stop();
        let _ = self.codec.set_async_notify_callback(None);
    }
}

pub(super) enum CodecEvent {
    Input(usize),
    Output(usize, BufferInfo),
    FormatChanged {
        sample_rate: Option<u32>,
        channels: Option<u16>,
    },
    Image,
    Error,
}

pub(super) fn codec_callback(
    tx: SyncSender<CodecEvent>,
    overflow: Arc<AtomicBool>,
) -> AsyncNotifyCallback {
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

pub(super) struct RawDecodedAudio {
    pub(super) samples: Vec<i16>,
    pub(super) sample_rate: Option<u32>,
    pub(super) channels: Option<u16>,
}
