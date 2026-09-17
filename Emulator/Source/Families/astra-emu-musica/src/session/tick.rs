use super::*;
impl MusicaSession {
    pub(super) fn tick(&mut self) -> FamilyResult<bool> {
        let visual_completion = matches!(
            self.vm.state().wait,
            Some(
                MusicaWaitState::CharacterTransition { .. }
                    | MusicaWaitState::LinearScroll { .. }
                    | MusicaWaitState::AxisScroll { .. }
            )
        );
        let tick = self
            .vm
            .state()
            .fixed_tick
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TICK", "tick counter overflowed"))?;
        self.poll_voice_duration()?;
        if let Some(wait) = self.vm.state().wait.clone() {
            let delta = ((u128::from(tick) * 1_000_000_000 / 60)
                - (u128::from(tick - 1) * 1_000_000_000 / 60)) as u64;
            self.wait_ns = self.wait_ns.saturating_add(delta);
            let (id, ready) = match wait {
                MusicaWaitState::Voice {
                    token_id,
                    milliseconds,
                    ..
                } => (
                    token_id,
                    self.pending.is_none()
                        && (self.vm.fast_forward_active()
                            || milliseconds.is_some_and(|duration| {
                                self.wait_ns >= u64::from(duration) * 1_000_000
                            })),
                ),
                MusicaWaitState::Input { token_id } => (
                    token_id,
                    (self.input_pending
                        || (self.vm.state().choice.is_none()
                            && self.vm.fast_forward_active()
                            && self.wait_ns >= 10_000_000))
                        && self.pending.is_none(),
                ),
                MusicaWaitState::Time {
                    token_id,
                    milliseconds,
                    ..
                } => (
                    token_id,
                    self.pending.is_none()
                        && (self.vm.fast_forward_active()
                            || self.wait_ns >= u64::from(milliseconds) * 1_000_000),
                ),
                MusicaWaitState::CharacterTransition {
                    token_id,
                    slot_id,
                    milliseconds,
                } => (
                    token_id,
                    (!self.vm.config().animation
                        && self.wait_ns >= u64::from(milliseconds) * 1_000_000)
                        || self
                            .vm
                            .state()
                            .characters
                            .get(&slot_id)
                            .and_then(|character| character.transition.as_ref())
                            .is_some_and(|transition| transition.completed),
                ),
                MusicaWaitState::LinearScroll {
                    token_id,
                    milliseconds,
                } => (
                    token_id,
                    (!self.vm.config().animation
                        && self.wait_ns >= u64::from(milliseconds) * 1_000_000)
                        || self
                            .vm
                            .state()
                            .linear_scroll
                            .as_ref()
                            .is_some_and(|scroll| scroll.completed),
                ),
                MusicaWaitState::AxisScroll {
                    token_id,
                    milliseconds,
                } => (
                    token_id,
                    (!self.vm.config().animation
                        && self.wait_ns >= u64::from(milliseconds) * 1_000_000)
                        || self
                            .vm
                            .state()
                            .axis_scroll
                            .as_ref()
                            .is_some_and(|scroll| scroll.completed),
                ),
                MusicaWaitState::Presentation { token_id, .. } => (token_id, true),
                MusicaWaitState::Media { token_id, .. } => (token_id, false),
                MusicaWaitState::Provider { .. } => {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_WAIT_UNSUPPORTED",
                        "script requires an unimplemented media or provider wait",
                    ))
                }
            };
            if ready {
                if self.vm.state().choice.is_some() {
                    self.vm.commit_choice().map_err(vm_error)?;
                } else {
                    self.vm.resolve_wait(&id).map_err(vm_error)?;
                }
                self.wait_ns = 0;
                self.input_pending = false;
            } else {
                self.vm.advance_waiting_tick(tick).map_err(vm_error)?;
                return Ok(false);
            }
        }
        let event = self.vm.step(tick).map_err(vm_error)?;
        let changed: FamilyResult<bool> = match event {
            Some(MusicaVmEvent::Movie(state)) => {
                if (state.width, state.height) != (self.info.width, self.info.height) {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_MOVIE_STAGE_IDENTITY",
                        "movie dimensions do not match the stage",
                    ));
                }
                self.movie = Some(crate::movie::Movie::open(&self.archive, &state)?);
                Ok(false)
            }
            Some(MusicaVmEvent::Choice) => {
                self.message = None;
                Ok(true)
            }
            Some(MusicaVmEvent::Message {
                text,
                speaker,
                audio_commands,
                ..
            }) => {
                self.voice_duration = None;
                self.audio.apply(audio_commands)?;
                self.poll_voice_duration()?;
                self.wait_ns = 0;
                self.message(text, speaker)?;
                Ok(true)
            }
            Some(MusicaVmEvent::Wait(_)) => {
                self.wait_ns = 0;
                Ok(false)
            }
            Some(MusicaVmEvent::Audio { commands }) => {
                self.audio.apply(commands)?;
                Ok(false)
            }
            Some(MusicaVmEvent::Chain { target }) => {
                let (file, label) =
                    crate::script::chain_target_parts(&target).ok_or_else(|| {
                        error(
                            "ASTRA_EMU_MUSICA_RUNTIME_CHAIN",
                            "invalid chained script location",
                        )
                    })?;
                let uri = format!("musica:/scr/{file}");
                let loaded =
                    crate::script_loader::load_script(&self.archive, &uri, self.primary_encoding)?;
                let encoding = loaded.script.encoding;
                self.vm
                    .replace_script(uri, loaded.hash, loaded.script, label)
                    .map_err(vm_error)?;
                self.scene.set_text_encoding(encoding);
                Ok(false)
            }
            Some(MusicaVmEvent::Stage(_)) => Ok(true),
            Some(
                MusicaVmEvent::Effect(_)
                | MusicaVmEvent::EffectCleared
                | MusicaVmEvent::Character(_)
                | MusicaVmEvent::Firefly(_)
                | MusicaVmEvent::FireflyCleared { .. }
                | MusicaVmEvent::SecondaryEffect(_)
                | MusicaVmEvent::SecondaryEffectCleared { .. }
                | MusicaVmEvent::WScroll2(_)
                | MusicaVmEvent::ScrollXf(_)
                | MusicaVmEvent::LinearScroll(_)
                | MusicaVmEvent::AxisScroll(_)
                | MusicaVmEvent::ScreenShake(_)
                | MusicaVmEvent::Panel { .. },
            ) => Ok(true),
            Some(MusicaVmEvent::Terminal) => {
                if self.vm.state().system_ui.page == crate::MusicaSystemPage::Title {
                    self.audio.restore(Vec::new())?;
                    self.message = None;
                    self.title_focus = None;
                    self.clear_input();
                    Ok(true)
                } else {
                    self.finished = true;
                    Ok(false)
                }
            }
            None => Ok(false),
        };
        changed.map(|changed| changed || visual_completion)
    }
}
