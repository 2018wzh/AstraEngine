use super::*;
pub(crate) fn variant(state: &MusicaRuntimeState) -> u8 {
    let clear = |flag: &str| state.global_variables.get(flag) == Some(&1);
    if clear("TOHKA_CLEAR") {
        2
    } else if ["AYAME_CLEAR", "SUI_CLEAR", "REN_CLEAR"]
        .into_iter()
        .all(clear)
    {
        1
    } else {
        0
    }
}
pub(crate) fn rows(variant: u8) -> &'static [i32] {
    if variant == 2 {
        &[24, 72, 120, 168, 216]
    } else {
        &[24, 72, 120, 216]
    }
}
pub(crate) fn focus_at(variant: u8, x: f32, y: f32) -> Option<u32> {
    if !(1024.0..1280.0).contains(&x) {
        return None;
    }
    rows(variant)
        .iter()
        .position(|top| (*top as f32..(*top + 48) as f32).contains(&y))
        .map(|index| index as u32)
}
pub(super) fn return_to_title(state: &mut MusicaRuntimeState) {
    state.wait = None;
    state.message = None;
    state.message_loads.clear();
    state.choice = None;
    state.movie = None;
    state.system_ui = MusicaSystemUiState {
        page: MusicaSystemPage::Title,
        ..Default::default()
    };
}
pub(super) fn validate(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if state.system_ui.page == MusicaSystemPage::Title
        && (state.launch_mode != MusicaLaunchMode::Title
            || state.terminal
            || state.wait.is_some()
            || state.system_ui.focus_index as usize >= rows(variant(state)).len())
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}
impl MusicaVm {
    pub(crate) fn start_title_script(
        &mut self,
        uri: String,
        hash: Hash256,
        script: ScScript,
    ) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Title {
            return Err(MusicaRuntimeError::State);
        }
        self.replace_script(uri, hash, script, None)?;
        self.state.effect = None;
        self.state.system_ui.page = MusicaSystemPage::None;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    pub fn begin_title_launch(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.fixed_tick != 0
            || self.state.pc_line != 0
            || self.state.instruction_count != 0
            || self.state.wait.is_some()
            || self.state.terminal
        {
            return Err(MusicaRuntimeError::State);
        }
        self.state.launch_mode = MusicaLaunchMode::Title;
        return_to_title(&mut self.state);
        Ok(())
    }
    pub fn title_variant(&self) -> u8 {
        variant(&self.state)
    }
    pub(crate) fn set_title_focus(&mut self, focus: u32) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Title
            || focus as usize >= rows(self.title_variant()).len()
        {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.focus_index = focus;
        Ok(())
    }
    pub(crate) fn open_title_load(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Title {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.page = MusicaSystemPage::Load;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }
    pub(crate) fn close_title_load(&mut self) -> Result<(), MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Load {
            return Err(MusicaRuntimeError::State);
        }
        return_to_title(&mut self.state);
        Ok(())
    }
    pub(crate) fn set_launch_mode(&mut self, mode: MusicaLaunchMode) {
        self.state.launch_mode = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_variants_follow_only_the_verified_clear_flags_and_bounds() {
        let source = b".end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap();
        vm.begin_title_launch().unwrap();
        assert_eq!(vm.title_variant(), 0);
        let mut unlocks =
            ["AYAME_CLEAR", "SUI_CLEAR", "REN_CLEAR"].map(|s| Hash256::from_sha256(s.as_bytes()));
        unlocks.sort_unstable();
        vm.merge_verified_gallery_unlocks(&unlocks).unwrap();
        assert_eq!(vm.title_variant(), 1);
        vm.merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();
        assert_eq!(vm.title_variant(), 2);
        assert_eq!(focus_at(0, 1174.0, 191.0), None);
        assert_eq!(focus_at(2, 1174.0, 191.0), Some(3));
        assert_eq!(focus_at(2, f32::NAN, 191.0), None);
        assert!(vm.set_title_focus(5).is_err());
        assert!(vm.encode_native_save().is_ok());
    }
}
