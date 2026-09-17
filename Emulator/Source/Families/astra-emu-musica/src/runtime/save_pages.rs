use super::*;
use crate::storage::{MANUAL_SAVE_FIRST_SLOT, SAVE_MAX_SLOTS, SAVE_PAGE_COUNT, SAVE_PAGE_WIDTH};

impl MusicaVm {
    pub fn open_save_page(&mut self) -> Result<(), MusicaRuntimeError> {
        self.open_slot_page(MusicaSystemPage::Save)
    }

    pub fn open_load_page(&mut self) -> Result<(), MusicaRuntimeError> {
        self.open_slot_page(MusicaSystemPage::Load)
    }

    fn open_slot_page(&mut self, page: MusicaSystemPage) -> Result<(), MusicaRuntimeError> {
        if self.state.terminal
            || self.state.system_ui.page != MusicaSystemPage::None
            || self.state.wait.is_none()
        {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.page = page;
        self.state.system_ui.focus_index = MANUAL_SAVE_FIRST_SLOT;
        Ok(())
    }

    pub fn close_gameplay_system_page(&mut self) -> Result<(), MusicaRuntimeError> {
        self.require_slot_page()?;
        self.state.system_ui.page = MusicaSystemPage::None;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    fn require_slot_page(&self) -> Result<(), MusicaRuntimeError> {
        if self.state.terminal
            || !matches!(
                self.state.system_ui.page,
                MusicaSystemPage::Save | MusicaSystemPage::Load
            )
            || self.state.system_ui.focus_index >= SAVE_MAX_SLOTS
        {
            return Err(MusicaRuntimeError::State);
        }
        Ok(())
    }

    pub fn move_save_page(&mut self, direction: i32) -> Result<(), MusicaRuntimeError> {
        self.require_slot_page()?;
        if direction == 0 {
            return Err(MusicaRuntimeError::State);
        }
        let page = self.state.system_ui.focus_index / SAVE_PAGE_WIDTH;
        let slot = self.state.system_ui.focus_index % SAVE_PAGE_WIDTH;
        let page = if direction < 0 {
            (page + SAVE_PAGE_COUNT - 1) % SAVE_PAGE_COUNT
        } else {
            (page + 1) % SAVE_PAGE_COUNT
        };
        self.state.system_ui.focus_index = page * SAVE_PAGE_WIDTH + slot;
        Ok(())
    }

    pub fn set_save_focus(&mut self, slot: u32) -> Result<(), MusicaRuntimeError> {
        self.require_slot_page()?;
        if slot >= SAVE_MAX_SLOTS {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.focus_index = slot;
        Ok(())
    }

    pub fn move_save_focus(&mut self, direction: i32) -> Result<(), MusicaRuntimeError> {
        self.require_slot_page()?;
        if direction == 0 {
            return Err(MusicaRuntimeError::State);
        }
        let current = self.state.system_ui.focus_index;
        self.state.system_ui.focus_index = if direction < 0 {
            (current + SAVE_MAX_SLOTS - 1) % SAVE_MAX_SLOTS
        } else {
            (current + 1) % SAVE_MAX_SLOTS
        };
        Ok(())
    }
}

pub(super) fn validate_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if matches!(
        state.system_ui.page,
        MusicaSystemPage::Save | MusicaSystemPage::Load
    ) && state.system_ui.focus_index >= SAVE_MAX_SLOTS
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_pages_open_at_manual_slots_wrap_and_preserve_the_story_wait() {
        let bytes = b".message 1   Hello\r\n.end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(bytes),
            crate::parse_sc(bytes, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap();
        assert!(vm.open_save_page().is_err());
        vm.step(1).unwrap();
        let wait = vm.state().wait.clone();
        vm.open_save_page().unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 20);
        assert!(vm.open_load_page().is_err());
        vm.set_save_focus(3).unwrap();
        vm.move_save_page(-1).unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 93);
        vm.move_save_page(1).unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 3);
        vm.set_save_focus(99).unwrap();
        vm.move_save_focus(1).unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 0);
        vm.move_save_focus(-1).unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 99);
        assert!(vm.set_save_focus(100).is_err());
        assert!(vm.move_save_page(0).is_err());
        assert_eq!(vm.state().system_ui.focus_index, 99);
        assert_eq!(vm.state().wait, wait);
        let bytes = vm.encode_native_save().unwrap();
        assert_eq!(
            MusicaVm::decode_native_save(&bytes)
                .unwrap()
                .system_ui
                .focus_index,
            99
        );
        let mut corrupt = MusicaVm::decode_native_save(&bytes).unwrap();
        corrupt.system_ui.focus_index = 100;
        assert!(MusicaVm::decode_native_save(&postcard::to_allocvec(&corrupt).unwrap()).is_err());
        vm.close_gameplay_system_page().unwrap();
        vm.open_load_page().unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 20);
    }
}
