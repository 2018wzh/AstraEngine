use super::*;
impl MusicaSession {
    pub(super) fn persist_progress(&mut self) -> FamilyResult<()> {
        let unlocks = &self.vm.state().gallery_unlocks;
        if *unlocks != self.persisted_unlocks {
            self.storage.write_progress(self.game, unlocks)?;
            self.persisted_unlocks = unlocks.clone();
        }
        Ok(())
    }
    pub(super) fn quick_save(&mut self) -> FamilyResult<()> {
        let pc_line = self.vm.state().pc_line;
        if self.last_quick_save_pc_line == Some(pc_line) {
            return Ok(());
        }
        let slot = 10 + self.quick_cursor;
        self.save(slot)?;
        let next = (self.quick_cursor + 1) % crate::storage::SAVE_PAGE_WIDTH;
        self.storage.write_quick_cursor(self.game, next)?;
        self.quick_cursor = next;
        self.last_quick_save_pc_line = Some(pc_line);
        tracing::debug!(event = "astra.emu.musica.quick_save.rotated", slot, next);
        Ok(())
    }
    pub(super) fn quick_load(&mut self) -> FamilyResult<()> {
        self.load(
            10 + (self.quick_cursor + crate::storage::SAVE_PAGE_WIDTH - 1)
                % crate::storage::SAVE_PAGE_WIDTH,
        )
    }

    pub(super) fn save(&mut self, slot: u32) -> FamilyResult<()> {
        let sounds = self.audio.snapshot()?;
        self.poll_voice_duration()?;
        let vm = self
            .vm
            .encode_native_save()
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM state cannot be saved"))?;
        let mut state = MusicaVm::decode_native_save(&vm).map_err(vm_error)?;
        if matches!(
            state.system_ui.page,
            crate::MusicaSystemPage::Save | crate::MusicaSystemPage::Load
        ) {
            state.system_ui.page = crate::MusicaSystemPage::None;
            state.system_ui.focus_index = 0;
        }
        let vm = postcard::to_allocvec(&state)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM state cannot be saved"))?;
        let pixels = self.gameplay_frame.as_ref().unwrap_or(&self.scene.pixels);
        let card = crate::storage::SaveCard::capture(self.info.width, self.info.height, pixels)?;
        self.storage.write(
            slot,
            &Snapshot {
                card,
                game: self.game,
                vm,
                message: self.message.clone(),
                wait_ns: self.wait_ns,
                sounds,
            },
        )?;
        tracing::info!(event = "astra.emu.musica.save.completed", slot);
        Ok(())
    }
    pub(super) fn load(&mut self, slot: u32) -> FamilyResult<()> {
        let mut saved = self.storage.read(slot)?;
        if saved.game != self.game {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_GAME",
                "save belongs to another archive/profile identity",
            ));
        }
        let state = MusicaVm::decode_native_save(&saved.vm)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM save is invalid"))?;
        let loaded = crate::script_loader::load_script(
            &self.archive,
            &state.script_uri,
            self.primary_encoding,
        )?;
        if loaded.hash != state.script_hash {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_SCRIPT",
                "saved script or include has changed",
            ));
        }
        if loaded.script.encoding != state.script_encoding {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_ENCODING",
                "saved script encoding no longer matches",
            ));
        }
        let script = loaded.script;
        let mut vm = MusicaVm::new(
            state.script_uri,
            state.script_hash,
            script,
            state.session_seed,
        )
        .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "saved script is invalid"))?;
        vm.set_config(self.vm.config().clone()).map_err(vm_error)?;
        vm.restore_native_save(&saved.vm, 1)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "saved VM state is invalid"))?;
        vm.set_launch_mode(self.vm.state().launch_mode);
        vm.merge_verified_gallery_unlocks(&self.persisted_unlocks)
            .map_err(vm_error)?;
        let mut scene = Scene::new(
            self.archive.clone(),
            self.info.width,
            self.info.height,
            state.script_encoding,
        )?;
        scene.set_text_shadow(self.scene.text_shadow());
        let choices = vm.choice_display().map_err(vm_error)?;
        scene.render(
            vm.state(),
            saved.message.as_ref(),
            choices
                .as_ref()
                .map(|(labels, index)| (labels.as_slice(), *index)),
        )?;
        self.cancel_text()?;
        if let Some(service) = &self.replacement {
            service.reset(TextResetReason::Load).into_result()?;
        }
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_TEXT_SEQUENCE",
                "text generation overflowed",
            )
        })?;
        for sound in &mut saved.sounds {
            if sound.id == 4 && !self.vm.voice_preferences().enabled(&sound.uri) {
                sound.playing = false;
                sound.fade = None;
            }
        }
        if let Some(movie) = self.movie.take() {
            movie.close(&self.audio)?;
        }
        self.audio.restore(saved.sounds)?;
        let movie = vm
            .state()
            .movie
            .as_ref()
            .map(|state| crate::movie::Movie::open(&self.archive, state))
            .transpose()?;
        vm.set_control_pressed(self.control_keys != 0);
        self.voice_duration = None;
        self.load_from_title = false;
        self.title_focus = None;
        self.gameplay_frame = None;
        self.save_cards.clear();
        self.vm = vm;
        self.movie = movie;
        self.scene = scene;
        self.message = saved.message;
        self.wait_ns = saved.wait_ns;
        self.phase = 0;
        self.input_pending = false;
        self.pointer = None;
        self.finished = self.vm.state().terminal;
        self.reset_host_clock = true;
        tracing::info!(event = "astra.emu.musica.load.completed", slot);
        Ok(())
    }
}
