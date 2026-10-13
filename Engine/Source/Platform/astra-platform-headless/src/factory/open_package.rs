use super::*;

impl HostState {
    pub(super) async fn open_package(
        &mut self,
        source: PackageSourceRequest,
    ) -> Result<PackageSourceHandle, PlatformError> {
        let state = match source {
            PackageSourceRequest::Bundled {
                relative_path,
                expected_hash,
            } => {
                let path = safe_join(&self.package_root, &relative_path)?;
                PackageState::File(FilePackageSource::open(path, &expected_hash)?)
            }
            PackageSourceRequest::UserAuthorized { expected_hash } => {
                let path = self.user_authorized_package.as_ref().ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::Cancelled,
                        "package.open_user_authorized",
                        "no user-authorized package was supplied to the test harness",
                    )
                })?;
                PackageState::File(FilePackageSource::open(path, &expected_hash)?)
            }
            PackageSourceRequest::HttpsRange { url, expected_hash } => {
                let parsed = url::Url::parse(&url).map_err(|_| {
                    PlatformError::new(
                        PlatformErrorCode::PermissionDenied,
                        "package.open_https",
                        "HTTPS package URL is invalid",
                    )
                })?;
                if !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::PermissionDenied,
                        "package.open_https",
                        "HTTPS package URL must not contain credentials or a fragment",
                    ));
                }
                let origin = parsed.origin().ascii_serialization();
                let allowed = self.profile.package_sources.iter().any(|policy| {
                    matches!(policy, astra_platform::PackageSourcePolicy::HttpsRange { allowed_origins } if allowed_origins.iter().any(|allowed| allowed == &origin))
                });
                if parsed.scheme() != "https" || !allowed {
                    return Err(PlatformError::new(
                        PlatformErrorCode::PermissionDenied,
                        "package.open_https",
                        "HTTPS package origin is not allowed by the profile",
                    ));
                }
                open_https_range(
                    parsed,
                    &expected_hash,
                    self.profile.max_package_bytes,
                    self.profile.limits.max_package_read_bytes,
                    &self.https_root_certificates,
                )
                .await?
            }
        };
        self.packages.insert(state)
    }

    pub(super) fn ensure_empty(&self) -> Result<(), PlatformError> {
        self.surfaces
            .ensure_empty()
            .and_then(|_| self.windows.ensure_empty())
            .and_then(|_| self.audio.ensure_empty())
            .and_then(|_| self.decoders.ensure_empty())
            .and_then(|_| self.saves.ensure_empty())
            .and_then(|_| self.packages.ensure_empty())
    }

    pub(super) fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
            && self.windows.is_empty()
            && self.audio.is_empty()
            && self.decoders.is_empty()
            && self.saves.is_empty()
            && self.packages.is_empty()
    }
}

pub(super) fn present_rgba(
    surface: &mut SurfaceState,
    frame: RgbaFrame,
) -> Result<(), PlatformError> {
    ensure_increasing(
        surface.last_sequence,
        frame.sequence,
        "surface.present_rgba",
    )?;
    if frame.width != surface.width || frame.height != surface.height {
        return Err(invalid(
            "surface.present_rgba",
            "frame dimensions do not match surface",
        ));
    }
    surface.frame = Some(frame.rgba8.into());
    surface.pending = None;
    surface.materialized_sequence = Some(frame.sequence);
    surface.last_sequence = frame.sequence;
    Ok(())
}

pub(super) fn ensure_increasing(
    last: u64,
    next: u64,
    operation: &'static str,
) -> Result<(), PlatformError> {
    if next == 0 || next <= last {
        return Err(invalid(
            operation,
            "sequence is zero, duplicated, or reversed",
        ));
    }
    Ok(())
}

