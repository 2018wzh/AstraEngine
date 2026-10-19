use super::*;
impl MusicaSession {
    pub(crate) fn install_localization(
        &mut self,
        table: Option<std::sync::Arc<crate::MusicaTranslationTable>>,
        mode: crate::TranslationMode,
    ) {
        self.translation_table = table;
        self.translation_mode = mode;
    }
    pub(super) fn cancel_text(&mut self) -> FamilyResult<()> {
        if let Some(service) = &self.replacement {
            service.invalidate();
        }
        self.pending = None;
        self.hook_failed = false;
        self.choice_text = None;
        Ok(())
    }

    pub(super) fn semantic_event(
        &mut self,
        mut event: Option<MusicaVmEvent>,
    ) -> FamilyResult<Option<MusicaVmEvent>> {
        if self.pending.is_none() {
            if let Some(MusicaVmEvent::Message { text, speaker, .. }) = &mut event {
                if let (Some(table), Some((body_hash, speaker_hash))) =
                    (&self.translation_table, self.vm.message_display_hashes())
                {
                    if let Some((translated, name_hit)) = table.lookup(body_hash) {
                        if name_hit || self.translation_mode == crate::TranslationMode::Replace {
                            *text = translated.to_owned();
                        } else {
                            self.translation_overlay = Some(translated.to_owned());
                        }
                    }
                    if let Some((translated, name_hit)) =
                        speaker_hash.and_then(|hash| table.lookup(hash))
                    {
                        if name_hit || self.translation_mode == crate::TranslationMode::Replace {
                            *speaker = Some(translated.to_owned());
                        }
                    }
                }
                self.scene.set_translation_overlay(
                    self.translation_overlay.clone(),
                    self.translation_visible,
                );
            }
            let enabled = self
                .replacement
                .as_ref()
                .is_some_and(|s| s.0.text.is_some());
            if !enabled
                || !matches!(
                    event,
                    Some(MusicaVmEvent::Message { .. } | MusicaVmEvent::Choice)
                )
            {
                return Ok(event);
            }
            self.generation = self.generation.checked_add(1).ok_or_else(|| {
                error("ASTRA_EMU_MUSICA_TEXT_SEQUENCE", "text sequence overflowed")
            })?;
            self.pending = Some(PendingText {
                id: self.generation,
                event: event.unwrap(),
            });
        }
        let pending = self.pending.as_ref().unwrap();
        let labels;
        let source = match &pending.event {
            MusicaVmEvent::Message { text, speaker, .. } => {
                let mut source = vec![(astra_emu_abi::TEXT_DIALOGUE, text.as_str())];
                if let Some(speaker) = speaker {
                    source.push((astra_emu_abi::TEXT_SPEAKER, speaker.as_str()));
                }
                source
            }
            MusicaVmEvent::Choice => {
                labels = self
                    .localized_choice_labels()?
                    .ok_or_else(|| error("ASTRA_EMU_MUSICA_CHOICE", "choice is unavailable"))?
                    .0;
                labels
                    .iter()
                    .map(|text| (astra_emu_abi::TEXT_CHOICE, text.as_str()))
                    .collect()
            }
            _ => unreachable!(),
        };
        self.audio.suspend(true)?;
        let result = self
            .replacement
            .as_ref()
            .unwrap()
            .transform(pending.id, &source);
        self.reset_host_clock = true;
        self.phase = 0;
        self.input_pending = false;
        let mut transformed = match result {
            Ok(result) => result.into_iter(),
            Err(cause) => {
                self.hook_failed = true;
                return Err(cause);
            }
        };
        let mut event = self.pending.take().unwrap().event;
        match &mut event {
            MusicaVmEvent::Message { text, speaker, .. } => {
                *text = transformed.next().unwrap();
                if speaker.is_some() {
                    *speaker = transformed.next();
                }
                self.choice_text = None;
            }
            MusicaVmEvent::Choice => self.choice_text = Some(transformed.collect()),
            _ => unreachable!(),
        }
        self.hook_failed = false;
        Ok(Some(event))
    }

    pub(super) fn choice_display(&self) -> FamilyResult<Option<(Vec<String>, u32)>> {
        let Some((labels, index)) = self.localized_choice_labels()? else {
            return Ok(None);
        };
        Ok(Some((self.choice_text.clone().unwrap_or(labels), index)))
    }

    fn localized_choice_labels(&self) -> FamilyResult<Option<(Vec<String>, u32)>> {
        Self::localized_choices_for(
            &self.vm,
            self.translation_table.as_deref(),
            self.translation_mode,
        )
    }

    pub(super) fn localized_choices_for(
        vm: &MusicaVm,
        table: Option<&crate::MusicaTranslationTable>,
        mode: crate::TranslationMode,
    ) -> FamilyResult<Option<(Vec<String>, u32)>> {
        let Some((mut labels, index)) = vm.choice_display().map_err(vm_error)? else {
            return Ok(None);
        };
        if let Some(table) = table {
            for (label, hash) in labels.iter_mut().zip(vm.choice_display_hashes()) {
                if let Some((translated, name_hit)) = table.lookup(hash) {
                    if name_hit || mode == crate::TranslationMode::Replace {
                        *label = translated.to_owned();
                    }
                }
            }
        }
        Ok(Some((labels, index)))
    }

    pub(super) fn message(&mut self, text: String, speaker: Option<String>) -> FamilyResult<()> {
        if text.len() > MAX_TEXT_BYTES
            || speaker.as_ref().is_some_and(|s| s.len() > MAX_SYMBOL_BYTES)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXT_BOUND",
                "message exceeds supported bounds",
            ));
        }
        self.message = Some((text, speaker));
        Ok(())
    }
    pub(super) fn poll_voice_duration(&mut self) -> FamilyResult<()> {
        let Some(MusicaWaitState::Voice {
            stream_id,
            milliseconds: None,
            ..
        }) = self.vm.state().wait.as_ref()
        else {
            self.voice_duration = None;
            return Ok(());
        };
        if self.voice_duration.is_none() {
            self.voice_duration = Some((self.audio.duration(*stream_id)?, Instant::now()));
        }
        let (reply, started) = self.voice_duration.as_ref().unwrap();
        match reply.try_recv() {
            Ok(result) => {
                self.vm.set_voice_duration(result?).map_err(vm_error)?;
                self.voice_duration = None;
                self.wait_ns = 0;
            }
            Err(std::sync::mpsc::TryRecvError::Empty)
                if started.elapsed() < Duration::from_secs(15) => {}
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_AUDIO_DURATION",
                    "voice duration request failed or timed out",
                ))
            }
        }
        Ok(())
    }
}
