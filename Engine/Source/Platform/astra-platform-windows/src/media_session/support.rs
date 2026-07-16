use astra_media::MediaPlaybackPipeline;
use astra_platform::{PlatformError, PlatformErrorCode, PlatformHostClient, PlatformId};

pub(super) fn performance_error(error: astra_core::PerformanceError) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "media.performance",
        error.to_string(),
    )
}

pub(super) fn duration_us(duration: std::time::Duration) -> Result<u64, PlatformError> {
    u64::try_from(duration.as_micros()).map_err(|_| {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            "media.performance",
            "measured duration exceeds the report range",
        )
    })
}

pub(super) fn initial_buffer_ready(pipeline: &MediaPlaybackPipeline) -> bool {
    let scheduler = pipeline.scheduler();
    (!scheduler.config.has_audio || !scheduler.audio_queue.is_empty())
        && (!scheduler.config.has_video || !scheduler.video_queue.is_empty())
}

pub(super) fn validate_profile(client: &PlatformHostClient) -> Result<(), PlatformError> {
    let profile = client.platform_profile()?;
    if profile.platform != PlatformId::Windows
        || !profile
            .renderer
            .providers
            .iter()
            .any(|id| id == "wgpu_hardware")
        || !profile.audio.providers.iter().any(|id| id == "wasapi")
        || !profile.decode.allow_software
        || !profile.decode.providers.iter().any(|id| id == "ffmpeg")
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "media.open",
            "Windows media session requires explicit wgpu/WASAPI and FFmpeg fallback policy",
        ));
    }
    Ok(())
}

pub(super) fn pcm_s16_to_f32(bytes: &[u8]) -> Result<Vec<f32>, PlatformError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "media.audio_convert",
            "decoded PCM payload is empty or misaligned",
        ));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0)
        .collect())
}

pub(super) fn bgra_to_rgba(bytes: &[u8]) -> Result<Vec<u8>, PlatformError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "media.video_convert",
            "decoded BGRA payload is empty or misaligned",
        ));
    }
    let mut rgba = bytes.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(rgba)
}

pub(super) fn media_error(
    operation: &'static str,
    error: astra_media::MediaError,
) -> PlatformError {
    match error {
        astra_media::MediaError::Diagnostics(diagnostics) => {
            let diagnostic = diagnostics.into_iter().next();
            let mut error = PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                operation,
                diagnostic
                    .as_ref()
                    .map_or("media operation failed", |value| value.message.as_str()),
            );
            if let Some(diagnostic) = diagnostic {
                error = error.with_field("diagnostic_code", diagnostic.code);
            }
            error
        }
        astra_media::MediaError::Message(message) => {
            PlatformError::new(PlatformErrorCode::IntegrityMismatch, operation, message)
        }
    }
}

pub(super) fn invalid_state(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

pub(super) fn media_clock_error(message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, "media.tick", message)
}

pub(super) fn append_cleanup_failure(
    current: &mut Option<PlatformError>,
    mut error: PlatformError,
    code: &'static str,
) {
    if let Some(root) = current {
        root.fields.insert("cleanup".to_string(), code.to_string());
    } else {
        error.fields.insert("cleanup".to_string(), code.to_string());
        *current = Some(error);
    }
}
