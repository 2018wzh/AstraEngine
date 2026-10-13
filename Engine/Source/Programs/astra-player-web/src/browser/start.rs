use super::*;

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
        session_id: None,
        player_sequence: None,
        input_kind: None,
        fixed_step: None,
        coverage_reached: Vec::new(),
        current_state_id: None,
        terminal_route_ids: Vec::new(),
        pending_choice_ids: Vec::new(),
        audio_meter: None,
    })?;
    let session = astra_platform_web::factory()
        .start(HostLaunchProfile::platform(profile))
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
    spawn_local(async move {
        let mut pointer = (0.0_f64, 0.0_f64);
        let mut media = NativeVnProductMediaHost::default();
        let timeline_clock = js_sys::Date::now();
        let mut user_activated = false;
        let mut save_transaction_id = 1_000_u64;
        loop {
            let event = events.recv().boxed_local();
            let tick = next_animation_frame().boxed_local();
            let event = match select(event, tick).await {
                Either::Left((event, _)) => match event {
                    Ok(event) => event,
                    Err(_) => break,
                },
                Either::Right((result, _)) => {
                    if let Err(error) = result {
                        tracing::error!(
                            event = "player.web.media_frame.failed",
                            diagnostic_code = "ASTRA_PLAYER_WEB_ANIMATION_FRAME",
                            error = ?error,
                            "Web Player animation frame wake failed"
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
                    PLAYER.with(|player| *player.borrow_mut() = None);
                    break;
                }
                PlatformEventKind::WindowResized {
                    width,
                    height,
                    scale_factor,
                    ..
                } if width > 0 && height > 0 => {
                    let batch = match vn.dispatch_ui_event(UiInputEventKind::Resize {
                        viewport: UiViewport {
                            physical_width: width,
                            physical_height: height,
                            scale_factor: scale_factor as f32,
                            font_scale: 1.0,
                            safe_area_points: UiInsets {
                                left: 0.0,
                                top: 0.0,
                                right: 0.0,
                                bottom: 0.0,
                            },
                        },
                    }) {
                        Ok(batch) => batch,
                        Err(error) => {
                            tracing::error!(event = "player.web.runtime.resize_failed", diagnostic_code = "ASTRA_PLAYER_UI_RESIZE", error = %error, "Web Player resize transaction failed");
                            break;
                        }
                    };
                    if let Err(error) = executor.execute_batch(batch).await {
                        tracing::error!(event = "player.web.host_command.failed", diagnostic_code = "ASTRA_PLAYER_HOST_COMMAND", error = %error, "Web Player resized frame execution failed");
                        break;
                    }
                }
                PlatformEventKind::Keyboard {
                    physical_key,
                    logical_key,
                    state,
                    repeat,
                    ..
                } => {
                    user_activated |= state == InputState::Pressed;
                    if state == InputState::Pressed && physical_key == "F5" {
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
                    if state == InputState::Pressed && physical_key == "F9" {
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
                    let batch = match vn.dispatch_ui_event(UiInputEventKind::Keyboard {
                        logical_key: logical_key.unwrap_or_else(|| physical_key.clone()),
                        physical_key,
                        state: match state {
                            InputState::Pressed => UiButtonState::Pressed,
                            InputState::Released => UiButtonState::Released,
                        },
                        repeat,
                        modifiers: 0,
                    }) {
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
                    if let Err(error) = execute_web_ui_host_request(
                        &mut vn,
                        &mut executor,
                        &mut save_transaction_id,
                    )
                    .await
                    {
                        tracing::error!(event = "player.web.ui_host_request.failed", diagnostic_code = "ASTRA_PLAYER_UI_HOST_REQUEST", error = %error, "Web Player UI host request failed");
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
                PlatformEventKind::PointerMoved { x, y, .. } => {
                    pointer = (x, y);
                    let batch = match vn.dispatch_ui_event(UiInputEventKind::PointerMove {
                        position: UiPoint {
                            x: x as f32,
                            y: y as f32,
                        },
                    }) {
                        Ok(batch) => batch,
                        Err(error) => {
                            tracing::error!(event = "player.web.runtime.pointer_move_failed", diagnostic_code = "ASTRA_PLAYER_UI_INPUT", error = %error, "Web Player pointer move failed");
                            break;
                        }
                    };
                    if let Err(error) = executor.execute_batch(batch).await {
                        tracing::error!(event = "player.web.host_command.failed", diagnostic_code = "ASTRA_PLAYER_HOST_COMMAND", error = %error, "Web Player host command failed");
                        break;
                    }
                    if let Err(error) = execute_web_ui_host_request(
                        &mut vn,
                        &mut executor,
                        &mut save_transaction_id,
                    )
                    .await
                    {
                        tracing::error!(event = "player.web.ui_host_request.failed", diagnostic_code = "ASTRA_PLAYER_UI_HOST_REQUEST", error = %error, "Web Player UI host request failed");
                        break;
                    }
                }
                PlatformEventKind::PointerButton { button, state, .. } => {
                    user_activated |= state == InputState::Pressed;
                    let batch = match vn.dispatch_ui_event(UiInputEventKind::PointerButton {
                        position: UiPoint {
                            x: pointer.0 as f32,
                            y: pointer.1 as f32,
                        },
                        button: match button {
                            PointerButton::Primary => AstraUiPointerButton::Primary,
                            PointerButton::Secondary => AstraUiPointerButton::Secondary,
                            PointerButton::Middle => AstraUiPointerButton::Middle,
                            PointerButton::Back => AstraUiPointerButton::Back,
                            PointerButton::Forward => AstraUiPointerButton::Forward,
                            PointerButton::Other(value) => AstraUiPointerButton::Other(value),
                        },
                        state: match state {
                            InputState::Pressed => UiButtonState::Pressed,
                            InputState::Released => UiButtonState::Released,
                        },
                    }) {
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
                    if let Err(error) = execute_web_ui_host_request(
                        &mut vn,
                        &mut executor,
                        &mut save_transaction_id,
                    )
                    .await
                    {
                        tracing::error!(event = "player.web.ui_host_request.failed", diagnostic_code = "ASTRA_PLAYER_UI_HOST_REQUEST", error = %error, "Web Player UI host request failed");
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
                PlatformEventKind::AccessibilityAction {
                    semantic_id,
                    action,
                    value,
                    ..
                } => {
                    let batch = match vn.dispatch_ui_event(UiInputEventKind::AccessibilityAction {
                        semantic_id,
                        action,
                        value,
                    }) {
                        Ok(batch) => batch,
                        Err(error) => {
                            tracing::error!(event = "player.web.accessibility.input_failed", diagnostic_code = "ASTRA_PLAYER_ACCESSIBILITY_INPUT", error = %error, "Web accessibility action failed");
                            break;
                        }
                    };
                    if let Err(error) = executor.execute_batch(batch).await {
                        tracing::error!(event = "player.web.accessibility.command_failed", diagnostic_code = "ASTRA_PLAYER_ACCESSIBILITY_COMMAND", error = %error, "Web accessibility command failed");
                        break;
                    }
                    if let Err(error) = execute_web_ui_host_request(
                        &mut vn,
                        &mut executor,
                        &mut save_transaction_id,
                    )
                    .await
                    {
                        tracing::error!(event = "player.web.accessibility.host_request_failed", diagnostic_code = "ASTRA_PLAYER_ACCESSIBILITY_HOST_REQUEST", error = %error, "Web accessibility host request failed");
                        break;
                    }
                    if let Err(error) = log_web_consumed_step(
                        player_sequence,
                        "accessibility",
                        &evidence_identity,
                        &vn,
                        &media,
                    ) {
                        tracing::error!(event = "player.web.accessibility.evidence_failed", diagnostic_code = "ASTRA_PLAYER_ACCESSIBILITY_EVIDENCE", error = %error, "Web accessibility evidence was unavailable");
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
        match vn.release_resources() {
            Ok(batch) => {
                if let Err(error) = executor.execute_batch(batch).await {
                    tracing::error!(event = "player.web.shutdown.failed", diagnostic_code = "ASTRA_PLAYER_SHUTDOWN", error = %error, "Web Player resource release failed");
                } else if let Err(error) = vn.shutdown() {
                    tracing::error!(event = "player.web.shutdown.failed", diagnostic_code = "ASTRA_PLAYER_SHUTDOWN", error = %error, "Web Player runtime shutdown failed");
                }
            }
            Err(error) => {
                tracing::error!(event = "player.web.shutdown.failed", diagnostic_code = "ASTRA_PLAYER_SHUTDOWN", error = %error, "Web Player resource release planning failed")
            }
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