pub(super) fn decode_session(
    state: &mut DecodeState,
    request: astra_platform::PlatformDecodeRequest,
    video_binding: &str,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
    max_frame_bytes: usize,
) -> Result<DecodeOutput, PlatformError> {
    if request.sequence == 0 || request.sequence <= state.last_sequence {
        return Err(invalid(
            "decode.submit",
            "decode request sequence must be strictly increasing within a session",
        ));
    }
    if state.kind != request.kind {
        return Err(invalid(
            "decode.submit",
            "decode request kind does not match session",
        ));
    }
    let sequence = request.sequence;
    let action = request.stream_action;
    let output = match action {
        astra_platform::DecodeStreamAction::OneShot => {
            if request.bytes.is_empty() {
                return Err(invalid(
                    "decode.submit",
                    "one-shot decode requires non-empty encoded bytes",
                ));
            }
            #[cfg(feature = "ffmpeg-vcpkg")]
            if state.video_stream.is_some() {
                return Err(invalid(
                    "decode.submit",
                    "one-shot decode is invalid while a video stream is active",
                ));
            }
            decode(state.kind, request)
        }
        astra_platform::DecodeStreamAction::Start => {
            if state.kind != DecodeKind::Video || request.bytes.is_empty() {
                return Err(invalid(
                    "decode.video.stream.start",
                    "video stream start requires a video session and encoded bytes",
                ));
            }
            #[cfg(feature = "ffmpeg-vcpkg")]
            {
                if state.video_stream.is_some() {
                    return Err(invalid(
                        "decode.video.stream.start",
                        "video stream is already active",
                    ));
                }
                let stream = open_headless_video_stream(
                    &request.codec,
                    &request.bytes,
                    video_binding,
                    max_video_frames,
                    max_decode_output_bytes,
                    max_frame_bytes,
                )?;
                let output = DecodeOutput::VideoStreamStart {
                    duration_estimate_us: Some(stream.duration_us),
                    frame_count: None,
                    decoded_byte_count: None,
                };
                state.video_stream = Some(stream);
                Ok(output)
            }
            #[cfg(not(feature = "ffmpeg-vcpkg"))]
            {
                let _ = (
                    video_binding,
                    max_video_frames,
                    max_decode_output_bytes,
                    max_frame_bytes,
                );
                Err(PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "decode.video.stream.start",
                    "video streaming requires an explicitly compiled ffmpeg-vcpkg provider",
                ))
            }
        }
        astra_platform::DecodeStreamAction::Next => {
            if state.kind != DecodeKind::Video || !request.bytes.is_empty() {
                return Err(invalid(
                    "decode.video.stream.next",
                    "video stream next requires a video session and empty bytes",
                ));
            }
            #[cfg(feature = "ffmpeg-vcpkg")]
            {
                next_headless_video_output(state)
            }
            #[cfg(not(feature = "ffmpeg-vcpkg"))]
            {
                let _ = max_decode_output_bytes;
                Err(PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "decode.video.stream.next",
                    "video streaming requires an explicitly compiled ffmpeg-vcpkg provider",
                ))
            }
        }
    }?;
    state.last_sequence = sequence;
    Ok(output)
}

