use super::*;

impl AstraEmuManagerController {
    pub(super) fn update_window_state(&mut self, mut state: WindowState) -> Result<(), String> {
        if state.width == 0 || state.height == 0 {
            // Keep the last drawable extent while the platform surface is
            // suspended; OS visibility remains an independent fact.
            let previous = self.window_state.ok_or("ASTRA_EMU_WINDOW_NOT_DRAWABLE")?;
            state.width = previous.width;
            state.height = previous.height;
            state.suspended = true;
        }
        state.validate().map_err(|error| error.to_string())?;
        if let Some(previous) = self.window_state {
            if previous.scale_factor != state.scale_factor {
                self.handle_physical_event(FamilyEvent::WindowScale {
                    factor: state.scale_factor,
                })?;
            }
            if previous.focused != state.focused {
                self.handle_physical_event(FamilyEvent::WindowFocused {
                    focused: state.focused,
                })?;
            }
            if previous.visible != state.visible {
                self.handle_physical_event(FamilyEvent::WindowVisibility {
                    visible: state.visible,
                })?;
            }
            if previous.suspended != state.suspended {
                self.handle_physical_event(FamilyEvent::WindowSuspended {
                    suspended: state.suspended,
                })?;
            }
            if (previous.width, previous.height) != (state.width, state.height) {
                self.handle_physical_event(FamilyEvent::WindowResized {
                    width: state.width,
                    height: state.height,
                })?;
            }
        }
        self.window_state = Some(state);
        Ok(())
    }

