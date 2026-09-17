use super::*;
use crate::MusicaSystemPage;

impl MusicaSession {
    pub(super) fn backlog_event(&mut self, event: &FamilyEvent) -> FamilyResult<bool> {
        let active = self.vm.state().system_ui.page == MusicaSystemPage::Backlog;
        if !active {
            if matches!(
                event,
                FamilyEvent::Key {
                    code: KeyCode::PageUp,
                    state: KeyState::Pressed,
                    ..
                }
            ) && !self.vm.state().backlog.is_empty()
                && !self.finished
            {
                self.cancel_text()?;
                self.vm.open_backlog().map_err(vm_error)?;
                self.input_pending = false;
                tracing::debug!(
                    event = "astra.emu.musica.backlog.opened",
                    entries = self.vm.state().backlog.len()
                );
                return Ok(true);
            }
            return Ok(false);
        }
        match event {
            FamilyEvent::Key {
                code: KeyCode::Escape | KeyCode::Backspace,
                state: KeyState::Pressed,
                ..
            } => {
                self.vm.close_backlog().map_err(vm_error)?;
                self.input_pending = false;
                tracing::debug!(event = "astra.emu.musica.backlog.closed");
            }
            FamilyEvent::Key {
                code: KeyCode::PageUp | KeyCode::ArrowUp,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_backlog(-1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::PageDown | KeyCode::ArrowDown,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_backlog(1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::Enter,
                state: KeyState::Pressed,
                ..
            }
            | FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } => {
                let commands = self.vm.replay_backlog_voice().map_err(vm_error)?;
                self.audio.apply(commands)?;
                tracing::debug!(event = "astra.emu.musica.backlog.voice");
            }
            FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested
            | FamilyEvent::Key {
                code: KeyCode::F5 | KeyCode::F9 | KeyCode::ControlLeft | KeyCode::ControlRight,
                ..
            } => return Ok(false),
            _ => {}
        }
        Ok(true)
    }
}
