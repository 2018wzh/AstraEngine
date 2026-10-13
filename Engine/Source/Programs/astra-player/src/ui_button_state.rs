use super::*;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) fn ui_button_state(state: astra_platform::InputState) -> astra_ui_core::UiButtonState {
    match state {
        astra_platform::InputState::Pressed => astra_ui_core::UiButtonState::Pressed,
        astra_platform::InputState::Released => astra_ui_core::UiButtonState::Released,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) fn ui_pointer_button(
    button: astra_platform::PointerButton,
) -> astra_ui_core::UiPointerButton {
    match button {
        astra_platform::PointerButton::Primary => astra_ui_core::UiPointerButton::Primary,
        astra_platform::PointerButton::Secondary => astra_ui_core::UiPointerButton::Secondary,
        astra_platform::PointerButton::Middle => astra_ui_core::UiPointerButton::Middle,
        astra_platform::PointerButton::Back => astra_ui_core::UiPointerButton::Back,
        astra_platform::PointerButton::Forward => astra_ui_core::UiPointerButton::Forward,
        astra_platform::PointerButton::Other(value) => astra_ui_core::UiPointerButton::Other(value),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) fn gamepad_navigation(
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

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) async fn execute_platform_save(
    source: &mut astra_player::NativeVnHostCommandSource,
    media: &astra_player::NativeVnProductMediaHost,
    executor: &mut astra_player::PlayerHostCommandExecutor<astra_player::PlatformCommandSink>,
    slot: &str,
    transaction: astra_player::PlayerHostResourceId,
    playtime_ms: u64,
) -> Result<(), astra_platform::PlatformError> {
    if !source.has_gameplay_thumbnail_capture() {
        capture_gameplay_surface(source, executor).await?;
    }
    let timestamp = astra_player::current_save_timestamp();
    source
        .prepare_save_metadata(slot, timestamp, playtime_ms)
        .map_err(|error| player_platform_error("player.save.metadata", error))?;
    let plan = source
        .prepare_product_save_transaction(slot, transaction, media)
        .map_err(|error| player_platform_error("player.save.prepare", error))?;
    executor
        .execute_save_transaction(plan)
        .await
        .map_err(|error| player_platform_error("player.save.transaction", error))?;
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) async fn capture_gameplay_surface(
    source: &mut astra_player::NativeVnHostCommandSource,
    executor: &mut astra_player::PlayerHostCommandExecutor<astra_player::PlatformCommandSink>,
) -> Result<(), astra_platform::PlatformError> {
    let results = executor
        .execute_batch(
            source
                .prepare_surface_capture()
                .map_err(|error| player_platform_error("player.save.capture.prepare", error))?,
        )
        .await
        .map_err(|error| player_platform_error("player.save.capture", error))?;
    let (width, height, rgba8) = match results.as_slice() {
        [PlayerHostCommandResult::Captured {
            width,
            height,
            rgba8,
            ..
        }] => (*width, *height, rgba8.clone()),
        _ => {
            return Err(player_platform_error(
                "player.save.capture",
                "ASTRA_PLAYER_SAVE_CAPTURE_RESULT: platform returned an unexpected result",
            ));
        }
    };
    source
        .cache_gameplay_surface(width, height, rgba8)
        .map_err(|error| player_platform_error("player.save.capture.cache", error))
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) async fn execute_platform_load(
    source: &mut astra_player::NativeVnHostCommandSource,
    media: &mut astra_player::NativeVnProductMediaHost,
    executor: &mut astra_player::PlayerHostCommandExecutor<astra_player::PlatformCommandSink>,
    slot: &str,
) -> Result<(), astra_platform::PlatformError> {
    let results = executor
        .execute_batch(
            source
                .read_save(slot)
                .map_err(|error| player_platform_error("player.save.read.prepare", error))?,
        )
        .await
        .map_err(|error| player_platform_error("player.save.read", error))?;
    let bytes = match results.as_slice() {
        [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
        _ => {
            return Err(astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.save.read",
                "ASTRA_PLAYER_SAVE_RESULT_INVALID: platform returned an unexpected result",
            ));
        }
    };
    let present = source
        .load_product_session(slot, bytes, media, executor)
        .await
        .map_err(|error| player_platform_error("player.save.restore", error))?;
    executor
        .execute_batch(present)
        .await
        .map_err(|error| player_platform_error("player.save.present", error))?;
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) fn player_platform_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> astra_platform::PlatformError {
    astra_platform::PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(super) fn log_consumed_vn_step(
    player_sequence: u64,
    kind: &str,
    source: &astra_player_vn::NativeVnHostCommandSource,
) -> Result<(), astra_platform::PlatformError> {
    let evidence = source.last_step_evidence().ok_or_else(|| {
        astra_platform::PlatformError::new(
            astra_platform::PlatformErrorCode::InvalidState,
            "player.runtime.evidence",
            "ASTRA_PLAYER_VN_EVIDENCE_MISSING: consumed input has no runtime evidence",
        )
    })?;
    let coverage = if evidence.coverage_reached.is_empty() {
        "-".to_string()
    } else {
        evidence
            .coverage_reached
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    };
    let current_state_id = evidence.current_state_id.as_deref().unwrap_or("-");
    let pending_choice_ids = if evidence.pending_choice_ids.is_empty() {
        "-".to_string()
    } else {
        evidence.pending_choice_ids.join(",")
    };
    let terminal_route_ids = if evidence.terminal_route_ids.is_empty() {
        "-".to_string()
    } else {
        evidence
            .terminal_route_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    };
    tracing::trace!(
        event = "astra.player.input.consumed",
        session_id = source.session_id(),
        player_sequence,
        kind,
        "Player host consumed platform input"
    );
    tracing::trace!(
        event = "astra.player.vn.step",
        session_id = source.session_id(),
        player_sequence,
        fixed_step = evidence.fixed_step,
        coverage = %coverage,
        presentation_count = evidence.presentation_count,
        coverage_count = evidence.coverage_reached.len(),
        current_state_id,
        pending_choice_ids = %pending_choice_ids,
        terminal_route_ids = %terminal_route_ids,
        "Player host committed RuntimeWorld VN step"
    );
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub(super) fn run_bundled_game(
    _test_null_audio: bool,
    _preview_control: bool,
) -> Result<(), PlayerCliError> {
    Err("native AstraPlayer bundle host is unavailable on this platform".into())
}

pub(super) fn parse_usize_arg(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    option: &str,
) -> Result<usize, PlayerCliError> {
    let raw = args
        .next()
        .ok_or_else(|| format!("missing {option} value"))?;
    let value = raw
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| format!("invalid {option} value"))?;
    if value == 0 {
        return Err(format!("{option} must be non-zero").into());
    }
    Ok(value)
}

pub(super) fn write_json<T: serde::Serialize>(
    path: PathBuf,
    value: &T,
) -> Result<(), PlayerCliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
