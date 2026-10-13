use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_host_profile, HostLimits, PackageSourcePolicy, PlatformError, PlatformErrorCode,
    PlatformHostProfile, PlatformId,
};

pub const HEADLESS_HOST_PROFILE_SCHEMA: &str = "astra.headless_host_profile.v3";
pub const HEADLESS_PRESENTATION_RATE_HZ: u32 = 60;
pub const HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ: u32 = 120;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessReadbackPolicy {
    RasterizedFrames,
    CheckpointsOnly,
}

const fn default_headless_readback_policy() -> HeadlessReadbackPolicy {
    HeadlessReadbackPolicy::RasterizedFrames
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GpuBackendPolicy {
    Dx12,
    Vulkan,
    Metal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GpuDeviceTypePolicy {
    Integrated,
    Discrete,
    AnyHardware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GpuAdapterPolicy {
    pub backend: GpuBackendPolicy,
    pub device_type: GpuDeviceTypePolicy,
    pub require_timestamp_query: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_identity_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessProviderBindings {
    pub renderer: String,
    pub text: String,
    pub audio_mixer: String,
    pub audio_output: String,
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
    #[serde(default = "default_headless_presentation_rate_hz")]
    pub presentation_rate_hz: u32,
    pub frame_format: String,
    pub image_artifact_format: String,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
    pub audio_sample_format: String,
    pub audio_artifact_format: String,
    pub providers: HeadlessProviderBindings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_adapter: Option<GpuAdapterPolicy>,
    pub render_policy: HeadlessRenderPolicy,
    #[serde(default = "default_headless_readback_policy")]
    pub readback_policy: HeadlessReadbackPolicy,
    pub input: HeadlessInputPolicy,
    pub artifacts: HeadlessArtifactPolicy,
    pub package_sources: Vec<PackageSourcePolicy>,
    pub max_package_bytes: u64,
    pub max_decode_output_bytes: u64,
    #[serde(default = "default_decoded_cache_bytes")]
    pub max_decoded_cache_bytes: u64,
    #[serde(default = "default_gpu_resource_bytes")]
    pub max_gpu_resource_bytes: u64,
    pub max_video_frames: u64,
    pub limits: HostLimits,
}

const fn default_decoded_cache_bytes() -> u64 {
    192 * 1024 * 1024
}

const fn default_gpu_resource_bytes() -> u64 {
    256 * 1024 * 1024
}

const fn default_headless_presentation_rate_hz() -> u32 {
    HEADLESS_PRESENTATION_RATE_HZ
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
                audio_mixer: "kira".to_string(),
                audio_output: "headless".to_string(),
                image_decode: "image_cpu".to_string(),
                audio_decode: "symphonia".to_string(),
                video_decode: "disabled".to_string(),
                save: "transactional_file".to_string(),
                package: "verified_bounded".to_string(),
                product_adapter: "astra.native_vn".to_string(),
            },
            gpu_adapter: None,
            render_policy: HeadlessRenderPolicy::Checkpoints,
            readback_policy: HeadlessReadbackPolicy::RasterizedFrames,
            tick_duration_ns: HEADLESS_TICK_DURATION_NS,
            presentation_rate_hz: HEADLESS_PRESENTATION_RATE_HZ,
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
            max_decoded_cache_bytes: 192 * 1024 * 1024,
            max_gpu_resource_bytes: 256 * 1024 * 1024,
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

    /// Hashes only the performance policy. Runtime package, build, and
    /// artifact-session identities are bound by the evidence report and trace
    /// manifest, so they must not invalidate a reusable performance budget.
    pub fn performance_policy_hash(&self) -> Result<String, PlatformError> {
        let mut policy = self.clone();
        policy.package_id.clear();
        policy.build_fingerprint.clear();
        policy.package_hash.clear();
        policy.artifacts.namespace.clear();
        policy.artifacts.required_checkpoints.clear();
        policy.hash()
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

mod validate;
use validate::factory_profile_mismatch;
pub use validate::*;
