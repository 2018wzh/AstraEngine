use astra_package::{PackageManifest, PackageReader};
use astra_platform::{
    InputState, PackageSourceRequest, PlatformError, PlatformErrorCode, PlatformEventKind,
    PlatformHostSession, SurfaceRequest, WindowRequest,
};
use astra_ui_core::{UiInputEventKind, UiInsets, UiPoint, UiTouchPhase, UiViewport};
use astra_vn_core::VnRunConfig;

use crate::{
    NativeVnHostCommandSource, NativeVnProductMediaHost, PlatformCommandSink,
    PlayerHostCommandExecutor, PlayerHostCommandResult, PlayerHostResourceId, VnUiHostRequest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnPlayerSessionConfig {
    pub profile: String,
    pub locale: String,
    pub bundled_package_path: String,
    pub width: u32,
    pub height: u32,
}

pub async fn run_native_vn_player_session(
    mut session: PlatformHostSession,
    package_bytes: Vec<u8>,
    config: NativeVnPlayerSessionConfig,
) -> Result<(), PlatformError> {
    if config.profile.is_empty()
        || config.locale.is_empty()
        || config.bundled_package_path.is_empty()
        || config.width == 0
        || config.height == 0
    {
        return Err(player_error(
            "player.session.open",
            "Player session config is incomplete",
        ));
    }
    let package_storage_hash = Hash256::from_sha256(&package_bytes).to_string();
    let package = PackageReader::open(&package_bytes)
        .map_err(|error| player_error_owned("player.package.open", error))?;
    let manifest: PackageManifest = package
        .container()
        .decode_postcard("package.manifest")
        .map_err(|error| player_error_owned("player.package.manifest", error))?;
    if manifest.profile != config.profile {
        return Err(player_error(
            "player.package.profile",
            "Player config/package profile mismatch",
        ));
    }
    let source = session
        .client
        .open_package(PackageSourceRequest::Bundled {
            relative_path: config.bundled_package_path,
            expected_hash: package_storage_hash,
        })
        .await?;
    let header = session.client.read_package_range(source, 0, 16).await?;
    if header.len() != 16 {
        return Err(player_error(
            "player.package.read",
            "platform package source returned a short container header",
        ));
    }
    session.client.close_package(source).await?;

    let window = session
        .client
        .create_window(WindowRequest {
            title: manifest.package_id,
            width: config.width,
            height: config.height,
            visible: true,
        })
        .await?;
    let surface = session
        .client
        .create_surface(SurfaceRequest {
            window,
            width: config.width,
            height: config.height,
        })
        .await?;
    let logical_surface = PlayerHostResourceId(1);
    let mut sink = PlatformCommandSink::new(session.client.clone());
    sink.bind_surface(logical_surface, surface)?;
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let mut vn = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig {
            profile: config.profile,
            locale: config.locale,
        },
        config.width,
        config.height,
        logical_surface,
    )
    .map_err(|error| player_error_owned("player.runtime.open", error))?;
    executor
        .execute_batch(
            vn.launch()
                .map_err(|error| player_error_owned("player.runtime.launch", error))?,
        )
        .await
        .map_err(|error| player_error_owned("player.host.execute", error))?;

    let mut pointer = (0.0_f64, 0.0_f64);
    let mut save_transaction_id = 1000_u64;
    let timeline_clock = std::time::Instant::now();
    let mut media = NativeVnProductMediaHost::default();
    let player_result: Result<(), PlatformError> = async {
        media
            .process(
                &mut vn,
                &mut executor,
                timeline_clock.elapsed().as_millis() as u64,
                Vec::new(),
            )
            .await?;
        let mut timeline_tick = tokio::time::interval(std::time::Duration::from_millis(8));
        timeline_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let event = tokio::select! {
                event = session.events.recv() => event?,
                _ = timeline_tick.tick(), if media.is_active() => {
                    media.poll_and_process(
                        &mut vn,
                        &mut executor,
                        timeline_clock.elapsed().as_millis() as u64,
                    ).await?;
                    continue;
                }
            };
            let player_sequence = event.sequence;
            let ui_input = match event.kind {
                PlatformEventKind::WindowClosed { window: closed } if closed == window => break,
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
                PlatformEventKind::WindowInsetsChanged { insets, .. } => {
                    Some(UiInputEventKind::Resize {
                        viewport: UiViewport {
                            physical_width: config.width,
                            physical_height: config.height,
                            scale_factor: 1.0,
                            font_scale: 1.0,
                            safe_area_points: UiInsets {
                                left: insets.left as f32,
                                top: insets.top as f32,
                                right: insets.right as f32,
                                bottom: insets.bottom as f32,
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
                } if input_window == window => Some(UiInputEventKind::Keyboard {
                    logical_key: logical_key.unwrap_or_else(|| physical_key.clone()),
                    physical_key,
                    state: ui_button_state(state),
                    repeat,
                    modifiers: 0,
                }),
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
                let batch = vn
                    .dispatch_ui_event(kind)
                    .map_err(|error| player_error_owned("player.runtime.ui_input", error))?;
                executor
                    .execute_batch(batch)
                    .await
                    .map_err(|error| player_error_owned("player.host.execute", error))?;
                if let Some(request) = vn.take_ui_host_request() {
                    match request {
                        VnUiHostRequest::Save { slot_id, .. } => {
                            save_transaction_id =
                                save_transaction_id.checked_add(1).ok_or_else(|| {
                                    player_error(
                                        "player.save.transaction",
                                        "save transaction counter overflowed",
                                    )
                                })?;
                            if let Err(error) = execute_platform_save(
                                &mut vn,
                                &mut executor,
                                &slot_id,
                                PlayerHostResourceId(save_transaction_id),
                            )
                            .await
                            {
                                vn.mark_save_failed(&slot_id).map_err(|cleanup_error| {
                                    player_error_owned("player.save.abort_state", cleanup_error)
                                })?;
                                return Err(error);
                            }
                            if let Some(batch) =
                                vn.mark_save_committed(&slot_id).map_err(|error| {
                                    player_error_owned("player.save.commit_state", error)
                                })?
                            {
                                executor.execute_batch(batch).await.map_err(|error| {
                                    player_error_owned("player.save.commit_completion", error)
                                })?;
                            }
                        }
                        VnUiHostRequest::Load { slot_id } => {
                            execute_platform_load(&mut vn, &mut executor, &slot_id).await?;
                        }
                        VnUiHostRequest::Delete { slot_id } => {
                            executor
                                .execute_batch(vn.delete_save(&slot_id).map_err(|error| {
                                    player_error_owned("player.save.delete.prepare", error)
                                })?)
                                .await
                                .map_err(|error| player_error_owned("player.save.delete", error))?;
                            vn.mark_save_deleted(&slot_id).map_err(|error| {
                                player_error_owned("player.save.delete_state", error)
                            })?;
                        }
                    }
                }
                media
                    .process(
                        &mut vn,
                        &mut executor,
                        timeline_clock.elapsed().as_millis() as u64,
                        Vec::new(),
                    )
                    .await?;
                log_consumed_vn_step(player_sequence, &vn)?;
            }
        }
        Ok(())
    }
    .await;

    let media_cleanup = media.shutdown(&mut vn, &mut executor).await;
    match (player_result, media_cleanup) {
        (Err(error), Err(cleanup)) => {
            return Err(player_error_owned(
                "player.session",
                format!("{error}; media cleanup failed: {cleanup}"),
            ));
        }
        (Err(error), Ok(())) => return Err(error),
        (Ok(()), Err(cleanup)) => return Err(cleanup),
        (Ok(()), Ok(())) => {}
    }
    let release = vn
        .release_resources()
        .map_err(|error| player_error_owned("player.runtime.release_resources", error))?;
    executor
        .execute_batch(release)
        .await
        .map_err(|error| player_error_owned("player.host.release_resources", error))?;
    vn.shutdown()
        .map_err(|error| player_error_owned("player.runtime.shutdown", error))?;
    session.client.destroy_surface(surface).await?;
    session.client.destroy_window(window).await?;
    session.client.shutdown().await?;
    Ok(())
}

async fn execute_platform_save(
    vn: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    slot: &str,
    transaction: PlayerHostResourceId,
) -> Result<(), PlatformError> {
    let plan = vn
        .prepare_save_transaction(slot, transaction)
        .map_err(|error| player_error_owned("player.save.prepare", error))?;
    executor
        .execute_save_transaction(plan)
        .await
        .map_err(|error| player_error_owned("player.save.commit", error))?;
    Ok(())
}

async fn execute_platform_load(
    vn: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    slot: &str,
) -> Result<(), PlatformError> {
    let read = vn
        .read_save(slot)
        .map_err(|error| player_error_owned("player.load.prepare", error))?;
    let results = executor
        .execute_batch(read)
        .await
        .map_err(|error| player_error_owned("player.load.read", error))?;
    let payload = match results.as_slice() {
        [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
        _ => {
            return Err(player_error(
                "player.load.read",
                "platform returned an unexpected save read result",
            ));
        }
    };
    let restore = vn
        .restore(payload)
        .map_err(|error| player_error_owned("player.load.restore", error))?;
    executor
        .execute_batch(restore)
        .await
        .map_err(|error| player_error_owned("player.load.present", error))?;
    Ok(())
}

fn ui_button_state(state: InputState) -> astra_ui_core::UiButtonState {
    match state {
        InputState::Pressed => astra_ui_core::UiButtonState::Pressed,
        InputState::Released => astra_ui_core::UiButtonState::Released,
    }
}

fn ui_pointer_button(button: astra_platform::PointerButton) -> astra_ui_core::UiPointerButton {
    match button {
        astra_platform::PointerButton::Primary => astra_ui_core::UiPointerButton::Primary,
        astra_platform::PointerButton::Secondary => astra_ui_core::UiPointerButton::Secondary,
        astra_platform::PointerButton::Middle => astra_ui_core::UiPointerButton::Middle,
        astra_platform::PointerButton::Back => astra_ui_core::UiPointerButton::Back,
        astra_platform::PointerButton::Forward => astra_ui_core::UiPointerButton::Forward,
        astra_platform::PointerButton::Other(value) => astra_ui_core::UiPointerButton::Other(value),
    }
}

fn gamepad_navigation(
    control: astra_platform::GamepadControl,
) -> Option<astra_ui_core::UiNavigationAction> {
    use astra_platform::GamepadControl;
    use astra_ui_core::UiNavigationAction;
    match control {
        GamepadControl::DpadUp => Some(UiNavigationAction::Up),
        GamepadControl::DpadDown => Some(UiNavigationAction::Down),
        GamepadControl::DpadLeft => Some(UiNavigationAction::Left),
        GamepadControl::DpadRight => Some(UiNavigationAction::Right),
        GamepadControl::South => Some(UiNavigationAction::Activate),
        GamepadControl::East => Some(UiNavigationAction::Cancel),
        GamepadControl::LeftShoulder => Some(UiNavigationAction::PagePrevious),
        GamepadControl::RightShoulder => Some(UiNavigationAction::PageNext),
        _ => None,
    }
}

fn log_consumed_vn_step(
    player_sequence: u64,
    vn: &NativeVnHostCommandSource,
) -> Result<(), PlatformError> {
    let evidence = vn.last_step_evidence().ok_or_else(|| {
        player_error(
            "player.runtime.evidence",
            "consumed input has no RuntimeWorld evidence",
        )
    })?;
    tracing::trace!(
        event = "astra.player.input.consumed",
        player_sequence,
        fixed_step = evidence.fixed_step,
        runtime_state_hash = %evidence.runtime_state_hash,
        runtime_event_hash = %evidence.runtime_event_hash,
        runtime_presentation_hash = %evidence.runtime_presentation_hash,
        "Player consumed physical platform input"
    );
    Ok(())
}

fn player_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

fn player_error_owned(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
use astra_core::Hash256;
