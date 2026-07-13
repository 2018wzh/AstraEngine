use astra_package::{PackageManifest, PackageReader};
use astra_platform::{
    migrate_host_profile_json, validate_host_profile, PlatformError, PlatformErrorCode,
    PlatformHostProfile,
};
use serde::{Deserialize, Serialize};

pub use astra_player_core::WEB_PLAYER_LIVE_EVIDENCE_SCHEMA;

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
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_step: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_state_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_event_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_presentation_hash: Option<String>,
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
struct CookedPlatformProfiles {
    schema: String,
    profiles: Vec<serde_json::Value>,
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

#[cfg(any(target_arch = "wasm32", feature = "web-code-check"))]
mod browser {
    use super::*;
    use astra_platform::{
        InputState, PlatformErrorCode, PlatformEventKind, PlatformHostClient, PlatformHostFactory,
        PointerButton, SurfaceHandle, SurfaceRequest, WindowHandle, WindowRequest,
    };
    use astra_player_core::{
        PlatformCommandSink, PlayerActionMap, PlayerHostCommandExecutor, PlayerHostCommandResult,
        PlayerHostResourceId,
    };
    use astra_player_vn::{NativeVnHostCommandSource, NativeVnProductMediaHost};
    use astra_vn_core::VnRunConfig;
    use futures_util::future::{select, Either};
    use futures_util::FutureExt;
    use js_sys::Promise;
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

    #[derive(Clone)]
    struct WebEvidenceIdentity {
        target: String,
        profile: String,
        package_hash: String,
    }

