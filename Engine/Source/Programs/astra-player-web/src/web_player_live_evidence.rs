use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPlayerLiveEvidence {
    pub schema: String,
    pub event: String,
    pub target: String,
    pub profile: String,
    pub package_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_byte_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_step: Option<u64>,
    #[serde(default)]
    pub coverage_reached: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state_id: Option<String>,
    #[serde(default)]
    pub terminal_route_ids: Vec<String>,
    #[serde(default)]
    pub pending_choice_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_meter: Option<astra_player_vn::NativeVnAudioMeterSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPlayerConfig {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub platform: String,
    pub locale: String,
    pub package: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CookedPlatformProfiles {
    pub(super) schema: String,
    pub(super) profiles: Vec<serde_json::Value>,
}

pub fn validate_package(
    config: &WebPlayerConfig,
    package_bytes: &[u8],
) -> Result<PlatformHostProfile, PlatformError> {
    if config.schema != "astra.player_config.v2"
        || config.platform != "web"
        || config.locale.is_empty()
        || config.locale.len() > 64
        || !config
            .locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "web_player.config",
            "Web Player config identity is invalid",
        ));
    }
    let package = PackageReader::open(package_bytes).map_err(|error| {
        PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "web_player.package",
            format!("package integrity validation failed: {error}"),
        )
    })?;
    let manifest: PackageManifest = package
        .container()
        .decode_postcard("package.manifest")
        .map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "web_player.package",
                format!("package manifest is invalid: {error}"),
            )
        })?;
    if manifest.profile != config.profile {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "web_player.package",
            "package profile does not match Web Player config",
        ));
    }
    let profile_bytes = package
        .container()
        .read_section("platform.profiles")
        .map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "web_player.package",
                format!("platform profile section is missing: {error}"),
            )
        })?;
    let profiles: CookedPlatformProfiles =
        serde_json::from_slice(&profile_bytes).map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "web_player.package",
                format!("platform profile section is invalid: {error}"),
            )
        })?;
    if !matches!(profiles.schema.as_str(), "astra.platform_profiles.v3") {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "web_player.package",
            "platform profile section schema is unsupported",
        ));
    }
    let profile = profiles
        .profiles
        .into_iter()
        .map(migrate_host_profile_json)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "web_player.package",
                format!("platform profile migration failed: {error}"),
            )
        })?
        .into_iter()
        .find(|profile| {
            profile.platform == astra_platform::PlatformId::Web
                && profile.target == config.target
                && profile.package_id == manifest.package_id
        })
        .ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "web_player.package",
                "package does not contain a matching Web platform profile",
            )
        })?;
    validate_host_profile(&profile)?;
    Ok(profile)
}
