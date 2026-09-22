use super::*;

/// Physical buttons are distinct; releasing Space must not release held Enter.
#[derive(Default)]
pub(super) struct InputState {
    confirm: u8,
}
impl InputState {
    pub fn event(&mut self, vm: &mut CmvsPs2aVmState, event: &FamilyEvent) {
        let binding = match event {
            FamilyEvent::Key {
                code: KeyCode::Enter,
                state,
                ..
            } => Some((1, state)),
            FamilyEvent::Key {
                code: KeyCode::Space,
                state,
                ..
            } => Some((2, state)),
            FamilyEvent::Key {
                code: KeyCode::NumpadEnter,
                state,
                ..
            } => Some((4, state)),
            FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            } => Some((8, state)),
            _ => None,
        };
        if let Some((bit, state)) = binding {
            let was_held = self.confirm != 0;
            match state {
                KeyState::Pressed => self.confirm |= bit,
                KeyState::Released => self.confirm &= !bit,
                KeyState::Repeated => return,
            }
            vm.input_confirm_held = self.confirm != 0;
            vm.input_advance_press |= !was_held && vm.input_confirm_held;
            vm.input_advance_release |= was_held && !vm.input_confirm_held;
        }
        if matches!(
            event,
            FamilyEvent::WindowFocused { focused: false }
                | FamilyEvent::WindowSuspended { suspended: true }
        ) {
            self.confirm = 0;
            vm.input_confirm_held = false;
            // Focus loss is cancellation, never a synthetic story advance.
            vm.input_advance_press = false;
            vm.input_advance_release = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode, state: KeyState) -> FamilyEvent {
        FamilyEvent::Key {
            code,
            state,
            modifiers: KeyModifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
        }
    }
    #[test]
    fn release_between_ticks_survives_until_native_consume() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Released));
        assert!(vm.input_advance_release);
        assert!(!vm.input_confirm_held);
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Repeated));
        assert!(vm.input_advance_release);
    }
    #[test]
    fn independent_buttons_and_focus_loss_do_not_advance() {
        let mut input = InputState::default();
        let mut vm = CmvsPs2aVmState::new(0);
        input.event(&mut vm, &key(KeyCode::Enter, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::Space, KeyState::Pressed));
        input.event(&mut vm, &key(KeyCode::Space, KeyState::Released));
        assert!(vm.input_confirm_held);
        assert!(!vm.input_advance_release);
        input.event(&mut vm, &FamilyEvent::WindowFocused { focused: false });
        assert!(!vm.input_confirm_held);
        assert!(!vm.input_advance_release);
        assert!(!vm.input_advance_press);
    }
}
