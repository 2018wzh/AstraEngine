use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PlatformError, PlatformErrorCode, PlatformId};

pub const PLATFORM_HOST_PROFILE_SCHEMA: &str = "astra.platform_host_profile.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderPolicy {
    pub providers: Vec<String>,
    #[serde(default)]
    pub allow_software: bool,
}

impl ProviderPolicy {
    pub fn required(provider: impl Into<String>) -> Self {
        Self {
            providers: vec![provider.into()],
            allow_software: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackageSourcePolicy {
    Bundled,
    UserAuthorized,
    HttpsRange { allowed_origins: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HostLimits {
    pub command_queue_capacity: usize,
    pub event_queue_capacity: usize,
    pub max_frame_bytes: usize,
    pub max_audio_frames: usize,
    pub max_package_read_bytes: usize,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            command_queue_capacity: 256,
            event_queue_capacity: 1024,
            max_frame_bytes: 64 * 1024 * 1024,
            max_audio_frames: 48_000 * 4,
            max_package_read_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlatformHostProfile {
    pub schema: String,
    pub id: String,
    pub platform: PlatformId,
    pub target: String,
    pub package_id: String,
    pub renderer: ProviderPolicy,
    pub decode: ProviderPolicy,
    pub audio: ProviderPolicy,
    pub save: ProviderPolicy,
    pub package_sources: Vec<PackageSourcePolicy>,
    pub limits: HostLimits,
}

impl PlatformHostProfile {
    pub fn windows_release(target: impl Into<String>, package_id: impl Into<String>) -> Self {
        Self {
            schema: PLATFORM_HOST_PROFILE_SCHEMA.to_string(),
            id: "windows-release".to_string(),
            platform: PlatformId::Windows,
            target: target.into(),
            package_id: package_id.into(),
            renderer: ProviderPolicy::required("wgpu_hardware"),
            decode: ProviderPolicy::required("wmf"),
            audio: ProviderPolicy::required("wasapi"),
            save: ProviderPolicy::required("saved_games"),
            package_sources: vec![
                PackageSourcePolicy::Bundled,
                PackageSourcePolicy::UserAuthorized,
            ],
            limits: HostLimits::default(),
        }
    }

    pub fn web_release(target: impl Into<String>, package_id: impl Into<String>) -> Self {
        Self {
            schema: PLATFORM_HOST_PROFILE_SCHEMA.to_string(),
            id: "web-release-chrome".to_string(),
            platform: PlatformId::Web,
            target: target.into(),
            package_id: package_id.into(),
            renderer: ProviderPolicy::required("webgpu"),
            decode: ProviderPolicy::required("webcodecs"),
            audio: ProviderPolicy::required("webaudio"),
            save: ProviderPolicy::required("opfs"),
            package_sources: vec![
                PackageSourcePolicy::Bundled,
                PackageSourcePolicy::UserAuthorized,
            ],
            limits: HostLimits::default(),
        }
    }

    pub fn hash(&self) -> Result<String, PlatformError> {
        let bytes = serde_json::to_vec(self).map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.hash",
                "platform profile could not be encoded",
            )
            .with_field("serde_error", error.to_string())
        })?;
        Ok(astra_core::Hash256::from_sha256(&bytes).to_string())
    }
}

pub fn validate_host_profile(profile: &PlatformHostProfile) -> Result<(), PlatformError> {
    if profile.schema != PLATFORM_HOST_PROFILE_SCHEMA {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "profile.validate",
            "platform host profile schema is unsupported",
        ));
    }
    for (field, value) in [
        ("id", profile.id.as_str()),
        ("target", profile.target.as_str()),
        ("package_id", profile.package_id.as_str()),
    ] {
        if !is_safe_identifier(value) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.validate",
                "platform profile identity is unsafe",
            )
            .with_field("field", field));
        }
    }
    for (field, policy) in [
        ("renderer", &profile.renderer),
        ("decode", &profile.decode),
        ("audio", &profile.audio),
        ("save", &profile.save),
    ] {
        if policy.providers.is_empty() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.validate",
                "provider policy must declare at least one provider",
            )
            .with_field("field", field));
        }
        let mut unique = BTreeSet::new();
        if policy
            .providers
            .iter()
            .any(|provider| !is_safe_identifier(provider) || !unique.insert(provider))
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.validate",
                "provider policy contains an unsafe or duplicate provider",
            )
            .with_field("field", field));
        }
    }
    if profile.limits.command_queue_capacity == 0
        || profile.limits.event_queue_capacity == 0
        || profile.limits.max_frame_bytes == 0
        || profile.limits.max_audio_frames == 0
        || profile.limits.max_package_read_bytes == 0
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "profile.validate",
            "platform host limits must be non-zero",
        ));
    }
    Ok(())
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}
