use super::*;
use crate::MusicaSystemPage;
impl MusicaSession {
    pub(super) fn start_menu_script(&mut self, uri: String) -> FamilyResult<()> {
        let loaded = crate::script_loader::load_script(&self.archive, &uri, self.primary_encoding)?;
        let generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_TEXT_SEQUENCE",
                "text generation overflowed",
            )
        })?;
        self.cancel_text()?;
        if let Some(service) = &self.replacement {
            service.reset(TextResetReason::NewGame).into_result()?;
        }
        self.audio.restore(Vec::new())?;
        self.vm
            .start_menu_script(uri, loaded.hash, loaded.script)
            .map_err(vm_error)?;
        self.scene
            .set_text_encoding(self.vm.state().script_encoding);
        self.generation = generation;
        self.message = None;
        self.voice_duration = None;
        self.phase = 0;
        self.wait_ns = 0;
        self.title_focus = None;
        self.last_quick_save_pc_line = None;
        self.clear_input();
        tracing::info!(event = "astra.emu.musica.title.start");
        Ok(())
    }
    fn activate_title(&mut self) -> FamilyResult<()> {
        match (
            self.vm.title_variant(),
            self.vm.state().system_ui.focus_index,
        ) {
            (_, 0) => self.start_menu_script(self.entry_uri.clone()),
            (_, 1) => {
                self.vm.open_title_load().map_err(vm_error)?;
                self.load_from_title = true;
                self.clear_input();
                self.refresh_save_cards()
            }
            (_, 2) => Err(error(
                "ASTRA_EMU_MUSICA_CONFIG_PAGE_UNAVAILABLE",
                "native configuration page is not integrated yet",
            )),
            (2, 3) => self
                .vm
                .set_gallery_page(MusicaSystemPage::Memories, 0)
                .map_err(vm_error),
            (_, 3) | (2, 4) => {
                self.finished = true;
                Ok(())
            }
            _ => Err(error("ASTRA_EMU_MUSICA_TITLE_FOCUS", "invalid title focus")),
        }
    }
    pub(super) fn title_event(&mut self, event: &FamilyEvent) -> FamilyResult<bool> {
        if self.vm.state().system_ui.page != MusicaSystemPage::Title {
            return Ok(false);
        }
        match event {
            FamilyEvent::Key {
                code: KeyCode::ArrowUp | KeyCode::ArrowDown,
                state: KeyState::Pressed,
                ..
            } => {
                let count = crate::runtime::title::rows(self.vm.title_variant()).len() as u32;
                let offset = if matches!(
                    event,
                    FamilyEvent::Key {
                        code: KeyCode::ArrowUp,
                        ..
                    }
                ) {
                    count - 1
                } else {
                    1
                };
                let focus = (self.vm.state().system_ui.focus_index + offset) % count;
                self.vm.set_title_focus(focus).map_err(vm_error)?;
                self.title_focus = Some(focus);
            }
            FamilyEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => self.activate_title()?,
            FamilyEvent::PointerMove { x, y } => {
                self.pointer = Some((*x, *y));
                self.title_focus = crate::runtime::title::focus_at(self.vm.title_variant(), *x, *y);
                if let Some(focus) = self.title_focus {
                    self.vm.set_title_focus(focus).map_err(vm_error)?;
                }
            }
            FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } => {
                if let Some((x, y)) = self.pointer {
                    if let Some(focus) =
                        crate::runtime::title::focus_at(self.vm.title_variant(), x, y)
                    {
                        self.vm.set_title_focus(focus).map_err(vm_error)?;
                        self.activate_title()?;
                    }
                }
            }
            FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested => return Ok(false),
            _ => {}
        }
        Ok(true)
    }
}