    #[wasm_bindgen(start)]
    pub async fn start() -> Result<(), JsValue> {
        tracing::info!(event = "player.web.start", "Web Player startup began");
        let config_bytes = fetch_bytes("AstraPlayer.config.json").await?;
        let config: WebPlayerConfig = serde_json::from_slice(&config_bytes)
            .map_err(|error| JsValue::from_str(&format!("invalid player config: {error}")))?;
        let package_bytes = fetch_bytes(&config.package).await?;
        let evidence_identity = WebEvidenceIdentity {
            target: config.target.clone(),
            profile: config.profile.clone(),
            package_hash: astra_core::Hash256::from_sha256(&package_bytes).to_string(),
        };
        let profile = validate_package(&config, &package_bytes)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        tracing::info!(
            event = "player.web.package.validated",
            profile = %config.profile,
            package_byte_size = package_bytes.len(),
            "Web Player package identity validated"
        );
        emit_web_evidence(&WebPlayerLiveEvidence {
            schema: WEB_PLAYER_LIVE_EVIDENCE_SCHEMA.to_string(),
            event: "package.validated".to_string(),
            target: evidence_identity.target.clone(),
            profile: evidence_identity.profile.clone(),
            package_hash: evidence_identity.package_hash.clone(),
            package_byte_size: Some(package_bytes.len() as u64),
            provider_id: None,
            session_id: None,
            player_sequence: None,
            input_kind: None,
            fixed_step: None,
            runtime_state_hash: None,
            runtime_event_hash: None,
            runtime_presentation_hash: None,
            coverage_reached: Vec::new(),
            current_state_id: None,
            terminal_route_ids: Vec::new(),
            pending_choice_ids: Vec::new(),
            audio_meter: None,
        })?;
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
        let logical_surface = PlayerHostResourceId(1);
        let mut sink = PlatformCommandSink::new(session.client.clone());
        sink.bind_surface(logical_surface, surface)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let mut executor = PlayerHostCommandExecutor::new(sink);
        let mut vn = NativeVnHostCommandSource::from_package(
            &package,
            VnRunConfig {
                profile: config.profile.clone(),
                locale: config.locale.clone(),
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
        let action_map = PlayerActionMap::standard();
        spawn_local(async move {
            let mut pointer = (0.0_f64, 0.0_f64);
            let mut media = NativeVnProductMediaHost::default();
            let timeline_clock = js_sys::Date::now();
            let mut user_activated = false;
            let mut save_transaction_id = 1_000_u64;
            loop {
                let event = events.recv().boxed_local();
                let tick = sleep(8).boxed_local();
                let event = match select(event, tick).await {
                    Either::Left((event, _)) => match event {
                        Ok(event) => event,
                        Err(_) => break,
                    },
                    Either::Right((result, _)) => {
                        if let Err(error) = result {
                            tracing::error!(
                                event = "player.web.media_timer.failed",
                                diagnostic_code = "ASTRA_PLAYER_WEB_TIMER",
                                error = ?error,
                                "Web Player media timer failed"
                            );
                            break;
                        }
                        if user_activated && media.is_active() {
                            let now_ms = (js_sys::Date::now() - timeline_clock).max(0.0) as u64;
                            if let Err(error) =
                                media.poll_and_process(&mut vn, &mut executor, now_ms).await
                            {
                                tracing::error!(
                                    event = "player.web.media.failed",
                                    diagnostic_code = "ASTRA_PLAYER_WEB_MEDIA",
                                    error = %error,
                                    "Web Player media processing failed"
                                );
                                break;
                            }
                        }
                        continue;
                    }
                };
                let player_sequence = event.sequence;
                match event.kind {
                    PlatformEventKind::WindowClosed { .. } => {
                        let shutdown = async {
                            let batch =
                                vn.release_resources().map_err(|error| error.to_string())?;
                            executor
                                .execute_batch(batch)
                                .await
                                .map_err(|error| error.to_string())?;
                            vn.shutdown().map_err(|error| error.to_string())?;
                            client
                                .destroy_surface(surface)
                                .await
                                .map_err(|error| error.to_string())?;
                            client
                                .destroy_window(window)
                                .await
                                .map_err(|error| error.to_string())?;
                            client.shutdown().await.map_err(|error| error.to_string())
                        }
                        .await;
                        if let Err(error) = shutdown {
                            tracing::error!(
                                event = "player.web.shutdown.failed",
                                diagnostic_code = "ASTRA_PLAYER_SHUTDOWN",
                                error = %error,
                                "Web Player shutdown failed"
                            );
                        }
                        PLAYER.with(|player| *player.borrow_mut() = None);
                        break;
                    }
                    PlatformEventKind::Keyboard {
                        physical_key,
                        state: InputState::Pressed,
                        ..
                    } => {
                        user_activated = true;
                        if physical_key == "F5" {
                            save_transaction_id = match save_transaction_id.checked_add(1) {
                                Some(value) => value,
                                None => {
                                    tracing::error!(
                                        event = "player.web.save.sequence_overflow",
                                        diagnostic_code = "ASTRA_PLAYER_SAVE_TRANSACTION_OVERFLOW",
                                        "Web Player save transaction sequence overflowed"
                                    );
                                    break;
                                }
                            };
                            if let Err(error) = execute_web_save(
                                &mut vn,
                                &mut executor,
                                "slot.quick",
                                PlayerHostResourceId(save_transaction_id),
                            )
                            .await
                            {
                                tracing::error!(
                                    event = "player.web.save.failed",
                                    diagnostic_code = "ASTRA_PLAYER_WEB_SAVE",
                                    error = %error,
                                    "Web Player save transaction failed"
                                );
                                break;
                            }
                            if let Err(error) = media
                                .process(
                                    &mut vn,
                                    &mut executor,
                                    (js_sys::Date::now() - timeline_clock).max(0.0) as u64,
                                    Vec::new(),
                                )
                                .await
                            {
                                tracing::error!(
                                    event = "player.web.media.failed",
                                    diagnostic_code = "ASTRA_PLAYER_WEB_MEDIA",
                                    error = %error,
                                    "Web Player media processing failed after save"
                                );
                                break;
                            }
                            continue;
                        }
                        if physical_key == "F9" {
                            if let Err(error) =
                                execute_web_load(&mut vn, &mut executor, "slot.quick").await
                            {
                                tracing::error!(
                                    event = "player.web.load.failed",
                                    diagnostic_code = "ASTRA_PLAYER_WEB_LOAD",
                                    error = %error,
                                    "Web Player load transaction failed"
                                );
                                break;
                            }
                            if let Err(error) = media
                                .process(
                                    &mut vn,
                                    &mut executor,
                                    (js_sys::Date::now() - timeline_clock).max(0.0) as u64,
                                    Vec::new(),
                                )
                                .await
                            {
                                tracing::error!(
                                    event = "player.web.media.failed",
                                    diagnostic_code = "ASTRA_PLAYER_WEB_MEDIA",
                                    error = %error,
                                    "Web Player media processing failed after load"
                                );
                                break;
                            }
                            continue;
                        }
                        let Some(action) = action_map.keyboard(&physical_key) else {
                            continue;
                        };
                        let batch = match vn.dispatch_action(action) {
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
                        if let Err(error) = media
                            .process(
                                &mut vn,
                                &mut executor,
                                (js_sys::Date::now() - timeline_clock).max(0.0) as u64,
                                Vec::new(),
                            )
                            .await
                        {
                            tracing::error!(
                                event = "player.web.media.failed",
                                diagnostic_code = "ASTRA_PLAYER_WEB_MEDIA",
                                error = %error,
                                "Web Player media processing failed after keyboard input"
                            );
                            break;
                        }
                        if let Err(error) = log_web_consumed_step(
                            player_sequence,
                            "keyboard",
                            &evidence_identity,
                            &vn,
                            &media,
                        ) {
                            tracing::error!(
                                event = "player.web.runtime.evidence_failed",
                                diagnostic_code = "ASTRA_PLAYER_RUNTIME_EVIDENCE",
                                error = %error,
                                "Web Player runtime evidence was unavailable"
                            );
                            break;
                        }
                    }
                    PlatformEventKind::PointerMoved { x, y, .. } => pointer = (x, y),
                    PlatformEventKind::PointerButton {
                        button: PointerButton::Primary,
                        state: InputState::Pressed,
                        ..
                    } => {
                        user_activated = true;
                        let batch = match vn.dispatch_pointer(pointer.0, pointer.1) {
                            Ok(batch) => batch,
                            Err(error) => {
                                tracing::error!(
                                    event = "player.web.runtime.hit_test_failed",
                                    diagnostic_code = "ASTRA_PLAYER_HIT_TEST",
                                    error = %error,
                                    "Web Player pointer hit-test failed"
                                );
                                break;
                            }
                        };
                        if let Err(error) = executor.execute_batch(batch).await {
                            tracing::error!(
                                event = "player.web.host_command.failed",
                                diagnostic_code = "ASTRA_PLAYER_HOST_COMMAND",
                                error = %error,
                                "Web Player host command failed"
                            );
                            break;
                        }
                        if let Err(error) = media
                            .process(
                                &mut vn,
                                &mut executor,
                                (js_sys::Date::now() - timeline_clock).max(0.0) as u64,
                                Vec::new(),
                            )
                            .await
                        {
                            tracing::error!(
                                event = "player.web.media.failed",
                                diagnostic_code = "ASTRA_PLAYER_WEB_MEDIA",
                                error = %error,
                                "Web Player media processing failed after pointer input"
                            );
                            break;
                        }
                        if let Err(error) = log_web_consumed_step(
                            player_sequence,
                            "pointer",
                            &evidence_identity,
                            &vn,
                            &media,
                        ) {
                            tracing::error!(
                                event = "player.web.runtime.evidence_failed",
                                diagnostic_code = "ASTRA_PLAYER_RUNTIME_EVIDENCE",
                                error = %error,
                                "Web Player runtime evidence was unavailable"
                            );
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Err(error) = media.shutdown(&mut vn, &mut executor).await {
                tracing::error!(
                    event = "player.web.media.cleanup_failed",
                    diagnostic_code = "ASTRA_PLAYER_WEB_MEDIA_CLEANUP",
                    error = %error,
                    "Web Player media cleanup failed"
                );
            }
            let _ = client.destroy_surface(surface).await;
            let _ = client.destroy_window(window).await;
            let _ = client.shutdown().await;
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

    async fn execute_web_save(
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        slot: &str,
        transaction: PlayerHostResourceId,
    ) -> Result<(), astra_platform::PlatformError> {
        let plan = source
            .prepare_save_transaction(slot, transaction)
            .map_err(|error| web_player_error("player.save.prepare", error))?;
        executor
            .execute_save_transaction(plan)
            .await
            .map_err(|error| web_player_error("player.save.transaction", error))?;
        Ok(())
    }

    async fn execute_web_load(
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        slot: &str,
    ) -> Result<(), astra_platform::PlatformError> {
        let results = executor
            .execute_batch(
                source
                    .read_save(slot)
                    .map_err(|error| web_player_error("player.save.read.prepare", error))?,
            )
            .await
            .map_err(|error| web_player_error("player.save.read", error))?;
        let bytes = match results.as_slice() {
            [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
            _ => {
                return Err(web_player_error(
                    "player.save.read",
                    "ASTRA_PLAYER_SAVE_RESULT_INVALID: platform returned an unexpected result",
                ));
            }
        };
        let present = source
            .restore(bytes)
            .map_err(|error| web_player_error("player.save.restore", error))?;
        executor
            .execute_batch(present)
            .await
            .map_err(|error| web_player_error("player.save.present", error))?;
        Ok(())
    }

    fn web_player_error(
        operation: &'static str,
        error: impl std::fmt::Display,
    ) -> astra_platform::PlatformError {
        astra_platform::PlatformError::new(
            PlatformErrorCode::InvalidState,
            operation,
            error.to_string(),
        )
    }

    fn log_web_consumed_step(
        player_sequence: u64,
        kind: &str,
        identity: &WebEvidenceIdentity,
        source: &NativeVnHostCommandSource,
        media: &NativeVnProductMediaHost,
    ) -> Result<(), astra_platform::PlatformError> {
        let evidence = source.last_step_evidence().ok_or_else(|| {
            web_player_error(
                "player.runtime.evidence",
                "ASTRA_PLAYER_RUNTIME_EVIDENCE_MISSING",
            )
        })?;
        tracing::info!(
            event = "astra.player.web.runtime.input_consumed",
            player_sequence,
            input_kind = kind,
            fixed_step = evidence.fixed_step,
            runtime_state_hash = %evidence.runtime_state_hash,
            runtime_event_hash = %evidence.runtime_event_hash,
            runtime_presentation_hash = %evidence.runtime_presentation_hash,
            terminal_route_count = evidence.terminal_route_ids.len(),
            pending_choice_count = evidence.pending_choice_ids.len(),
            "Web Player consumed a platform input through RuntimeWorld"
        );
        emit_web_evidence(&WebPlayerLiveEvidence {
            schema: WEB_PLAYER_LIVE_EVIDENCE_SCHEMA.to_string(),
            event: "runtime.input_consumed".to_string(),
            target: identity.target.clone(),
            profile: identity.profile.clone(),
            package_hash: identity.package_hash.clone(),
            package_byte_size: None,
            provider_id: Some(source.provider_id().to_string()),
            session_id: Some(source.session_id().to_string()),
            player_sequence: Some(player_sequence),
            input_kind: Some(kind.to_string()),
            fixed_step: Some(evidence.fixed_step),
            runtime_state_hash: Some(evidence.runtime_state_hash.to_string()),
            runtime_event_hash: Some(evidence.runtime_event_hash.to_string()),
            runtime_presentation_hash: Some(evidence.runtime_presentation_hash.to_string()),
            coverage_reached: evidence.coverage_reached.iter().cloned().collect(),
            current_state_id: evidence.current_state_id.clone(),
            terminal_route_ids: evidence.terminal_route_ids.iter().cloned().collect(),
            pending_choice_ids: evidence.pending_choice_ids.clone(),
            audio_meter: media.last_audio_meter(),
        })
        .map_err(|error| web_player_error("player.runtime.evidence", error))?;
        Ok(())
    }

    fn emit_web_evidence(value: &WebPlayerLiveEvidence) -> Result<(), String> {
        let encoded = serde_json::to_string(&value)
            .map_err(|error| format!("ASTRA_PLAYER_WEB_EVIDENCE_ENCODE: {error}"))?;
        web_sys::console::info_1(&JsValue::from_str(&format!(
            "ASTRA_PLAYER_EVIDENCE {encoded}"
        )));
        Ok(())
    }

    async fn sleep(milliseconds: i32) -> Result<(), JsValue> {
        let promise = Promise::new(&mut |resolve, reject| {
            let Some(window) = web_sys::window() else {
                let _ = reject.call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str("window unavailable"),
                );
                return;
            };
            if window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds)
                .is_err()
            {
                let _ = reject.call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str("timer registration failed"),
                );
            }
        });
        JsFuture::from(promise).await.map(|_| ())
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
