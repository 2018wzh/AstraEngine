use super::*;

impl DecodeWorkerState {
    pub(super) fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
        let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe()
            .map_err(|_| host_error("decode.open", "WMF provider initialization failed"))?;
        Ok(Self {
            kind,
            provider,
            next_sequence: 1,
            video_stream: None,
            audio_stream: None,
        })
    }

    pub(super) fn decode(
        &mut self,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        let capability = self.provider.capability();
        if request.sequence != self.next_sequence || request.kind != self.kind {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence or kind is invalid",
            ));
        }
        let next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence overflowed",
            )
        })?;
        let kind = match request.kind {
            DecodeKind::Image => astra_media::DecodeKind::Image,
            DecodeKind::Audio => astra_media::DecodeKind::Audio,
            DecodeKind::Video => astra_media::DecodeKind::Video,
        };
        if matches!(
            request.stream_action,
            astra_platform::DecodeStreamAction::OneShot | astra_platform::DecodeStreamAction::Start
        ) && !capability
            .codecs
            .iter()
            .any(|codec| codec == &request.codec)
        {
            return Err(PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "decode.submit",
                "the selected WMF provider does not support the requested codec",
            )
            .with_field("diagnostic_code", "ASTRA_WMF_CODEC_UNSUPPORTED")
            .with_field("codec", request.codec));
        }
        let output = match request.stream_action {
            astra_platform::DecodeStreamAction::OneShot => {
                if request.bytes.is_empty()
                    || !request.description.is_empty()
                    || request.sample_rate.is_some()
                    || request.channels.is_some()
                    || request.coded_width.is_some()
                    || request.coded_height.is_some()
                    || self.video_stream.is_some()
                    || self.audio_stream.is_some()
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "decode.submit",
                        "one-shot decode request has invalid metadata or an active stream",
                    ));
                }
                let result = self
                    .provider
                    .decode(&astra_media::DecodeRequest {
                        kind,
                        codec: request.codec,
                        bytes: request.bytes,
                        profile: "desktop-release".to_string(),
                    })
                    .map_err(media_decode_error)?;
                match result.output {
                    MediaDecodeOutput::CpuBuffer { bytes, format } => {
                        DecodeOutput::CpuBuffer { format, bytes }
                    }
                    MediaDecodeOutput::AudioPcmI16 {
                        sample_rate,
                        channels,
                        samples,
                    } => DecodeOutput::AudioPcmI16 {
                        sample_rate,
                        channels,
                        samples,
                    },
                    MediaDecodeOutput::AudioPcmF32 {
                        sample_rate,
                        channels,
                        samples,
                    } => DecodeOutput::AudioPcmF32 {
                        sample_rate,
                        channels,
                        samples,
                    },
                    MediaDecodeOutput::MediaSurfaceToken(_) => {
                        return Err(host_error(
                            "decode.submit",
                            "WMF returned an unsupported external media surface",
                        ));
                    }
                }
            }
            astra_platform::DecodeStreamAction::Start => {
                if request.bytes.is_empty()
                    || !matches!(request.kind, DecodeKind::Video | DecodeKind::Audio)
                    || !request.description.is_empty()
                    || request.sample_rate.is_some()
                    || request.channels.is_some()
                    || request.coded_width.is_some()
                    || request.coded_height.is_some()
                    || self.video_stream.is_some()
                    || self.audio_stream.is_some()
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "decode.stream.start",
                        "stream start request is invalid",
                    ));
                }
                match request.kind {
                    DecodeKind::Video => {
                        let mut decoder = astra_media::open_windows_video_stream(
                            &request.bytes,
                            60 * 60 * 4,
                            512 * 1024 * 1024,
                        )
                        .map_err(media_decode_error)?;
                        let duration_us = decoder.duration_us();
                        let pending = decoder
                            .next_frame()
                            .map_err(media_decode_error)?
                            .ok_or_else(|| {
                                host_error("decode.stream.start", "video stream produced no frames")
                            })?;
                        self.video_stream = Some(WindowsVideoStreamState {
                            max_frames: 60 * 60 * 4,
                            max_decoded_byte_count: 512 * 1024 * 1024,
                            decoder,
                            pending: Some(pending),
                            frame_count: 0,
                            decoded_byte_count: 0,
                            end_emitted: false,
                        });
                        DecodeOutput::VideoStreamStart {
                            duration_estimate_us: Some(duration_us),
                            frame_count: None,
                            decoded_byte_count: None,
                        }
                    }
                    DecodeKind::Audio => {
                        let mut decoder = astra_media::open_windows_audio_stream(
                            &request.bytes,
                            64 * 1024 * 1024,
                        )
                        .map_err(media_decode_error)?;
                        let chunk = decoder
                            .next_chunk()
                            .map_err(media_decode_error)?
                            .ok_or_else(|| {
                                host_error(
                                    "decode.stream.start",
                                    "audio stream produced no samples",
                                )
                            })?;
                        self.audio_stream = Some(decoder);
                        DecodeOutput::AudioPcmI16 {
                            sample_rate: chunk.sample_rate,
                            channels: chunk.channels,
                            samples: chunk.samples,
                        }
                    }
                    DecodeKind::Image => unreachable!(),
                }
            }
            astra_platform::DecodeStreamAction::Next => {
                if !request.bytes.is_empty()
                    || !request.description.is_empty()
                    || request.sample_rate.is_some()
                    || request.channels.is_some()
                    || request.coded_width.is_some()
                    || request.coded_height.is_some()
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "decode.stream.next",
                        "stream next request must not carry payload or metadata",
                    ));
                }
                match request.kind {
                    DecodeKind::Video => next_windows_video_frame(&mut self.video_stream)?,
                    DecodeKind::Audio => next_windows_audio_chunk(&mut self.audio_stream)?,
                    DecodeKind::Image => {
                        return Err(host_error(
                            "decode.stream.next",
                            "image streams are not supported",
                        ));
                    }
                }
            }
        };
        self.next_sequence = next_sequence;
        Ok(output)
    }
}

