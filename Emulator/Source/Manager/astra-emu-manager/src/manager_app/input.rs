use super::*;

impl AstraEmuManagerController {
    pub(super) fn update_window_state(&mut self, mut state: WindowState) -> Result<(), String> {
        if state.width == 0 || state.height == 0 {
            // A minimized surface has no drawable extent. Preserve the last
            // real extent and communicate the visibility change separately.
            let previous = self.window_state.ok_or("ASTRA_EMU_WINDOW_NOT_DRAWABLE")?;
            state.width = previous.width;
            state.height = previous.height;
            state.visible = false;
        }
        state.validate().map_err(|error| error.to_string())?;
        if let Some(previous) = self.window_state {
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
        if self.active.is_none() {
            return Ok(());
        }
        if matches!(
            event,
            FamilyEvent::WindowFocused { focused: false }
                | FamilyEvent::WindowVisibility { visible: false }
        ) {
            self.release_inputs()?;
        }
        if self.pending_events.len() >= astra_emu_family_api::MAX_EVENTS_PER_ADVANCE {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        if let FamilyEvent::Key {
            code,
            state,
            modifiers,
        } = &event
        {
            self.key_modifiers = *modifiers;
            if *state == KeyState::Released {
                self.physical_keys.retain(|held| held != code);
            } else if !self.physical_keys.contains(code) {
                self.physical_keys.push(*code);
            }
        }
        let closing = matches!(event, FamilyEvent::WindowCloseRequested);
        self.pending_events.push(event);
        if closing {
            let events = std::mem::take(&mut self.pending_events);
            let result = self
                .active
                .as_mut()
                .expect("active session checked above")
                .advance(0, &events);
            let cleanup = self.close_active(PlaySessionEndReason::Shutdown);
            result.and(cleanup)?;
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
        if self.active.is_none() {
            return Err("ASTRA_EMU_FAMILY_SESSION_NOT_ACTIVE".into());
        }
        if self.pending_events.len() >= astra_emu_family_api::MAX_EVENTS_PER_ADVANCE {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        let event = match control {
            "pointer.x" => {
                if !value.is_finite() {
                    return Err("ASTRA_EMU_INPUT_POINTER_INVALID".into());
                }
                self.pointer_position.0 = value;
                FamilyEvent::PointerMove {
                    x: self.pointer_position.0,
                    y: self.pointer_position.1,
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
        self.pending_events.push(event);
        Ok(())
    }

    pub(super) fn release_inputs(&mut self) -> Result<(), String> {
        let held = self.held_controls.iter().cloned().collect::<Vec<_>>();
        let required = held.len() + self.physical_keys.len();
        if self.pending_events.len().saturating_add(required)
            > astra_emu_family_api::MAX_EVENTS_PER_ADVANCE
        {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        for code in std::mem::take(&mut self.physical_keys) {
            self.pending_events.push(FamilyEvent::Key {
                code,
                state: KeyState::Released,
                modifiers: self.key_modifiers,
            });
        }
        for control in held {
            let event = if let Some(button) = pointer_button(&control) {
                FamilyEvent::PointerButton {
                    button,
                    state: KeyState::Released,
                }
            } else {
                let code = key_code(&control)
                    .ok_or_else(|| "ASTRA_EMU_INPUT_CONTROL_INVALID".to_owned())?;
                FamilyEvent::Key {
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

    fn configure_gamepad(&mut self, enabled: bool, deadzone: &str) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_INPUT_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        let deadzone =
            GamepadDeadzone::parse(deadzone).ok_or("ASTRA_EMU_INPUT_DEADZONE_INVALID")?;
        self.input_mapping.gamepad_enabled = enabled;
        self.input_mapping.deadzone = deadzone;
        Ok(())
    }

    pub(super) fn load_game_input_mapping(&mut self, game_id: &str) -> Result<(), String> {
        let settings = self
            .library
            .game_settings(game_id)
            .map_err(|e| e.to_string())?;
        self.input_mapping = match settings.and_then(|s| s.input_mapping) {
            Some(mapping) => mapping,
            None => self
                .library
                .load_input_mapping()
                .map_err(|e| e.to_string())?
                .unwrap_or_else(default_vn_preset),
        };
        Ok(())
    }

    pub(super) fn save_input_config(
        &mut self,
        enabled: bool,
        deadzone: &str,
    ) -> Result<(), String> {
        self.configure_gamepad(enabled, deadzone)?;
        self.library
            .save_input_mapping(&self.input_mapping)
            .map_err(|e| e.to_string())
    }

    pub(super) fn input_mapping(&self) -> astra_emu_manager_core::InputMapping {
        self.input_mapping.clone()
    }

    pub(super) fn set_gamepad_binding(
        &mut self,
        button_id: &str,
        key_name: &str,
    ) -> Result<(), String> {
        let button = GamepadInput::parse(button_id)
            .ok_or_else(|| "ASTRA_EMU_GAMEPAD_BUTTON_INVALID".to_owned())?;
        if !key_name.is_empty() && key_code(key_name).is_none() {
            return Err("ASTRA_EMU_INPUT_KEY_INVALID".into());
        }
        if self.active.is_some() {
            return Err("ASTRA_EMU_INPUT_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        if key_name.is_empty() {
            self.input_mapping.gamepad.remove(&button);
        } else {
            self.input_mapping.gamepad.insert(button, key_name.into());
        }
        Ok(())
    }

    pub(super) fn reset_gamepad_mapping(&mut self) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_INPUT_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        self.input_mapping = default_vn_preset();
        Ok(())
    }

    pub(super) fn save_per_game_input_mapping(
        &mut self,
        enabled: bool,
        deadzone: &str,
    ) -> Result<ManagerViewModel, String> {
        self.configure_gamepad(enabled, deadzone)?;
        let game = self.selected_game()?;
        let mut settings = self
            .library
            .game_settings(&game.game_id)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        settings.input_mapping = Some(self.input_mapping.clone());
        self.library
            .set_game_settings(&game.game_id, &settings)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn clear_per_game_input_mapping(&mut self) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_INPUT_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        let game = self.selected_game()?;
        self.library
            .clear_game_settings(&game.game_id)
            .map_err(|error| error.to_string())?;
        self.load_game_input_mapping(&game.game_id)?;
        self.model()
    }
}

fn pointer_button(control: &str) -> Option<astra_emu_family_api::PointerButton> {
    use astra_emu_family_api::PointerButton;
    match control {
        "pointer.primary" => Some(PointerButton::Primary),
        "pointer.secondary" => Some(PointerButton::Secondary),
        "pointer.middle" => Some(PointerButton::Middle),
        _ => None,
    }
}
