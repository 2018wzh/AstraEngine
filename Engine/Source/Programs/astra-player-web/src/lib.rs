use astra_package::{PackageManifest, PackageReader};
use astra_platform::{
    migrate_host_profile_json, validate_host_profile, PlatformError, PlatformErrorCode,
    PlatformHostProfile,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPlayerConfig {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub platform: String,
    pub package: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CookedPlatformProfiles {
    schema: String,
    profiles: Vec<serde_json::Value>,
}

pub fn validate_package(
    config: &WebPlayerConfig,
    package_bytes: &[u8],
) -> Result<PlatformHostProfile, PlatformError> {
    if config.schema != "astra.player_config.v2" || config.platform != "web" {
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
    if !matches!(
        profiles.schema.as_str(),
        "astra.platform_profiles.v1" | "astra.platform_profiles.v2"
    ) {
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

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
    use astra_platform::{
        InputState, PlatformEventKind, PlatformHostClient, PlatformHostFactory, PointerButton,
        SurfaceHandle, SurfaceRequest, WindowHandle, WindowRequest,
    };
    use astra_player_core::{PlatformCommandSink, PlayerHostCommandExecutor, PlayerHostResourceId};
    use astra_player_vn::NativeVnHostCommandSource;
    use astra_vn_core::{CompiledStory, VnRunConfig};
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::{spawn_local, JsFuture};
    use web_sys::Response;

    thread_local! {
        static PLAYER: RefCell<Option<WebPlayerRuntime>> = const { RefCell::new(None) };
    }

    struct WebPlayerRuntime {
        _client: PlatformHostClient,
        _window: WindowHandle,
        _surface: SurfaceHandle,
        _package_bytes: Vec<u8>,
    }

    #[wasm_bindgen(start)]
    pub async fn start() -> Result<(), JsValue> {
        tracing::info!(event = "player.web.start", "Web Player startup began");
        let config_bytes = fetch_bytes("AstraPlayer.config.json").await?;
        let config: WebPlayerConfig = serde_json::from_slice(&config_bytes)
            .map_err(|error| JsValue::from_str(&format!("invalid player config: {error}")))?;
        let package_bytes = fetch_bytes(&config.package).await?;
        let profile = validate_package(&config, &package_bytes)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        tracing::info!(
            event = "player.web.package.validated",
            profile = %config.profile,
            package_byte_size = package_bytes.len(),
            "Web Player package identity validated"
        );
        let session = astra_platform_web::factory()
            .start(profile)
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let window = session
            .client
            .create_window(WindowRequest {
                title: "AstraPlayer".to_string(),
                width: 1280,
                height: 720,
                visible: true,
            })
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let surface = session
            .client
            .create_surface(SurfaceRequest {
                window,
                width: 1280,
                height: 720,
            })
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let package = astra_package::PackageReader::open(&package_bytes)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let compiled: CompiledStory = package
            .container()
            .decode_postcard("vn.compiled_story")
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let logical_surface = PlayerHostResourceId(1);
        let mut sink = PlatformCommandSink::new(session.client.clone());
        sink.bind_surface(logical_surface, surface)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let mut executor = PlayerHostCommandExecutor::new(sink);
        let mut vn = NativeVnHostCommandSource::new(
            compiled,
            VnRunConfig {
                profile: config.profile.clone(),
                locale: "zh-Hans".to_string(),
            },
            1280,
            720,
            logical_surface,
        )
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
        executor
            .execute_batch(
                vn.launch()
                    .map_err(|error| JsValue::from_str(&error.to_string()))?,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let client = session.client.clone();
        let mut events = session.events;
        spawn_local(async move {
            while let Ok(event) = events.recv().await {
                match event.kind {
                    PlatformEventKind::WindowClosed { .. } => {
                        let _ = client.destroy_surface(surface).await;
                        let _ = client.destroy_window(window).await;
                        let _ = client.shutdown().await;
                        PLAYER.with(|player| *player.borrow_mut() = None);
                        break;
                    }
                    PlatformEventKind::Keyboard {
                        state: InputState::Pressed,
                        ..
                    }
                    | PlatformEventKind::PointerButton {
                        button: PointerButton::Primary,
                        state: InputState::Pressed,
                        ..
                    } => {
                        let batch = match vn.primary_input() {
                            Ok(batch) => batch,
                            Err(error) => {
                                tracing::error!(
                                    event = "player.web.runtime.input_failed",
                                    diagnostic_code = "ASTRA_PLAYER_RUNTIME_INPUT",
                                    operation = "player.runtime.input",
                                    error = %error,
                                    "Web Player runtime input failed"
                                );
                                break;
                            }
                        };
                        if let Err(error) = executor.execute_batch(batch).await {
                            tracing::error!(
                                event = "player.web.host_command.failed",
                                diagnostic_code = "ASTRA_PLAYER_HOST_COMMAND",
                                operation = "player.host.execute",
                                error = %error,
                                "Web Player host command failed"
                            );
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });
        PLAYER.with(|player| {
            *player.borrow_mut() = Some(WebPlayerRuntime {
                _client: session.client,
                _window: window,
                _surface: surface,
                _package_bytes: package_bytes,
            });
        });
        tracing::info!(event = "player.web.ready", "Web Player host is ready");
        Ok(())
    }

    async fn fetch_bytes(path: &str) -> Result<Vec<u8>, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
        let response = JsFuture::from(window.fetch_with_str(path)).await?;
        let response: Response = response.dyn_into()?;
        if !response.ok() {
            return Err(JsValue::from_str(&format!(
                "fetch failed with status {}",
                response.status()
            )));
        }
        let buffer = JsFuture::from(response.array_buffer()?).await?;
        Ok(js_sys::Uint8Array::new(&buffer).to_vec())
    }
}