pub(super) fn next_windows_video_frame(
    stream: &mut Option<WindowsVideoStreamState>,
) -> Result<DecodeOutput, PlatformError> {
    let state = stream
        .as_mut()
        .ok_or_else(|| host_error("decode.stream.next", "video stream has not been started"))?;
    let frame = match state.pending.take() {
        Some(frame) => Some(frame),
        None => state.decoder.next_frame().map_err(media_decode_error)?,
    };
    if let Some(frame) = frame {
        state.frame_count = state
            .frame_count
            .checked_add(1)
            .ok_or_else(|| host_error("decode.stream.next", "video frame count overflowed"))?;
        state.decoded_byte_count = state
            .decoded_byte_count
            .checked_add(frame.bgra8.len() as u64)
            .ok_or_else(|| host_error("decode.stream.next", "decoded byte count overflowed"))?;
        if state.frame_count > state.max_frames
            || state.decoded_byte_count > state.max_decoded_byte_count
        {
            return Err(host_error(
                "decode.stream.next",
                "decoded video stream exceeds its profile-bound budget",
            ));
        }
        // The decoder already owns a validated BGRA8 allocation.  Keep
        // the frame metadata in the small format descriptor and move the
        // pixel Vec directly through the PlatformHost response; encoding
        // the whole frame as postcard here caused a second full-payload
        // allocation on every streaming frame.
        let bytes = frame.bgra8;
        tracing::trace!(
            event = "platform.windows.decode.video_frame.moved",
            sequence = frame.sequence,
            width = frame.width,
            height = frame.height,
            bytes = bytes.len(),
            transfer = "owned_cpu_buffer",
            "WMF streaming frame moved through PlatformHost"
        );
        return Ok(DecodeOutput::VideoFrame {
            sequence: frame.sequence,
            pts_us: frame.pts_us,
            duration_us: frame.duration_us,
            width: frame.width,
            height: frame.height,
            bgra8: bytes,
        });
    }
    if state.end_emitted {
        return Err(host_error(
            "decode.stream.next",
            "video stream end was already emitted",
        ));
    }
    state.end_emitted = true;
    Ok(DecodeOutput::VideoStreamEnd {
        frame_count: state.frame_count,
        decoded_byte_count: state.decoded_byte_count,
    })
}

pub(super) fn next_windows_audio_chunk(
    stream: &mut Option<astra_media::WindowsAudioStreamDecoder>,
) -> Result<DecodeOutput, PlatformError> {
    let decoder = stream
        .as_mut()
        .ok_or_else(|| host_error("decode.stream.next", "audio stream has not been started"))?;
    let chunk = decoder
        .next_chunk()
        .map_err(media_decode_error)?
        .ok_or_else(|| {
            host_error("decode.stream.next", "audio stream reached end of stream").with_field(
                "diagnostic_code",
                astra_platform::DECODE_STREAM_EOS_DIAGNOSTIC,
            )
        })?;
    Ok(DecodeOutput::AudioPcmI16 {
        sample_rate: chunk.sample_rate,
        channels: chunk.channels,
        samples: chunk.samples,
    })
}

pub(super) fn media_decode_error(error: astra_media::MediaError) -> PlatformError {
    match error {
        astra_media::MediaError::Diagnostics(diagnostics) => {
            let diagnostic = diagnostics.into_iter().next();
            let mut error = PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "decode.submit",
                diagnostic
                    .as_ref()
                    .map_or("WMF decode failed", |value| value.message.as_str()),
            );
            if let Some(diagnostic) = diagnostic {
                error = error.with_field("diagnostic_code", diagnostic.code);
            }
            error
        }
        astra_media::MediaError::Message(message) => PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "decode.submit",
            message,
        ),
    }
}

pub(super) fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}

pub(super) fn default_roots() -> Option<super::super::HostRoots> {
    Some(super::super::HostRoots {
        save_base: saved_games_root().ok()?,
        bundle_root: std::env::current_exe().ok()?.parent()?.to_path_buf(),
    })
}

pub(super) fn saved_games_root() -> Result<std::path::PathBuf, PlatformError> {
    use std::ffi::c_void;
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_SavedGames, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_SavedGames, KF_FLAG_DEFAULT, None)
            .map_err(|_| host_error("save.store.open", "Saved Games folder is unavailable"))?;
        let root = path
            .to_string()
            .map_err(|_| host_error("save.store.open", "Saved Games path is invalid"))?;
        CoTaskMemFree(Some(path.as_ptr() as *const c_void));
        Ok(std::path::PathBuf::from(root))
    }
}
