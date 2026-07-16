//! Android packaged Player entrypoint.
//!
//! The Kotlin `GameActivity` is a lifecycle and permission adapter only. Game
//! state, package validation, platform resources, and the Player session remain
//! owned by Rust.

pub const ANDROID_PLAYER_LIBRARY_NAME: &str = "astra_player_android";

#[cfg(target_os = "android")]
mod android {
    use std::{ffi::CString, io::Read};

    use android_activity::AndroidApp;
    use astra_package::{PackageManifest, PackageReader};
    use astra_platform::{HostLaunchProfile, PlatformError, PlatformErrorCode, PlatformId};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Profiles {
        schema: String,
        profiles: Vec<serde_json::Value>,
    }

    #[unsafe(no_mangle)]
    pub fn android_main(app: AndroidApp) {
        if let Err(error) = run(app) {
            tracing::error!(
                event = "player.android.host.failed",
                diagnostic_code = ?error.code,
                operation = %error.operation,
                "Android Player host terminated"
            );
            panic!("Android Player host failed: {error}");
        }
    }

    fn run(app: AndroidApp) -> Result<(), PlatformError> {
        let package_bytes = read_asset(&app, "game.astrapkg")?;
        let package = PackageReader::open(&package_bytes)
            .map_err(|error| player_error("player.package.open", error))?;
        let manifest: PackageManifest = package
            .container()
            .decode_postcard("package.manifest")
            .map_err(|error| player_error("player.package.manifest", error))?;
        let profiles: Profiles = serde_json::from_slice(
            &package
                .container()
                .read_section("platform.profiles")
                .map_err(|error| player_error("player.package.profiles", error))?,
        )
        .map_err(|error| player_error("player.package.profiles", error))?;
        if !matches!(
            profiles.schema.as_str(),
            "astra.platform_profiles.v1" | "astra.platform_profiles.v2"
        ) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "player.package.profiles",
                "unsupported platform profile section",
            ));
        }
        let mut matches = profiles
            .profiles
            .into_iter()
            .map(astra_platform::migrate_host_profile_json)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|profile| {
                profile.platform == PlatformId::Android && profile.package_id == manifest.package_id
            });
        let profile = matches.next().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "player.package.profiles",
                "package does not contain a matching Android host profile",
            )
        })?;
        if matches.next().is_some() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "player.package.profiles",
                "package contains multiple eligible Android host profiles",
            ));
        }
        let locale = astra_vn_package::load_player_locale_config(&package)
            .map_err(|error| player_error("player.package.locale", error))?
            .default_locale;
        let config = astra_player::NativeVnPlayerSessionConfig {
            profile: manifest.profile,
            locale,
            bundled_package_path: "game.astrapkg".to_string(),
            width: 1280,
            height: 720,
        };
        drop(package);
        astra_platform_android::run_player_host(
            app,
            HostLaunchProfile::platform(profile),
            move |session| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| player_error("player.runtime.start", error))?;
                runtime.block_on(astra_player::run_native_vn_player_session(
                    session,
                    package_bytes,
                    config,
                ))
            },
        )
    }

    fn read_asset(app: &AndroidApp, name: &str) -> Result<Vec<u8>, PlatformError> {
        let name =
            CString::new(name).map_err(|error| player_error("player.package.asset", error))?;
        let mut asset = app.asset_manager().open(&name).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "player.package.asset",
                "bundled game.astrapkg asset is missing",
            )
        })?;
        let expected = asset.length();
        let mut bytes = Vec::with_capacity(expected);
        asset
            .read_to_end(&mut bytes)
            .map_err(|error| player_error("player.package.asset", error))?;
        if bytes.len() != expected || bytes.is_empty() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "player.package.asset",
                "bundled package asset length is invalid",
            ));
        }
        Ok(bytes)
    }

    fn player_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            operation,
            error.to_string(),
        )
    }
}
