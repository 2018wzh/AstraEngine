use super::*;
use astra_core::Hash256;
use astra_package::{PackageManifest, PackageReader};
use astra_platform::{PlatformHostProfile, PlatformId};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Config {
    pub(super) schema: String,
    pub(super) target: String,
    pub(super) profile: String,
    pub(super) platform: String,
    pub(super) locale: String,
    pub(super) package: String,
    pub(super) package_storage_hash: String,
    #[serde(default)]
    pub(super) source_unlock: Option<SourceUnlockConfig>,
    pub(super) display: DisplayConfig,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceUnlockConfig {
    pub(super) schema: String,
    pub(super) source_profile: String,
}
#[derive(Deserialize)]
pub(super) struct DisplayConfig {
    pub(super) schema: String,
    pub(super) original_resolution: DisplayResolution,
    pub(super) scale_filter: String,
}
#[derive(Deserialize)]
pub(super) struct DisplayResolution {
    pub(super) width: u32,
    pub(super) height: u32,
}
#[derive(Deserialize)]
pub(super) struct Profiles {
    pub(super) schema: String,
    pub(super) profiles: Vec<serde_json::Value>,
}

pub(super) fn load_bundle() -> Result<
    (
        Config,
        PackageReader,
        PackageManifest,
        PlatformHostProfile,
        String,
    ),
    PlayerCliError,
> {
    let resource_root =
        bundled_player_resource_root().map_err(|error| -> PlayerCliError { error.into() })?;
    let config: Config =
        serde_json::from_slice(&fs::read(resource_root.join("AstraPlayer.config.json"))?)?;
    let expected_platform = match () {
        _ if cfg!(target_os = "windows") => "windows",
        _ if cfg!(target_os = "macos") => "macos",
        _ => "linux",
    };
    if config.schema != "astra.player_config.v2" || config.platform != expected_platform {
        return Err("Player config does not match the native platform".into());
    }
    if config.display.schema != "astra.player_display_config.v1"
        || config.display.original_resolution.width == 0
        || config.display.original_resolution.height == 0
        || config.display.original_resolution.width > 16_384
        || config.display.original_resolution.height > 16_384
        || !matches!(config.display.scale_filter.as_str(), "linear" | "nearest")
    {
        return Err("invalid native Player display config".into());
    }
    astra_platform::validate_safe_relative_path(&config.package)?;
    let expected_package_storage_hash = config
        .package_storage_hash
        .parse::<Hash256>()
        .map_err(|_| "Player config package_storage_hash is invalid")?;
    let package_source: std::sync::Arc<dyn astra_byte_source::BoundedByteSource> =
        std::sync::Arc::new(astra_byte_source::FileByteSource::open(
            resource_root.join(&config.package),
        )?);
    let container = astra_package::AstraContainerReader::open_storage_verified_source(
        package_source,
        expected_package_storage_hash,
    )?;
    let package_storage_hash = expected_package_storage_hash.to_string();
    let package = if container.has_section("source.unlock") {
        let unlock = config
            .source_unlock
            .as_ref()
            .ok_or("source-locked package requires Player source_unlock config")?;
        if unlock.schema != "astra.player_source_unlock.v1" {
            return Err("unsupported Player source unlock config".into());
        }
        astra_platform::validate_safe_relative_path(&unlock.source_profile)?;
        #[cfg(target_os = "windows")]
        let policy: astra_package::SourceUnlockPolicy =
            container.decode_postcard("source.unlock")?;
        #[cfg(target_os = "windows")]
        let source_manifest: astra_package::SourceVerificationManifest =
            serde_json::from_slice(&fs::read(resource_root.join(&unlock.source_profile))?)?;
        #[cfg(target_os = "windows")]
        {
            use astra_platform::UserAuthorizedSourceDirectoryProvider;
            let source = astra_platform_windows::WindowsSourceDirectoryProvider
                .authorize_source_directory()?;
            astra_player_vn::open_source_locked_verified_container(
                container,
                &policy,
                &source_manifest,
                &source,
            )?
        }
        #[cfg(not(target_os = "windows"))]
        {
            return Err("source-locked package is only enabled for the Windows RC".into());
        }
    } else {
        if config.source_unlock.is_some() {
            return Err("Player source_unlock config requires a source-locked package".into());
        }
        PackageReader::open_verified_container(container)?
    };
    let manifest: PackageManifest = package.container().decode_postcard("package.manifest")?;
    if manifest.profile != config.profile {
        return Err("Player config/package profile mismatch".into());
    }
    let profiles: Profiles =
        serde_json::from_slice(&package.container().read_section("platform.profiles")?)?;
    if !matches!(profiles.schema.as_str(), "astra.platform_profiles.v3") {
        return Err("unsupported platform profile section".into());
    }
    let profile = profiles
        .profiles
        .into_iter()
        .map(astra_platform::migrate_host_profile_json)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|profile| {
            profile.platform
                == match () {
                    _ if cfg!(target_os = "windows") => PlatformId::Windows,
                    _ if cfg!(target_os = "macos") => PlatformId::Macos,
                    _ => PlatformId::Linux,
                }
                && profile.target == config.target
                && profile.package_id == manifest.package_id
        })
        .ok_or("package does not contain a matching native platform profile")?;
    Ok((config, package, manifest, profile, package_storage_hash))
}