    pub(super) fn handle_physical_event(&mut self, event: FamilyEvent) -> Result<(), String> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.closing.is_some())
        {
            return Ok(());
        }
        if matches!(
            event,
            FamilyEvent::WindowFocused { focused: false }
                | FamilyEvent::WindowVisibility { visible: false }
                | FamilyEvent::WindowSuspended { suspended: true }
        ) {
            self.release_inputs()?;
        }
        if self.pending_events.len() >= astra_emu_manager_core::model::MAX_INPUT_EVENTS {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        if let FamilyEvent::Key {
            code,
            state,
            modifiers,
            ..
        } = &event
        {
            self.key_modifiers = *modifiers;
            if *state == KeyState::Released {
                self.physical_keys.retain(|held| held != code);
            } else if !self.physical_keys.contains(code) {
                self.physical_keys.push(*code);
            }
        }
        let lifecycle = matches!(
            event,
            FamilyEvent::WindowFocused { .. }
                | FamilyEvent::WindowVisibility { .. }
                | FamilyEvent::WindowSuspended { .. }
        );
        let closing = matches!(event, FamilyEvent::WindowCloseRequested);
        self.pending_events.push(event);
        if closing {
            self.close_active(PlaySessionEndReason::Shutdown)?;
        } else if lifecycle {
            self.flush_owner_events()?;
        }
        Ok(())
    }
    pub(super) fn set_physical_control(&mut self, control: &str, pressed: bool) {
        if pressed {
            self.held_controls.insert(control.to_owned());
        } else {
            self.held_controls.remove(control);
        }
        match control {
            "shift" | "shift_left" | "shift_right" => self.key_modifiers.shift = pressed,
            "control" | "control_left" | "control_right" => self.key_modifiers.control = pressed,
            "alt" | "alt_left" | "alt_right" => self.key_modifiers.alt = pressed,
            "super" | "super_left" | "super_right" | "meta" => {
                self.key_modifiers.super_key = pressed
            }
            _ => {}
        }
    }

    pub(super) fn game_input(
        &mut self,
        control: &str,
        pressed: bool,
        value: f32,
    ) -> Result<(), String> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.closing.is_some())
        {
            return Err("ASTRA_EMU_FAMILY_SESSION_NOT_ACTIVE".into());
        }
        if self.pending_events.len() >= astra_emu_manager_core::model::MAX_INPUT_EVENTS {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        let mut event = match control {
            "pointer.x" => {
                if !value.is_finite() {
                    return Err("ASTRA_EMU_INPUT_POINTER_INVALID".into());
                }
                self.pointer_position.0 = value;
                FamilyEvent::PointerMove {
                    x: self.pointer_position.0,
                    y: self.pointer_position.1,
                    displayed: None,
                }
            }
            "pointer.y" => {
                if !value.is_finite() {
                    return Err("ASTRA_EMU_INPUT_POINTER_INVALID".into());
                }
                self.pointer_position.1 = value;
                FamilyEvent::PointerMove {
                    x: self.pointer_position.0,
                    y: self.pointer_position.1,
                    displayed: None,
                }
            }
            "pointer.primary" | "pointer.secondary" | "pointer.middle" => {
                if pressed {
                    self.held_controls.insert(control.into());
                } else {
                    self.held_controls.remove(control);
                }
                FamilyEvent::PointerButton {
                    button: pointer_button(control).expect("pointer control checked above"),
                    state: if pressed {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    displayed: None,
                }
            }
            "wheel" => {
                if !value.is_finite() {
                    return Err("ASTRA_EMU_INPUT_WHEEL_INVALID".into());
                }
                FamilyEvent::Wheel {
                    delta_x: 0.0,
                    delta_y: value,
                }
            }
            _ => {
                let code = key_code(control)
                    .ok_or_else(|| "ASTRA_EMU_INPUT_CONTROL_INVALID".to_owned())?;
                if pressed {
                    self.set_physical_control(control, true);
                }
                let event = FamilyEvent::Key {
                    logical: None,
                    code,
                    state: if pressed {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    modifiers: self.key_modifiers,
                };
                if !pressed {
                    self.set_physical_control(control, false);
                }
                event
            }
        };
        if matches!(
            &event,
            FamilyEvent::PointerMove { .. } | FamilyEvent::PointerButton { .. }
        ) {
            let Some((identity, info)) = self.mailbox.displayed()? else {
                return Ok(());
            };
            match &mut event {
                FamilyEvent::PointerMove { x, y, displayed } => {
                    let viewport = stage_renderer::content_viewport_for_output(
                        info.logical_width,
                        info.logical_height,
                        info.width,
                        info.height,
                        info.width,
                        info.height,
                    )?;
                    *x = viewport.x as f32 + *x * viewport.width as f32 / info.logical_width as f32;
                    *y = viewport.y as f32
                        + *y * viewport.height as f32 / info.logical_height as f32;
                    *displayed = Some(identity);
                }
                FamilyEvent::PointerButton { displayed, .. } => *displayed = Some(identity),
                _ => {}
            }
        }
        self.pending_events.push(event);
        Ok(())
    }

    pub(super) fn release_inputs(&mut self) -> Result<(), String> {
        let held = self.held_controls.iter().cloned().collect::<Vec<_>>();
        let required = held.len()
            + self.physical_keys.len()
            + held
                .iter()
                .filter(|control| pointer_button(control).is_some())
                .count();
        if self.pending_events.len().saturating_add(required)
            > astra_emu_manager_core::model::MAX_INPUT_EVENTS
        {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        for code in std::mem::take(&mut self.physical_keys) {
            self.pending_events.push(FamilyEvent::Key {
                logical: None,
                code,
                state: KeyState::Released,
                modifiers: self.key_modifiers,
            });
        }
        for control in held {
            let event = if let Some(button) = pointer_button(&control) {
                if let Some((identity, info)) = self.mailbox.displayed()? {
                    self.pending_events.push(FamilyEvent::PointerMove {
                        x: self.pointer_position.0 * info.width as f32 / info.logical_width as f32,
                        y: self.pointer_position.1 * info.height as f32
                            / info.logical_height as f32,
                        displayed: Some(identity),
                    });
                }
                FamilyEvent::PointerButton {
                    button,
                    state: KeyState::Released,
                    displayed: self.mailbox.displayed_identity()?,
                }
            } else {
                let code = key_code(&control)
                    .ok_or_else(|| "ASTRA_EMU_INPUT_CONTROL_INVALID".to_owned())?;
                FamilyEvent::Key {
                    logical: None,
                    code,
                    state: KeyState::Released,
                    modifiers: self.key_modifiers,
                }
            };
            self.pending_events.push(event);
            self.set_physical_control(&control, false);
        }
        self.held_controls.clear();
        self.key_modifiers = KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        };
        Ok(())
    }

    /// Mapping for the gamepad pump: the running game's mapping, else the global one.
    pub(super) fn input_mapping(&self) -> astra_emu_manager_core::InputMapping {
        match (&self.active, &self.session_mapping) {
            (Some(_), Some(mapping)) => mapping.clone(),
            _ => self.input_mapping.clone(),
        }
    }

    pub(super) fn game_input_mapping(
        &self,
        game_id: &str,
    ) -> Result<astra_emu_manager_core::InputMapping, String> {
        Ok(self
            .library
            .game_settings(game_id)
            .map_err(|e| e.to_string())?
            .and_then(|settings| settings.input_mapping)
            .unwrap_or_else(|| self.input_mapping.clone()))
    }

    fn update_global_mapping(
        &mut self,
        change: impl FnOnce(&mut astra_emu_manager_core::InputMapping) -> Result<(), String>,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let mut mapping = self.input_mapping.clone();
        change(&mut mapping)?;
        self.library
            .save_input_mapping(&mapping)
            .map_err(|e| e.to_string())?;
        self.input_mapping = mapping;
        self.model()
    }

    fn update_game_settings(
        &mut self,
        game_id: &str,
        change: impl FnOnce(&mut astra_emu_manager_core::GameSettings) -> Result<(), String>,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let mut settings = self
            .library
            .game_settings(game_id)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        change(&mut settings)?;
        self.library
            .set_game_settings(game_id, &settings)
            .map_err(|e| e.to_string())?;
        self.model()
    }

    pub(super) fn set_gamepad_enabled(&mut self, enabled: bool) -> Result<ManagerViewModel, String> {
        self.update_global_mapping(|mapping| {
            mapping.gamepad_enabled = enabled;
            Ok(())
        })
    }

    pub(super) fn set_gamepad_deadzone(
        &mut self,
        deadzone: &str,
    ) -> Result<ManagerViewModel, String> {
        let deadzone =
            GamepadDeadzone::parse(deadzone).ok_or("ASTRA_EMU_INPUT_DEADZONE_INVALID")?;
        self.update_global_mapping(|mapping| {
            mapping.deadzone = deadzone;
            Ok(())
        })
    }

    pub(super) fn set_gamepad_binding(
        &mut self,
        button: &str,
        key: &str,
    ) -> Result<ManagerViewModel, String> {
        self.update_global_mapping(|mapping| bind(mapping, button, key))
    }

    pub(super) fn reset_gamepad_bindings(&mut self) -> Result<ManagerViewModel, String> {
        self.update_global_mapping(|mapping| {
            mapping.gamepad = default_vn_preset().gamepad;
            Ok(())
        })
    }

    pub(super) fn set_game_input_override(
        &mut self,
        game_id: &str,
        enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        let global = self.input_mapping.clone();
        self.update_game_settings(game_id, |settings| {
            settings.input_mapping = enabled.then_some(global);
            Ok(())
        })
    }

    pub(super) fn set_game_gamepad_binding(
        &mut self,
        game_id: &str,
        button: &str,
        key: &str,
    ) -> Result<ManagerViewModel, String> {
        let global = self.input_mapping.clone();
        self.update_game_settings(game_id, |settings| {
            bind(settings.input_mapping.get_or_insert(global), button, key)
        })
    }
}

fn bind(
    mapping: &mut astra_emu_manager_core::InputMapping,
    button: &str,
    key: &str,
) -> Result<(), String> {
    let button =
        GamepadInput::parse(button).ok_or_else(|| "ASTRA_EMU_GAMEPAD_BUTTON_INVALID".to_owned())?;
    if key.is_empty() {
        mapping.gamepad.remove(&button);
    } else if key_code(key).is_some() {
        mapping.gamepad.insert(button, key.into());
    } else {
        return Err("ASTRA_EMU_INPUT_KEY_INVALID".into());
    }
    Ok(())
}

fn pointer_button(control: &str) -> Option<astra_emu_manager_core::model::PointerButton> {
    use astra_emu_manager_core::model::PointerButton;
    match control {
        "pointer.primary" => Some(PointerButton::Primary),
        "pointer.secondary" => Some(PointerButton::Secondary),
        "pointer.middle" => Some(PointerButton::Middle),
        _ => None,
    }
}
