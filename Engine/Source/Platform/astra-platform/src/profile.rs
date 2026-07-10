use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PlatformError, PlatformErrorCode, PlatformId};

pub const PLATFORM_HOST_PROFILE_SCHEMA_V1: &str = "astra.platform_host_profile.v1";
pub const PLATFORM_HOST_PROFILE_SCHEMA: &str = "astra.platform_host_profile.v2";
pub const DEFAULT_MAX_PACKAGE_CACHE_ENTRY_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_PACKAGE_CACHE_TOTAL_BYTES: u64 = 64 * 1024 * 1024 * 1024;

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
pub struct PackageCachePolicy {
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for PackageCachePolicy {
    fn default() -> Self {
        Self {
            max_entry_bytes: DEFAULT_MAX_PACKAGE_CACHE_ENTRY_BYTES,
            max_total_bytes: DEFAULT_MAX_PACKAGE_CACHE_TOTAL_BYTES,
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
    pub package_cache: PackageCachePolicy,
}

#[derive(Debug, Deserialize)]
struct PlatformHostProfileV1 {
    schema: String,
    id: String,
    platform: PlatformId,
    target: String,
    package_id: String,
    renderer: ProviderPolicy,
    decode: ProviderPolicy,
    audio: ProviderPolicy,
    save: ProviderPolicy,
    package_sources: Vec<PackageSourcePolicy>,
    limits: HostLimits,
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
            package_cache: PackageCachePolicy::default(),
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
            package_cache: PackageCachePolicy::default(),
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

pub fn migrate_host_profile_json(
    value: serde_json::Value,
) -> Result<PlatformHostProfile, PlatformError> {
    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_profile_migration("profile schema is missing"))?;
    match schema {
        PLATFORM_HOST_PROFILE_SCHEMA => serde_json::from_value(value).map_err(|_| {
            invalid_profile_migration("v2 platform host profile could not be decoded")
        }),
        PLATFORM_HOST_PROFILE_SCHEMA_V1 => {
            let profile: PlatformHostProfileV1 = serde_json::from_value(value).map_err(|_| {
                invalid_profile_migration("v1 platform host profile could not be decoded")
            })?;
            if profile.schema != PLATFORM_HOST_PROFILE_SCHEMA_V1 {
                return Err(invalid_profile_migration(
                    "v1 platform host profile schema is invalid",
                ));
            }
            Ok(PlatformHostProfile {
                schema: PLATFORM_HOST_PROFILE_SCHEMA.to_string(),
                id: profile.id,
                platform: profile.platform,
                target: profile.target,
                package_id: profile.package_id,
                renderer: profile.renderer,
                decode: profile.decode,
                audio: profile.audio,
                save: profile.save,
                package_sources: profile.package_sources,
                limits: profile.limits,
                package_cache: PackageCachePolicy::default(),
            })
        }
        _ => Err(invalid_profile_migration(
            "platform host profile schema is unsupported",
        )),
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
    validate_release_provider_policy(profile)?;
    let mut source_kinds = BTreeSet::new();
    let mut origins = BTreeSet::new();
    for source in &profile.package_sources {
        let kind = match source {
            PackageSourcePolicy::Bundled => "bundled",
            PackageSourcePolicy::UserAuthorized => "user_authorized",
            PackageSourcePolicy::HttpsRange { allowed_origins } => {
                if allowed_origins.is_empty() {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidProfile,
                        "profile.validate",
                        "HTTPS range policy requires an origin allowlist",
                    ));
                }
                for origin in allowed_origins {
                    let parsed = url::Url::parse(origin).map_err(|_| invalid_origin())?;
                    if parsed.scheme() != "https"
                        || parsed.host_str().is_none()
                        || !parsed.username().is_empty()
                        || parsed.password().is_some()
                        || !matches!(parsed.path(), "" | "/")
                        || parsed.query().is_some()
                        || parsed.fragment().is_some()
                        || parsed.origin().ascii_serialization() != origin.trim_end_matches('/')
                        || !origins.insert(origin.trim_end_matches('/').to_string())
                    {
                        return Err(invalid_origin());
                    }
                }
                "https_range"
            }
        };
        if !source_kinds.insert(kind) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.validate",
                "package source policy contains a duplicate source kind",
            ));
        }
    }
    if !source_kinds.contains("bundled") {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "profile.validate",
            "release platform profile must allow bundled package sources",
        ));
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
    if profile.package_cache.max_entry_bytes == 0
        || profile.package_cache.max_total_bytes == 0
        || profile.package_cache.max_entry_bytes > profile.package_cache.max_total_bytes
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "profile.validate",
            "package cache limits are invalid",
        ));
    }
    Ok(())
}

fn invalid_profile_migration(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidProfile,
        "profile.migrate",
        message,
    )
}

fn validate_release_provider_policy(profile: &PlatformHostProfile) -> Result<(), PlatformError> {
    let expected = match profile.platform {
        PlatformId::Windows => ["wgpu_hardware", "wmf", "wasapi", "saved_games"],
        PlatformId::Web => ["webgpu", "webcodecs", "webaudio", "opfs"],
        PlatformId::Linux | PlatformId::Macos | PlatformId::Ios | PlatformId::Android => {
            return Ok(())
        }
    };
    for ((field, policy), required) in [
        ("renderer", &profile.renderer),
        ("decode", &profile.decode),
        ("audio", &profile.audio),
        ("save", &profile.save),
    ]
    .into_iter()
    .zip(expected)
    {
        if policy.allow_software || policy.providers.as_slice() != [required] {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "profile.validate",
                "release provider policy declares an unsupported fallback",
            )
            .with_field("field", field));
        }
    }
    if profile.platform == PlatformId::Web && profile.id != "web-release-chrome" {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "profile.validate",
            "Migration 8 Web profile only supports Chrome",
        ));
    }
    Ok(())
}

fn invalid_origin() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidProfile,
        "profile.validate",
        "HTTPS range origin must be a unique canonical HTTPS origin",
    )
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}
