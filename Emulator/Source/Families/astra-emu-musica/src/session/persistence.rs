use super::*;
impl MusicaSession {
    pub(super) fn save(&mut self) -> FamilyResult<()> {
        let sounds = self.audio.snapshot()?;
        self.poll_voice_duration()?;
        let vm = self
            .vm
            .encode_native_save()
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM state cannot be saved"))?;
        self.storage.write(&Snapshot {
            game: self.game,
            vm,
            message: self.message.clone(),
            wait_ns: self.wait_ns,
            sounds,
        })?;
        tracing::info!(event = "astra.emu.musica.save.completed");
        Ok(())
    }
    pub(super) fn load(&mut self) -> FamilyResult<()> {
        let mut saved = self.storage.read()?;
        if saved.game != self.game {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_GAME",
                "save belongs to another archive/profile identity",
            ));
        }
        let state = MusicaVm::decode_native_save(&saved.vm)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "VM save is invalid"))?;
        let bytes = read_asset(&self.archive, &state.script_uri, 16 * 1024 * 1024)?;
        if Hash256::from_sha256(&bytes) != state.script_hash {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_SCRIPT",
                "saved script has changed",
            ));
        }
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_musica())
            .map_err(|_| error("ASTRA_EMU_MUSICA_SCRIPT", "saved script cannot be parsed"))?;
        let mut vm = MusicaVm::new(
            state.script_uri,
            state.script_hash,
            script,
            state.session_seed,
        )
        .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "saved script is invalid"))?;
        vm.set_voice_preferences(self.vm.voice_preferences().clone());
        vm.restore_native_save(&saved.vm, 1)
            .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_STATE", "saved VM state is invalid"))?;
        vm.set_auto_delay_units(self.vm.auto_delay_units())
            .map_err(vm_error)?;
        let mut scene = Scene::new(self.archive.clone(), self.info.width, self.info.height)?;
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
        self.vm = vm;
        self.movie = movie;
        self.scene = scene;
        self.message = saved.message;
        self.wait_ns = saved.wait_ns;
        self.phase = 0;
        self.input_pending = false;
        self.pointer = None;
        self.finished = self.vm.state().terminal;
        tracing::info!(event = "astra.emu.musica.load.completed");
        Ok(())
    }
}
