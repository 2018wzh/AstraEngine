use super::*;
use crate::{HostLimits, PackageSourcePolicy, PlatformError, PlatformErrorCode};

pub fn validate_headless_host_profile(profile: &HeadlessHostProfile) -> Result<(), PlatformError> {
    if profile.schema != HEADLESS_HOST_PROFILE_SCHEMA
        || profile.input.schema != HEADLESS_INPUT_POLICY_SCHEMA
        || profile.input.protocol_schema != USER_INPUT_SEQUENCE_SCHEMA
    {
        return Err(invalid_headless_profile(
            "headless profile or input schema is unsupported",
        ));
    }
    if profile.tick_duration_ns != HEADLESS_TICK_DURATION_NS
        || !matches!(
            profile.presentation_rate_hz,
            HEADLESS_PRESENTATION_RATE_HZ | HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ
        )
        || profile.frame_format != "rgba8_srgb"
        || profile.image_artifact_format != "png"
        || profile.audio_sample_rate != HEADLESS_AUDIO_SAMPLE_RATE
        || profile.audio_channels != HEADLESS_AUDIO_CHANNELS
        || profile.audio_sample_format != "pcm_s16le"
        || profile.audio_artifact_format != "wav"
    {
        return Err(invalid_headless_profile(
            "headless canonical media or time format was changed",
        ));
    }
    let viewport_bytes = usize::try_from(profile.viewport_width)
        .ok()
        .and_then(|width| {
            usize::try_from(profile.viewport_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if profile.viewport_width == 0
        || profile.viewport_height == 0
        || viewport_bytes.is_none_or(|bytes| bytes > profile.limits.max_frame_bytes)
    {
        return Err(invalid_headless_profile(
            "headless viewport is invalid or exceeds frame limits",
        ));
    }
    for (field, value) in [
        ("id", profile.id.as_str()),
        ("target", profile.target.as_str()),
        ("product_profile", profile.product_profile.as_str()),
        ("package_id", profile.package_id.as_str()),
        ("artifact_namespace", profile.artifacts.namespace.as_str()),
    ] {
        if !is_safe_identifier(value) {
            return Err(
                invalid_headless_profile("headless profile identity is unsafe")
                    .with_field("field", field),
            );
        }
    }
    for (field, value) in [
        ("build_fingerprint", profile.build_fingerprint.as_str()),
        ("package_hash", profile.package_hash.as_str()),
    ] {
        if !is_sha256(value) {
            return Err(invalid_headless_profile(
                "headless profile identity requires a full sha256 value",
            )
            .with_field("field", field));
        }
    }
    let providers = [
        ("renderer", profile.providers.renderer.as_str()),
        ("text", profile.providers.text.as_str()),
        ("audio_mixer", profile.providers.audio_mixer.as_str()),
        ("audio_output", profile.providers.audio_output.as_str()),
        ("image_decode", profile.providers.image_decode.as_str()),
        ("audio_decode", profile.providers.audio_decode.as_str()),
        ("video_decode", profile.providers.video_decode.as_str()),
        ("save", profile.providers.save.as_str()),
        ("package", profile.providers.package.as_str()),
        (
            "product_adapter",
            profile.providers.product_adapter.as_str(),
        ),
    ];
    for (field, provider) in providers {
        if !is_safe_identifier(provider) {
            return Err(
                invalid_headless_profile("headless provider binding is unsafe or missing")
                    .with_field("field", field),
            );
        }
    }
    if profile.presentation_rate_hz == HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ
        && (profile.schema != HEADLESS_HOST_PROFILE_SCHEMA
            || profile.providers.renderer != "wgpu_offscreen"
            || profile.render_policy != HeadlessRenderPolicy::All
            || profile.readback_policy != HeadlessReadbackPolicy::CheckpointsOnly
            || !profile
                .gpu_adapter
                .as_ref()
                .is_some_and(|policy| policy.require_timestamp_query))
    {
        return Err(invalid_headless_profile(
            "120 Hz presentation is reserved for timestamped headless performance E2",
        ));
    }
    if let Some(policy) = &profile.gpu_adapter {
        if policy
            .adapter_identity_hash
            .as_deref()
            .is_some_and(|value| !is_sha256(value))
        {
            return Err(invalid_headless_profile(
                "headless GPU adapter identity must be a full sha256 value",
            ));
        }
    }
    if profile.input.max_messages == 0
        || profile.input.max_tick == 0
        || profile.input.transports.is_empty()
    {
        return Err(invalid_headless_profile(
            "headless input policy has no bounded transport",
        ));
    }
    let artifacts = &profile.artifacts;
    if artifacts.max_artifacts == 0
        || artifacts.max_total_bytes == 0
        || artifacts.max_submitted_frames == 0
        || artifacts.max_rasterized_frames == 0
        || artifacts.max_audio_frames == 0
        || artifacts.max_duration_ns == 0
        || artifacts.max_artifacts < artifacts.required_checkpoints.len() as u64
    {
        return Err(invalid_headless_profile(
            "headless artifact limits are invalid",
        ));
    }
    let mut checkpoints = BTreeSet::new();
    if artifacts
        .required_checkpoints
        .iter()
        .any(|checkpoint| !is_safe_identifier(checkpoint) || !checkpoints.insert(checkpoint))
    {
        return Err(invalid_headless_profile(
            "headless required checkpoints are unsafe or duplicated",
        ));
    }
    if profile.package_sources.is_empty()
        || profile.max_package_bytes == 0
        || profile.max_decode_output_bytes == 0
        || profile.max_decoded_cache_bytes == 0
        || profile.max_decoded_cache_bytes > profile.max_decode_output_bytes
        || profile.max_gpu_resource_bytes == 0
        || profile.max_video_frames == 0
        || profile.max_package_bytes < profile.limits.max_package_read_bytes as u64
    {
        return Err(invalid_headless_profile(
            "headless profile package source or package byte limit is invalid",
        ));
    }
    validate_headless_package_sources(&profile.package_sources)?;
    validate_host_limits(&profile.limits)
}

pub fn validate_headless_performance_profile(
    profile: &HeadlessHostProfile,
) -> Result<&GpuAdapterPolicy, PlatformError> {
    validate_headless_host_profile(profile)?;
    let policy = profile.gpu_adapter.as_ref().ok_or_else(|| {
        invalid_headless_profile("performance E2 requires an explicit GPU adapter policy")
    })?;
    if profile.schema != HEADLESS_HOST_PROFILE_SCHEMA
        || profile.providers.renderer != "wgpu_offscreen"
        || profile.presentation_rate_hz != HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ
        || !policy.require_timestamp_query
        || profile.render_policy != HeadlessRenderPolicy::All
        || profile.readback_policy != HeadlessReadbackPolicy::CheckpointsOnly
        || profile.artifacts.retention != HeadlessArtifactRetention::ManifestOnly
        || profile.max_decoded_cache_bytes > 192 * 1024 * 1024
        || profile.max_gpu_resource_bytes > 256 * 1024 * 1024
    {
        return Err(invalid_headless_profile(
            "performance E2 requires headless v3, 120 Hz presentation, all-frame GPU submission, checkpoint-only readback, manifest-only artifact streaming, and timestamp queries",
        ));
    }
    Ok(policy)
}

fn validate_headless_package_sources(
    package_sources: &[PackageSourcePolicy],
) -> Result<(), PlatformError> {
    let mut kinds = BTreeSet::new();
    for source in package_sources {
        let kind = match source {
            PackageSourcePolicy::Bundled => "bundled",
            PackageSourcePolicy::UserAuthorized => "user_authorized",
            PackageSourcePolicy::HttpsRange { allowed_origins } => {
                if allowed_origins.is_empty() {
                    return Err(invalid_headless_profile(
                        "headless HTTPS package source requires an allowlist",
                    ));
                }
                for origin in allowed_origins {
                    let parsed = url::Url::parse(origin).map_err(|_| {
                        invalid_headless_profile("headless HTTPS package source origin is invalid")
                    })?;
                    if parsed.scheme() != "https"
                        || parsed.host_str().is_none()
                        || parsed.origin().ascii_serialization() != origin.trim_end_matches('/')
                    {
                        return Err(invalid_headless_profile(
                            "headless HTTPS package source origin is invalid",
                        ));
                    }
                }
                "https_range"
            }
        };
        if !kinds.insert(kind) {
            return Err(invalid_headless_profile(
                "headless package source kind is duplicated",
            ));
        }
    }
    Ok(())
}

fn validate_host_limits(limits: &HostLimits) -> Result<(), PlatformError> {
    if limits.command_queue_capacity == 0
        || limits.event_queue_capacity == 0
        || limits.max_frame_bytes == 0
        || limits.max_audio_frames == 0
        || limits.max_package_read_bytes == 0
    {
        return Err(invalid_headless_profile(
            "headless host limits must be non-zero",
        ));
    }
    Ok(())
}

pub(super) fn factory_profile_mismatch(message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidProfile, "host.start", message)
}

fn invalid_headless_profile(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidProfile,
        "headless.profile.validate",
        message,
    )
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn is_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