pub(super) fn decode(
    kind: DecodeKind,
    request: astra_platform::PlatformDecodeRequest,
) -> Result<DecodeOutput, PlatformError> {
    if kind != request.kind {
        return Err(invalid(
            "decode.submit",
            "decode request kind does not match session",
        ));
    }
    let media_kind = match kind {
        DecodeKind::Image => MediaDecodeKind::Image,
        DecodeKind::Audio => MediaDecodeKind::Audio,
        DecodeKind::Video => MediaDecodeKind::Video,
    };
    let request = DecodeRequest {
        kind: media_kind,
        codec: request.codec,
        bytes: request.bytes,
        profile: "headless".into(),
    };
    if kind == DecodeKind::Video {
        return Err(invalid(
            "decode.video",
            "video decode requires the typed incremental stream path",
        ));
    }
    let result = match kind {
        DecodeKind::Image => ImageDecodeProvider.decode(&request),
        DecodeKind::Audio => SymphoniaAudioDecodeProvider.decode(&request),
        DecodeKind::Video => unreachable!(),
    }
    .map_err(media_error)?;
    match result.output {
        MediaDecodeOutput::CpuBuffer { bytes, format } => {
            Ok(DecodeOutput::CpuBuffer { format, bytes })
        }
        MediaDecodeOutput::AudioPcmI16 {
            sample_rate,
            channels,
            samples,
        } => Ok(DecodeOutput::AudioPcmI16 {
            sample_rate,
            channels,
            samples,
        }),
        MediaDecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        } => Ok(DecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        }),
        MediaDecodeOutput::MediaSurfaceToken(_) => Err(invalid(
            "decode.submit",
            "headless decode cannot return a native media token",
        )),
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
pub(super) fn open_headless_video_stream(
    codec: &str,
    encoded: &[u8],
    video_binding: &str,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
    max_frame_bytes: usize,
) -> Result<HeadlessVideoStream, PlatformError> {
    if video_binding != "ffmpeg-vcpkg" {
        return Err(PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "decode.video.stream.start",
            "video decode requires the explicit ffmpeg-vcpkg profile binding",
        ));
    }
    astra_media::probe_ffmpeg_provider().map_err(media_error)?;
    let max_video_frames_usize = usize::try_from(max_video_frames).map_err(|_| {
        invalid(
            "decode.video.stream.start",
            "video frame limit exceeds the current host address space",
        )
    })?;
    let max_decode_output_bytes_usize = usize::try_from(max_decode_output_bytes).map_err(|_| {
        invalid(
            "decode.video.stream.start",
            "video byte limit exceeds the current host address space",
        )
    })?;
    let limits = astra_media::FfmpegStreamLimits {
        max_encoded_bytes: encoded.len(),
        max_video_frames: max_video_frames_usize,
        max_video_frame_bytes: max_frame_bytes.min(max_decode_output_bytes_usize),
        ..astra_media::FfmpegStreamLimits::default()
    };
    let decoder =
        astra_media::FfmpegPlaybackDecoder::open(codec, encoded, limits).map_err(media_error)?;
    let duration_us = decoder.playback_config().duration_us;
    if duration_us == 0 {
        return Err(invalid(
            "decode.video.stream.start",
            "video decoder reported an empty duration",
        ));
    }
    Ok(HeadlessVideoStream {
        decoder,
        duration_us,
        max_frames: max_video_frames,
        max_decoded_byte_count: max_decode_output_bytes,
        frame_count: 0,
        decoded_byte_count: 0,
        end_emitted: false,
    })
}

#[cfg(feature = "ffmpeg-vcpkg")]
pub(super) fn next_headless_video_output(
    state: &mut DecodeState,
) -> Result<DecodeOutput, PlatformError> {
    let stream = state.video_stream.as_mut().ok_or_else(|| {
        invalid(
            "decode.video.stream.next",
            "video stream has not been started",
        )
    })?;
    while let Some(packet) = stream.decoder.read_next().map_err(media_error)? {
        let FfmpegDecodedPacket::Video { packet, bgra8 } = packet else {
            continue;
        };
        let expected = u64::from(packet.width)
            .checked_mul(u64::from(packet.height))
            .and_then(|pixels| pixels.checked_mul(4));
        if packet.sequence != stream.frame_count.saturating_add(1)
            || packet.duration_us == 0
            || expected != Some(bgra8.len() as u64)
        {
            return Err(invalid(
                "decode.video.stream.next",
                "decoded video frame metadata is invalid",
            ));
        }
        stream.frame_count = packet.sequence;
        stream.decoded_byte_count = stream
            .decoded_byte_count
            .checked_add(bgra8.len() as u64)
            .ok_or_else(|| {
                invalid(
                    "decode.video.stream.next",
                    "decoded video byte accounting overflowed",
                )
            })?;
        if stream.frame_count > stream.max_frames
            || stream.decoded_byte_count > stream.max_decoded_byte_count
        {
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "decode.video.stream.next",
                "decoded video stream exceeds its profile-bound budget",
            ));
        }
        return Ok(DecodeOutput::VideoFrame {
            sequence: packet.sequence,
            pts_us: packet.pts_us,
            duration_us: packet.duration_us,
            width: packet.width,
            height: packet.height,
            bgra8,
        });
    }
    if stream.end_emitted {
        return Err(invalid(
            "decode.video.stream.next",
            "video stream end was already emitted",
        ));
    }
    stream.end_emitted = true;
    Ok(DecodeOutput::VideoStreamEnd {
        frame_count: stream.frame_count,
        decoded_byte_count: stream.decoded_byte_count,
    })
}
