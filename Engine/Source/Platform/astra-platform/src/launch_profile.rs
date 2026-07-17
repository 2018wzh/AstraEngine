use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_host_profile, HostLimits, PackageSourcePolicy, PlatformError, PlatformErrorCode,
    PlatformHostProfile, PlatformId,
};

pub const HEADLESS_HOST_PROFILE_SCHEMA: &str = "astra.headless_host_profile.v2";
pub const HEADLESS_INPUT_POLICY_SCHEMA: &str = "astra.headless_input_policy.v1";
pub const USER_INPUT_SEQUENCE_SCHEMA: &str = "astra.user_input_sequence.v1";
pub const HEADLESS_PROTOCOL_SCHEMA: &str = "astra.headless_protocol.v1";
pub const HEADLESS_TICK_DURATION_NS: u64 = 16_666_667;
pub const HEADLESS_AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const HEADLESS_AUDIO_CHANNELS: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Platform(PlatformId),
    Headless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessArtifactRetention {
    All,
    Checkpoints,
    Final,
    ManifestOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessRenderPolicy {
    All,
    Checkpoints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessProviderBindings {
    pub renderer: String,
    pub text: String,
    pub audio_mixer: String,
    pub image_decode: String,
    pub audio_decode: String,
    pub video_decode: String,
    pub save: String,
    pub package: String,
    pub product_adapter: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessInputPolicy {
    pub schema: String,
    pub protocol_schema: String,
    pub max_messages: u64,
    pub max_tick: u64,
    pub transports: BTreeSet<HeadlessTransport>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessTransport {
    File,
    Stdio,
    RealtimeCli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessArtifactPolicy {
    pub namespace: String,
    pub retention: HeadlessArtifactRetention,
    pub max_artifacts: u64,
    pub max_total_bytes: u64,
    pub max_submitted_frames: u64,
    pub max_rasterized_frames: u64,
    pub max_audio_frames: u64,
    pub max_duration_ns: u64,
    pub required_checkpoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessHostProfile {
    pub schema: String,
    pub id: String,
    pub target: String,
    pub product_profile: String,
    pub package_id: String,
    pub build_fingerprint: String,
    pub package_hash: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub tick_duration_ns: u64,
    pub frame_format: String,
    pub image_artifact_format: String,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
    pub audio_sample_format: String,
    pub audio_artifact_format: String,
    pub providers: HeadlessProviderBindings,
    pub render_policy: HeadlessRenderPolicy,
    pub input: HeadlessInputPolicy,
    pub artifacts: HeadlessArtifactPolicy,
    pub package_sources: Vec<PackageSourcePolicy>,
    pub max_package_bytes: u64,
    pub max_decode_output_bytes: u64,
    pub max_video_frames: u64,
    pub limits: HostLimits,
}

impl HeadlessHostProfile {
    pub fn reference(
        target: impl Into<String>,
        package_id: impl Into<String>,
        build_fingerprint: impl Into<String>,
        package_hash: impl Into<String>,
    ) -> Self {
        Self {
            schema: HEADLESS_HOST_PROFILE_SCHEMA.to_string(),
            id: "headless-reference".to_string(),
            target: target.into(),
            product_profile: "classic".to_string(),
            package_id: package_id.into(),
            build_fingerprint: build_fingerprint.into(),
            package_hash: package_hash.into(),
            viewport_width: 1280,
            viewport_height: 720,
            providers: HeadlessProviderBindings {
                renderer: "cpu_reference".to_string(),
                text: "cosmic_text_cpu".to_string(),
                audio_mixer: "audio_graph_cpu".to_string(),
                image_decode: "image_cpu".to_string(),
                audio_decode: "symphonia".to_string(),
                video_decode: "disabled".to_string(),
                save: "transactional_file".to_string(),
                package: "verified_bounded".to_string(),
                product_adapter: "astra.native_vn".to_string(),
            },
            render_policy: HeadlessRenderPolicy::Checkpoints,
            tick_duration_ns: HEADLESS_TICK_DURATION_NS,
            frame_format: "rgba8_srgb".to_string(),
            image_artifact_format: "png".to_string(),
            audio_sample_rate: HEADLESS_AUDIO_SAMPLE_RATE,
            audio_channels: HEADLESS_AUDIO_CHANNELS,
            audio_sample_format: "pcm_s16le".to_string(),
            audio_artifact_format: "wav".to_string(),
            input: HeadlessInputPolicy {
                schema: HEADLESS_INPUT_POLICY_SCHEMA.to_string(),
                protocol_schema: USER_INPUT_SEQUENCE_SCHEMA.to_string(),
                max_messages: 100_000,
                max_tick: 10_000_000,
                transports: [HeadlessTransport::File, HeadlessTransport::Stdio]
                    .into_iter()
                    .collect(),
            },
            artifacts: HeadlessArtifactPolicy {
                namespace: "headless-run".to_string(),
                retention: HeadlessArtifactRetention::All,
                max_artifacts: 100_000,
                max_total_bytes: 8 * 1024 * 1024 * 1024,
                max_submitted_frames: 100_000,
                max_rasterized_frames: 100_000,
                max_audio_frames: 48_000 * 60 * 60,
                max_duration_ns: 60 * 60 * 1_000_000_000,
                required_checkpoints: Vec::new(),
            },
            package_sources: vec![PackageSourcePolicy::Bundled],
            max_package_bytes: 8 * 1024 * 1024 * 1024,
            max_decode_output_bytes: 512 * 1024 * 1024,
            max_video_frames: 18_000,
            limits: HostLimits::default(),
        }
    }

    pub fn hash(&self) -> Result<String, PlatformError> {
        let bytes = serde_json::to_vec(self).map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.hash",
                "headless profile could not be encoded",
            )
            .with_field("serde_error", error.to_string())
        })?;
        Ok(astra_core::Hash256::from_sha256(&bytes).to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "profile", rename_all = "snake_case")]
pub enum HostLaunchProfile {
    Platform(Box<PlatformHostProfile>),
    Headless(Box<HeadlessHostProfile>),
}

impl From<PlatformHostProfile> for HostLaunchProfile {
    fn from(profile: PlatformHostProfile) -> Self {
        Self::platform(profile)
    }
}

impl From<HeadlessHostProfile> for HostLaunchProfile {
    fn from(profile: HeadlessHostProfile) -> Self {
        Self::headless(profile)
    }
}

impl HostLaunchProfile {
    pub fn platform(profile: PlatformHostProfile) -> Self {
        Self::Platform(Box::new(profile))
    }

    pub fn headless(profile: HeadlessHostProfile) -> Self {
        Self::Headless(Box::new(profile))
    }

    pub fn kind(&self) -> HostKind {
        match self {
            Self::Platform(profile) => HostKind::Platform(profile.platform),
            Self::Headless(_) => HostKind::Headless,
        }
    }

    pub fn validate(&self) -> Result<(), PlatformError> {
        match self {
            Self::Platform(profile) => validate_host_profile(profile),
            Self::Headless(profile) => validate_headless_host_profile(profile),
        }
    }

    pub fn limits(&self) -> &HostLimits {
        match self {
            Self::Platform(profile) => &profile.limits,
            Self::Headless(profile) => &profile.limits,
        }
    }

    pub fn package_sources(&self) -> &[PackageSourcePolicy] {
        match self {
            Self::Platform(profile) => &profile.package_sources,
            Self::Headless(profile) => &profile.package_sources,
        }
    }

    pub fn target(&self) -> &str {
        match self {
            Self::Platform(profile) => &profile.target,
            Self::Headless(profile) => &profile.target,
        }
    }

    pub fn package_id(&self) -> &str {
        match self {
            Self::Platform(profile) => &profile.package_id,
            Self::Headless(profile) => &profile.package_id,
        }
    }

    pub fn hash(&self) -> Result<String, PlatformError> {
        match self {
            Self::Platform(profile) => profile.hash(),
            Self::Headless(profile) => profile.hash(),
        }
    }

    pub fn require_platform(&self) -> Result<&PlatformHostProfile, PlatformError> {
        match self {
            Self::Platform(profile) => Ok(profile),
            Self::Headless(_) => Err(factory_profile_mismatch(
                "native platform factory rejects Headless launch profiles",
            )),
        }
    }

    pub fn require_headless(&self) -> Result<&HeadlessHostProfile, PlatformError> {
        match self {
            Self::Headless(profile) => Ok(profile),
            Self::Platform(_) => Err(factory_profile_mismatch(
                "Headless factory rejects native platform launch profiles",
            )),
        }
    }
}

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

fn factory_profile_mismatch(message: &'static str) -> PlatformError {
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
