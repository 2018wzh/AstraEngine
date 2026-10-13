use super::*;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) fn run_bundled_game(
    test_null_audio: bool,
    preview_control: bool,
) -> Result<(), PlayerCliError> {
    use astra_platform::{
        FixedDeadlineScheduler, HostLaunchProfile, PackageSourceRequest, PlatformEventKind,
        PlatformHostFactory, SurfaceRequest, WindowRequest,
    };
    use astra_player::{
        NativeVnHostCommandSource, PlatformCommandSink, PlayerHostCommandExecutor,
        PlayerHostResourceId,
    };
    use astra_ui_core::{UiInputEventKind, UiInsets, UiPoint, UiTouchPhase, UiViewport};
    use astra_vn_core::VnRunConfig;

    let (config, package, manifest, profile, package_storage_hash) = bundle_config::load_bundle()?;
    #[cfg(target_os = "macos")]
    let (mut runner, factory) = astra_platform_macos::main_thread_host()?;
    let player = async move {
        #[cfg(target_os = "windows")]
        let factory = astra_platform_windows::factory().with_test_null_audio(test_null_audio);
        #[cfg(target_os = "linux")]
        let factory = astra_platform_linux::factory();
        let mut session = factory.start(HostLaunchProfile::platform(profile)).await?;
        let mut preview: Option<preview_control::PreviewControl> = None;
        let session_result: Result<(), astra_platform::PlatformError> = async {
        let source = session
            .client
            .open_package(PackageSourceRequest::Bundled {
                relative_path: config.package.clone(),
                expected_hash: package_storage_hash.clone(),
            })
            .await?;
        let _container_header = session.client.read_package_range(source, 0, 16).await?;
        session.client.close_package(source).await?;

        let width = config.display.original_resolution.width;
        let height = config.display.original_resolution.height;
        let window = session
            .client
            .create_window(WindowRequest {
                title: if test_null_audio {
                    format!("{} [TEST NULL AUDIO]", manifest.package_id)
                } else {
                    manifest.package_id
                },
                width,
                height,
                visible: true,
            })
            .await?;
        let surface = session
            .client
            .create_surface(SurfaceRequest {
                window,
                width,
                height,
            })
            .await?;
        let logical_surface = PlayerHostResourceId(1);
        let mut sink = PlatformCommandSink::new(session.client.clone());
        sink.bind_surface(logical_surface, surface)?;
        let mut executor = PlayerHostCommandExecutor::new(sink);
        let runtime_execution = astra_player_vn::NativeVnRuntimeExecution::shipping_parallel();
        let Some(mut prepared) = astra_player::prepare_native_vn_open(&package, &mut session.events, runtime_execution).await? else { return Ok(()); };
        let mut startup_events = std::mem::take(&mut prepared.events);
        let mut vn = NativeVnHostCommandSource::from_package_with_options(
            &package,
            VnRunConfig {
                profile: config.profile,
                locale: config.locale,
            },
            width,
            height,
            logical_surface,
            prepared.options,
        )
        .map_err(|error| {
            astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.runtime.open",
                error.to_string(),
            )
        })?;
        let mut media = astra_player::NativeVnProductMediaHost::default();
        let player_result: Result<(), astra_platform::PlatformError> = async {
            astra_player::hydrate_save_catalog(&mut vn, &mut executor).await?;
            executor
                .execute_batch(vn.launch().map_err(|error| {
                    astra_platform::PlatformError::new(
                        astra_platform::PlatformErrorCode::InvalidState,
                        "player.runtime.launch",
                        error.to_string(),
                    )
                })?)
                .await
                .map_err(|error| {
                    astra_platform::PlatformError::new(
                        astra_platform::PlatformErrorCode::InvalidState,
                        "player.host.execute",
                        error.to_string(),
                    )
                })?;
            let mut pointer = (0.0_f64, 0.0_f64);
            let mut save_transaction_id = 1000_u64;
            let mut timeline_clock = native_session_clock::ActiveSessionClock::new(std::time::Instant::now());
            media
                .process(
                    &mut vn,
                    &mut executor,
                    timeline_clock.elapsed_ms(std::time::Instant::now()),
                    Vec::new(),
                )
                .await?;
            preview = if preview_control {
                Some(preview_control::PreviewControl::attach(&mut vn, &media).await?)
            } else { None };
            timeline_clock = native_session_clock::ActiveSessionClock::new(std::time::Instant::now());
            let mut timeline_tick = FixedDeadlineScheduler::new(
                std::time::Duration::from_nanos(16_666_667),
            )
            .map_err(|code| astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.runtime.scheduler",
                code,
            ))?;
            loop {
                let event = tokio::select! {
                    control = async { preview.as_mut().expect("preview branch enabled").recv().await }, if preview.is_some() => {
                        let request = control?;
                        if !preview.as_mut().unwrap().request(request, &mut vn, &mut media, &mut executor).await? { break; }
                        if preview.as_ref().unwrap().is_paused() {
                            timeline_clock.pause(std::time::Instant::now());
                        } else if timeline_clock.resume(std::time::Instant::now()) {
                            timeline_tick = FixedDeadlineScheduler::new(std::time::Duration::from_nanos(16_666_667)).map_err(|code| player_platform_error("player.preview.resume", code))?;
                        }
                        continue;
                    }
                    event = async { if let Some(event) = startup_events.pop_front() { Ok(event) } else { session.events.recv().await } } => event?,
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(timeline_tick.next_deadline())), if !timeline_clock.is_paused() => {
                        let due = timeline_tick.consume_due(std::time::Instant::now()).map_err(|code| {
                            astra_platform::PlatformError::new(
                                astra_platform::PlatformErrorCode::InvalidState,
                                "player.runtime.scheduler",
                                code,
                            )
                        })?.ok_or_else(|| astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.runtime.scheduler",
                            "scheduler woke before its deadline",
                        ))?;
                        for _ in 0..due.steps {
                            let mut policy_stepped = false;
                            if let Some(batch) = vn.tick_policy().map_err(|error| astra_platform::PlatformError::new(astra_platform::PlatformErrorCode::InvalidState, "player.runtime.policy_tick", error.to_string()))? {
                                policy_stepped = true;
                                executor.execute_batch(batch).await.map_err(|error| astra_platform::PlatformError::new(astra_platform::PlatformErrorCode::InvalidState, "player.host.execute", error.to_string()))?;
                            }
                            if let Some(batch) = vn.tick_presentation(16_666_667).map_err(|error| {
                                astra_platform::PlatformError::new(
                                    astra_platform::PlatformErrorCode::InvalidState,
                                    "player.runtime.presentation_tick",
                                    error.to_string(),
                                )
                            })? {
                                executor.execute_batch(batch).await.map_err(|error| {
                                    astra_platform::PlatformError::new(
                                        astra_platform::PlatformErrorCode::InvalidState,
                                        "player.host.execute",
                                        error.to_string(),
                                    )
                                })?;
                            }
                            if media.is_active() || policy_stepped {
                                let now_ms = timeline_clock.elapsed_ms(std::time::Instant::now());
                                media.poll_and_process(&mut vn, &mut executor, now_ms).await?;
                            }
                        }
                        if let Some(preview) = preview.as_mut() { preview.record(&mut vn, &media)?; }
                        continue;
                    }
                };
                let player_sequence = event.sequence;
                let ui_input = match event.kind {
                    PlatformEventKind::WindowClosed { window: closed } if closed == window => break,
                    _ if timeline_clock.is_paused() => continue,
                    PlatformEventKind::WindowResized {
                        window: resized,
                        width,
                        height,
                        scale_factor,
                    } if resized == window && width > 0 && height > 0 => {
                        Some(UiInputEventKind::Resize {
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
                        })
                    }
                    PlatformEventKind::Keyboard {
                        window: input_window,
                        physical_key,
                        logical_key,
                        state,
                        repeat,
                    } if input_window == window => {
                        Some(UiInputEventKind::Keyboard {
                            logical_key: logical_key.unwrap_or_else(|| physical_key.clone()),
                            physical_key,
                            state: ui_button_state(state),
                            repeat,
                            modifiers: 0,
                        })
                    }
                    PlatformEventKind::PointerMoved {
                        window: input_window,
                        x,
                        y,
                    } if input_window == window => {
                        pointer = (x, y);
                        Some(UiInputEventKind::PointerMove {
                            position: UiPoint {
                                x: x as f32,
                                y: y as f32,
                            },
                        })
                    }
                    PlatformEventKind::PointerButton {
                        window: input_window,
                        button,
                        state,
                    } if input_window == window => Some(UiInputEventKind::PointerButton {
                        position: UiPoint {
                            x: pointer.0 as f32,
                            y: pointer.1 as f32,
                        },
                        button: ui_pointer_button(button),
                        state: ui_button_state(state),
                    }),
                    PlatformEventKind::MouseWheel {
                        window: input_window,
                        delta_x,
                        delta_y,
                    } if input_window == window => Some(UiInputEventKind::Wheel {
                        delta_points: UiPoint {
                            x: delta_x,
                            y: delta_y,
                        },
                    }),
                    PlatformEventKind::ImePreedit {
                        window: input_window,
                        text,
                        cursor,
                    } if input_window == window => Some(UiInputEventKind::ImePreedit {
                        text,
                        cursor_start: cursor.map(|value| value.0 as u32),
                        cursor_end: cursor.map(|value| value.1 as u32),
                    }),
                    PlatformEventKind::ImeCommit {
                        window: input_window,
                        text,
                    } if input_window == window => Some(UiInputEventKind::ImeCommit { text }),
                    PlatformEventKind::Touch {
                        window: input_window,
                        id,
                        x,
                        y,
                        phase,
                    } if input_window == window => Some(UiInputEventKind::Touch {
                        device_id: 0,
                        contact_id: id,
                        position: UiPoint {
                            x: x as f32,
                            y: y as f32,
                        },
                        phase: match phase {
                            astra_platform::TouchPhase::Started => UiTouchPhase::Started,
                            astra_platform::TouchPhase::Moved => UiTouchPhase::Moved,
                            astra_platform::TouchPhase::Ended => UiTouchPhase::Ended,
                            astra_platform::TouchPhase::Cancelled => UiTouchPhase::Cancelled,
                        },
                    }),
                    PlatformEventKind::GamepadInput { control, value, .. } if value > 0.5 => {
                        gamepad_navigation(control)
                            .map(|action| UiInputEventKind::Navigation { action })
                    }
                    PlatformEventKind::AccessibilityAction {
                        window: input_window,
                        semantic_id,
                        action,
                        value,
                    } if input_window == window => Some(UiInputEventKind::AccessibilityAction {
                        semantic_id,
                        action,
                        value,
                    }),
                    _ => None,
                };
                if let Some(kind) = ui_input {
                    let batch = vn.prepare_ui_input(kind).map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.runtime.ui_input",
                            error.to_string(),
                        )
                    })?;
                    let results = executor.execute_batch(batch).await.map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.host.execute",
                            error.to_string(),
                        )
                    })?;
                    for result in results {
                        if let PlayerHostCommandResult::Captured { width, height, rgba8, .. } = result {
                            vn.cache_gameplay_surface(width, height, rgba8)
                                .map_err(|error| player_platform_error("player.save.capture.cache", error))?;
                        }
                    }
                    if vn.exit_requested() {
                        tracing::info!(
                            event = "player.session.exit_requested",
                            player_sequence,
                            "closing the Player session after a typed UI exit request"
                        );
                        break;
                    }
                    if let Some(request) = vn.take_ui_host_request() {
                        match request {
                            astra_player::VnUiHostRequest::Save { slot_id, .. } => {
                                save_transaction_id =
                                    save_transaction_id.checked_add(1).ok_or_else(|| {
                                        astra_platform::PlatformError::new(
                                            astra_platform::PlatformErrorCode::InvalidState,
                                            "player.save.transaction",
                                            "ASTRA_PLAYER_SAVE_TRANSACTION_OVERFLOW",
                                        )
                                    })?;
                                if let Err(error) = execute_platform_save(
                                    &mut vn,
                                    &media,
                                    &mut executor,
                                    &slot_id,
                                    PlayerHostResourceId(save_transaction_id),
                                    timeline_clock.elapsed_ms(std::time::Instant::now()),
                                )
                                .await
                                {
                                    vn.mark_save_failed(&slot_id).map_err(|cleanup_error| {
                                        player_platform_error(
                                            "player.save.abort_state",
                                            cleanup_error,
                                        )
                                    })?;
                                    return Err(error);
                                }
                                if let Some(batch) =
                                    vn.mark_save_committed(&slot_id).map_err(|error| {
                                        player_platform_error("player.save.commit_state", error)
                                    })?
                                {
                                    executor.execute_batch(batch).await.map_err(|error| {
                                        player_platform_error(
                                            "player.save.commit_completion",
                                            error,
                                        )
                                    })?;
                                }
                            }
                            astra_player::VnUiHostRequest::Load { slot_id } => {
                                execute_platform_load(&mut vn, &mut media, &mut executor, &slot_id).await?;
                            }
                            astra_player::VnUiHostRequest::Delete { slot_id } => {
                                executor
                                    .execute_batch(vn.delete_save(&slot_id).map_err(|error| {
                                        player_platform_error("player.save.delete.prepare", error)
                                    })?)
                                    .await
                                    .map_err(|error| {
                                        player_platform_error("player.save.delete", error)
                                    })?;
                                vn.mark_save_deleted(&slot_id).map_err(|error| {
                                    player_platform_error("player.save.delete_state", error)
                                })?;
                            }
                        }
                    }
                    media
                        .process_with_audio_tick(
                            &mut vn,
                            &mut executor,
                            timeline_clock.elapsed_ms(std::time::Instant::now()),
                            Vec::new(),
                            false,
                        )
                        .await?;
                    log_consumed_vn_step(player_sequence, "physical_ui", &vn)?;
                }
            }
            Ok(())
        }
        .await;
        // Every owner closes even when gameplay, media, or GPU submission failed.
        let mut failures = player_result.err().into_iter().collect::<Vec<_>>();
        if let Err(error) = media.shutdown(&mut vn, &mut executor).await {
            failures.push(error);
        }
        match vn.release_resources() {
            Ok(batch) => {
                if let Err(error) = executor.execute_batch(batch).await {
                    failures.push(player_platform_error(
                        "player.host.release_resources",
                        error,
                    ));
                }
            }
            Err(error) => failures.push(player_platform_error(
                "player.runtime.release_resources",
                error,
            )),
        }
        if let Err(error) = vn.shutdown() {
            failures.push(player_platform_error("player.runtime.shutdown", error));
        }
        if let Err(error) = session.client.destroy_surface(surface).await {
            failures.push(error);
        }
        if let Err(error) = session.client.destroy_window(window).await {
            failures.push(error);
        }
        if failures.len() == 1 {
            return Err(failures.remove(0));
        }
        if !failures.is_empty() {
            return Err(player_platform_error(
                "player.session",
                failures
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; cleanup: "),
            ));
        }
        Ok::<(), astra_platform::PlatformError>(())
        }.await;
        // The host owns partially created package, window and surface resources too.
        let shutdown_result = session.client.shutdown().await;
        let result = match (session_result, shutdown_result) {
            (Ok(()), result) | (result, Ok(())) => result,
            (Err(error), Err(cleanup)) => Err(player_platform_error(
                "player.session",
                format!("{error}; cleanup: {cleanup}"),
            )),
        };
        if let Some(preview) = &preview {
            preview.finished(result.is_ok());
        }
        result
    };
    #[cfg(target_os = "macos")]
    runner.run(player)??;
    #[cfg(not(target_os = "macos"))]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(player)?;
    Ok(())
}
