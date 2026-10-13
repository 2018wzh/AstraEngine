use super::*;

impl NativeVnHeadlessSession {
    pub(super) async fn load(&mut self, slot: &str) -> Result<(), ProductHostError> {
        let profile_started = self.profile_started();
        let read = self
            .source()?
            .read_save(slot)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let result = self
            .executor
            .execute_batch(read)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let bytes = match result.as_slice() {
            [PlayerHostCommandResult::SaveRead { bytes }] => bytes.clone(),
            _ => {
                return Err(ProductHostError::Output(
                    "save read result is invalid".into(),
                ))
            }
        };
        let present = self
            .source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("runtime source is unavailable".into()))?
            .load_product_session(slot, &bytes, &mut self.media, &mut self.executor)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.executor
            .execute_batch(present)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.add_profile_duration(profile_started, |sample, duration| {
            sample.save_load_ns = sample.save_load_ns.saturating_add(duration);
        })?;
        self.flush_performance_sample(false)
    }

    pub(super) fn refresh_observations(&mut self) -> Result<(), ProductHostError> {
        let source = self.source.as_ref().ok_or_else(|| {
            ProductHostError::Output("runtime source is unavailable for observation".into())
        })?;
        let evidence = source.last_step_evidence().ok_or_else(|| {
            ProductHostError::Output("runtime step has no observation evidence".into())
        })?;
        let product = source
            .product_observation_evidence()
            .map_err(|error| binding("product.observe", error))?;
        self.observations = vec![
            hashed_observation(
                "runtime.state_hash",
                &(
                    &evidence.current_state_id,
                    &evidence.pending_wait_command_id,
                    &evidence.pending_wait_await_id,
                    &evidence.pending_choice_ids,
                    &evidence.terminal_route_ids,
                ),
            )?,
            hashed_observation("runtime.event_hash", &evidence.coverage_reached)?,
            hashed_observation("runtime.presentation_hash", &evidence.presentation_count)?,
            hashed_observation("vn.current_state", &evidence.current_state_id)?,
            hashed_observation("vn.pending_wait_command", &evidence.pending_wait_command_id)?,
            hashed_observation("vn.pending_wait_await_id", &evidence.pending_wait_await_id)?,
            hashed_observation("vn.pending_choices", &evidence.pending_choice_ids)?,
            hashed_observation("vn.terminal_routes", &evidence.terminal_route_ids)?,
            hashed_observation(
                "vn.route_terminal",
                &(!evidence.terminal_route_ids.is_empty()),
            )?,
            hashed_observation("vn.ui_profile", &product.ui_profile)?,
            hashed_observation("vn.locale", &product.locale)?,
            hashed_observation("vn.system_page", &product.active_system_page)?,
            hashed_observation("vn.focused_semantic_id", &product.focused_semantic_id)?,
            hashed_observation("vn.auto_enabled", &product.auto_enabled)?,
            hashed_observation("vn.skip_mode", &product.skip_mode)?,
            hashed_observation("vn.reading_mode", &product.reading_mode)?,
            hashed_observation("vn.audio_enabled", &product.audio_enabled)?,
            hashed_observation("vn.skip_allowed", &product.skip_allowed)?,
            hashed_observation("vn.system_config", &product.system_config)?,
            hashed_observation("vn.text_reveal_complete", &product.text_reveal_complete)?,
            hashed_observation("vn.backlog_count", &product.backlog_count)?,
            hashed_observation(
                "vn.occupied_save_slot_count",
                &product.occupied_save_slot_count,
            )?,
            hashed_observation("media.active_video", &self.media.has_active_video())?,
            hashed_observation("media.active_voice", &self.media.has_active_voice())?,
        ];
        Ok(())
    }
}

pub(super) fn batch_presents_scene(batch: &PlayerHostCommandBatch) -> bool {
    batch
        .commands
        .iter()
        .any(|command| matches!(command, PlayerHostCommand::PresentScene { .. }))
}

