use super::*;
use crate::{storage::SAVE_PAGE_WIDTH, MusicaSystemPage};

impl MusicaSession {
    pub(super) fn is_save_page(&self) -> bool {
        matches!(
            self.vm.state().system_ui.page,
            MusicaSystemPage::Save | MusicaSystemPage::Load
        )
    }
    fn refresh_save_cards(&mut self) -> FamilyResult<()> {
        let base = self.vm.state().system_ui.focus_index / SAVE_PAGE_WIDTH * SAVE_PAGE_WIDTH;
        let mut cards = Vec::new();
        for slot in base..base + SAVE_PAGE_WIDTH {
            if let Some(card) = self.storage.card(slot, self.game)? {
                cards.push((slot, card));
            }
        }
        self.save_cards = cards;
        Ok(())
    }
    fn close_save_page(&mut self) -> FamilyResult<()> {
        self.vm.close_gameplay_system_page().map_err(vm_error)?;
        self.gameplay_frame = None;
        self.save_cards.clear();
        self.input_pending = false;
        tracing::debug!(event = "astra.emu.musica.save_page.closed");
        Ok(())
    }
    fn activate_save_slot(&mut self) -> FamilyResult<()> {
        let slot = self.vm.state().system_ui.focus_index;
        if self.vm.state().system_ui.page == MusicaSystemPage::Save {
            self.save(slot)?;
            self.refresh_save_cards()?;
        } else {
            // Empty cards remain on the page; malformed existing cards fail during refresh.
            if !self.save_cards.iter().any(|(id, _)| *id == slot) {
                return Ok(());
            }
            self.load(slot)?;
        }
        Ok(())
    }
    pub(super) fn save_page_event(&mut self, event: &FamilyEvent) -> FamilyResult<bool> {
        if !self.is_save_page() {
            // Family physical shortcuts replace the old Host's semantic menu requests.
            if self.vm.state().system_ui.page == MusicaSystemPage::None
                && self.vm.state().wait.is_some()
                && !self.finished
            {
                if let FamilyEvent::Key {
                    code: KeyCode::F6 | KeyCode::F8,
                    state: KeyState::Pressed,
                    ..
                } = event
                {
                    self.cancel_text()?;
                    self.gameplay_frame = Some(self.scene.pixels.clone());
                    if matches!(
                        event,
                        FamilyEvent::Key {
                            code: KeyCode::F6,
                            ..
                        }
                    ) {
                        self.vm.open_save_page().map_err(vm_error)?;
                    } else {
                        self.vm.open_load_page().map_err(vm_error)?;
                    }
                    self.clear_input();
                    self.refresh_save_cards()?;
                    tracing::debug!(event = "astra.emu.musica.save_page.opened", page = ?self.vm.state().system_ui.page);
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let previous_page = self.vm.state().system_ui.focus_index / SAVE_PAGE_WIDTH;
        match event {
            FamilyEvent::Key {
                code: KeyCode::Escape,
                state: KeyState::Pressed,
                ..
            } => self.close_save_page()?,
            FamilyEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => self.activate_save_slot()?,
            FamilyEvent::Key {
                code: KeyCode::ArrowUp,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_save_focus(-1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::ArrowDown,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_save_focus(1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::ArrowLeft,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_save_page(-1).map_err(vm_error)?,
            FamilyEvent::Key {
                code: KeyCode::ArrowRight,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_save_page(1).map_err(vm_error)?,
            FamilyEvent::PointerMove { x, y } => self.pointer = Some((*x, *y)),
            FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } => {
                if let Some((x, y)) = self.pointer {
                    if (650.0..720.0).contains(&y) && (462.0..818.0).contains(&x) {
                        if x < 578.0 {
                            self.vm.move_save_page(-1).map_err(vm_error)?;
                        } else if x < 700.0 {
                            self.vm.move_save_page(1).map_err(vm_error)?;
                        } else {
                            self.close_save_page()?;
                        }
                    } else if let Some(index) = slot_at(x, y) {
                        self.vm
                            .set_save_focus(previous_page * SAVE_PAGE_WIDTH + index)
                            .map_err(vm_error)?;
                        self.activate_save_slot()?;
                    }
                }
            }
            FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested => return Ok(false),
            _ => {}
        }
        if self.is_save_page()
            && previous_page != self.vm.state().system_ui.focus_index / SAVE_PAGE_WIDTH
        {
            self.refresh_save_cards()?;
        }
        Ok(true)
    }
}
fn slot_at(x: f32, y: f32) -> Option<u32> {
    (0..10).find(|index| {
        let (left, top) = crate::scene::save_pages::slot_position(*index);
        (left as f32..(left + 344) as f32).contains(&x)
            && (top as f32..(top + 98) as f32).contains(&y)
    })
}
