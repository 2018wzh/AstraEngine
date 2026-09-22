//! Android packaged Player entrypoint.
//!
//! The Kotlin `GameActivity` is a lifecycle and permission adapter only. Game
//! state, package validation, platform resources, and the Player session remain
//! owned by Rust.

#[cfg(any(target_os = "android", test))]
mod package_source;

pub const ANDROID_PLAYER_LIBRARY_NAME: &str = "astra_player_android";

#[cfg(target_os = "android")]
mod android {
    use crate::package_source::CancellableSource;
    use astra_byte_source::MemoryByteSource;
    use std::sync::{atomic::AtomicBool, Arc, OnceLock};

    use android_activity::AndroidApp;
    use astra_package::{AstraContainerReader, PackageManifest, PackageReader};
    use astra_platform::{HostLaunchProfile, PlatformError, PlatformErrorCode, PlatformId};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Profiles {
        schema: String,
        profiles: Vec<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct DisplayConfig {
        schema: String,
        original_resolution: DisplayResolution,
    }

    #[derive(Deserialize)]
    struct DisplayResolution {
        width: u32,
        height: u32,
    }

    #[unsafe(no_mangle)]
    pub fn android_main(app: AndroidApp) {
        static LOGGING: OnceLock<astra_observability::ObservabilityGuard> = OnceLock::new();
        LOGGING.get_or_init(|| {
            let mut logging = astra_observability::HostObservabilityConfig::for_cli("info");
            logging.role = astra_observability::HostRole::Player;
            astra_observability::init_host(logging)
                .expect("Android Player logging initialization failed")
        });
        if let Err(error) = run(app) {
            tracing::error!(
                event = "player.android.host.failed",
                diagnostic_code = ?error.code,
                operation = %error.operation,
                "Android Player host terminated"
            );
        }
    }

    fn run(app: AndroidApp) -> Result<(), PlatformError> {
        astra_platform_android::run_player_host(app, prepare)
    }

    fn prepare(
        package_bytes: Arc<Vec<u8>>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<astra_platform_android::AndroidPreparedPlayer, PlatformError> {
        let source = Arc::new(CancellableSource {
            source: MemoryByteSource::from_shared(package_bytes),
            cancelled,
        });
        let (container, storage_hash) =
            AstraContainerReader::open_storage_audited_source(source)
                .map_err(|error| player_error("player.package.audit", error))?;
        let package = PackageReader::open_verified_container(container)
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
        if !matches!(profiles.schema.as_str(), "astra.platform_profiles.v3") {
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
        let display: DisplayConfig = serde_json::from_slice(
            &package
                .container()
                .read_section("player.display_config")
                .map_err(|error| player_error("player.package.display", error))?,
        )
        .map_err(|error| player_error("player.package.display", error))?;
        if display.schema != "astra.player_display_config.v1"
            || !(1..=16_384).contains(&display.original_resolution.width)
            || !(1..=16_384).contains(&display.original_resolution.height)
        {
            return Err(player_error(
                "player.package.display",
                "invalid display configuration",
            ));
        }
        let config = astra_player::NativeVnPlayerSessionConfig {
            profile: manifest.profile,
            locale,
            width: display.original_resolution.width,
            height: display.original_resolution.height,
        };
        Ok(astra_platform_android::AndroidPreparedPlayer {
            profile: HostLaunchProfile::platform(profile),
            storage_hash: storage_hash.to_string(),
            player: Box::new(move |session| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| player_error("player.runtime.start", error))?;
                runtime.block_on(astra_player::run_native_vn_player_session(
                    session, package, config,
                ))
            }),
        })
    }

    fn player_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            operation,
            error.to_string(),
        )
    }
}
