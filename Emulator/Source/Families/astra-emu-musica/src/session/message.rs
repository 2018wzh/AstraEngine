use super::*;
impl MusicaSession {
    pub(super) fn cancel_text(&mut self) -> FamilyResult<()> {
        if let Some(pending) = self.pending.take() {
            if let Some(service) = &self.replacement {
                service.cancel(pending.id.into()).into_result()?;
            }
        }
        Ok(())
    }
    pub(super) fn poll_text(&mut self) -> FamilyResult<bool> {
        let Some(pending) = &self.pending else {
            return Ok(false);
        };
        let service = self
            .replacement
            .as_ref()
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_STATE", "text service is unavailable"))?;
        if pending.started.elapsed() > Duration::from_secs(15) {
            self.cancel_text()?;
            tracing::warn!(event = "astra.emu.musica.translation.timeout");
            return Ok(false);
        }
        match service.poll(pending.id.clone().into()).into_result() {
            Ok(TextPollResult::Pending) => Ok(false),
            Ok(TextPollResult::Ready(response)) => {
                let valid = response.validate().is_ok() && response.request_id == pending.id;
                self.pending = None;
                if !valid {
                    tracing::warn!(event = "astra.emu.musica.translation.invalid");
                    return Ok(false);
                }
                if let Some((_, speaker)) = &self.message {
                    let replacement = (response.replacement.to_string(), speaker.clone());
                    if self
                        .scene
                        .render(self.vm.state(), Some(&replacement), None)
                        .is_ok()
                    {
                        self.message = Some(replacement);
                    } else {
                        tracing::warn!(event = "astra.emu.musica.translation.render_failed");
                    }
                }
                Ok(false)
            }
            Ok(TextPollResult::Cancelled | TextPollResult::Failed(_)) | Err(_) => {
                self.pending = None;
                tracing::warn!(event = "astra.emu.musica.translation.failed");
                Ok(false)
            }
        }
    }
    pub(super) fn message(&mut self, text: String, speaker: Option<String>) -> FamilyResult<()> {
        self.cancel_text()?;
        if text.len() > MAX_TEXT_BYTES
            || speaker.as_ref().is_some_and(|s| s.len() > MAX_SYMBOL_BYTES)
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXT_BOUND",
                "message exceeds supported bounds",
            ));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_SEQUENCE", "text sequence overflowed"))?;
        self.message = Some((text.clone(), speaker.clone()));
        if let Some(service) = &self.replacement {
            let id = format!("{}.text.{}", self.id, self.generation);
            let request = TextReplacementRequest {
                request_id: id.clone().into(),
                source: text.into(),
                speaker: speaker.unwrap_or_default().into(),
                ruby: "".into(),
            };
            match service.submit(request).into_result() {
                Ok(()) => {
                    self.pending = Some(PendingText {
                        id,
                        started: Instant::now(),
                    })
                }
                Err(_) => tracing::warn!(event = "astra.emu.musica.translation.submit_failed"),
            }
        }
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