pub(super) fn presentation_substep_duration_ns(
    tick_duration_ns: u64,
    substeps: u32,
    substep: u32,
) -> u64 {
    let base = tick_duration_ns / u64::from(substeps);
    let remainder = tick_duration_ns % u64::from(substeps);
    base + u64::from(substep + 1 == substeps) * remainder
}

pub(super) fn hashed_observation(
    key: &str,
    value: &impl serde::Serialize,
) -> Result<Observation, ProductHostError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| ProductHostError::Output(error.to_string()))?;
    Ok(Observation {
        key: key.into(),
        value_hash: Hash256::from_sha256(&bytes).to_string(),
    })
}

pub(super) fn normalized(value: u16, extent: u32) -> f64 {
    f64::from(value) * f64::from(extent.saturating_sub(1)) / 65_535.0
}

pub(super) fn ui_point(pointer: (f64, f64)) -> UiPoint {
    UiPoint {
        x: pointer.0 as f32,
        y: pointer.1 as f32,
    }
}

pub(super) fn ui_button_state(state: ButtonState) -> UiButtonState {
    match state {
        ButtonState::Pressed => UiButtonState::Pressed,
        ButtonState::Released => UiButtonState::Released,
    }
}

pub(super) fn ui_pointer_button(button: astra_headless_protocol::PointerButton) -> UiPointerButton {
    match button {
        astra_headless_protocol::PointerButton::Primary => UiPointerButton::Primary,
        astra_headless_protocol::PointerButton::Secondary => UiPointerButton::Secondary,
        astra_headless_protocol::PointerButton::Middle => UiPointerButton::Middle,
        astra_headless_protocol::PointerButton::Back => UiPointerButton::Back,
        astra_headless_protocol::PointerButton::Forward => UiPointerButton::Forward,
        astra_headless_protocol::PointerButton::Other => UiPointerButton::Other(0),
    }
}

pub(super) fn ui_touch_phase(phase: TouchPhase) -> UiTouchPhase {
    match phase {
        TouchPhase::Started => UiTouchPhase::Started,
        TouchPhase::Moved => UiTouchPhase::Moved,
        TouchPhase::Ended => UiTouchPhase::Ended,
        TouchPhase::Cancelled => UiTouchPhase::Cancelled,
    }
}

pub(super) fn ui_navigation_action(control: GamepadControl) -> Option<UiNavigationAction> {
    match control {
        GamepadControl::South => Some(UiNavigationAction::Activate),
        GamepadControl::East => Some(UiNavigationAction::Cancel),
        GamepadControl::DpadUp => Some(UiNavigationAction::Up),
        GamepadControl::DpadDown => Some(UiNavigationAction::Down),
        GamepadControl::DpadLeft => Some(UiNavigationAction::Left),
        GamepadControl::DpadRight => Some(UiNavigationAction::Right),
        GamepadControl::LeftShoulder => Some(UiNavigationAction::PagePrevious),
        GamepadControl::RightShoulder => Some(UiNavigationAction::PageNext),
        _ => None,
    }
}

pub(super) fn canonical_ms(tick: u64) -> Result<u64, ProductHostError> {
    tick.checked_mul(astra_headless_protocol::TICK_DURATION_NS)
        .and_then(|value| value.checked_div(1_000_000))
        .ok_or_else(|| ProductHostError::Input("tick time overflowed".into()))
}

pub(super) fn requires_active_input(input: &PhysicalInput) -> bool {
    matches!(
        input,
        PhysicalInput::Keyboard { .. }
            | PhysicalInput::ImePreedit { .. }
            | PhysicalInput::ImeCommit { .. }
            | PhysicalInput::PointerMove { .. }
            | PhysicalInput::PointerButton { .. }
            | PhysicalInput::Wheel { .. }
            | PhysicalInput::Touch { .. }
            | PhysicalInput::GamepadInput { .. }
    )
}

pub(super) fn binding(operation: &str, error: impl std::fmt::Display) -> ProductHostError {
    ProductHostError::Binding(format!("{operation}: {error}"))
}
