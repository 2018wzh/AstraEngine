use super::*;
impl MusicaSession {
    pub(crate) fn native_frame(&self) -> (FrameInfo, Arc<[u8]>) {
        (self.info, self.scene.pixels.clone())
    }
    pub(crate) fn native_failed_hook(&self) -> bool {
        self.hook_failed
    }
    pub(crate) fn native_suspended(&self) -> bool {
        self.suspended
    }
    pub(crate) fn native_pause(&mut self, paused: bool) -> FamilyResult<()> {
        self.suspended = paused;
        self.phase = 0;
        self.clear_input();
        self.reset_host_clock = true;
        self.update_pause()
    }
    pub(crate) fn native_retry(&mut self) -> FamilyResult<AdvanceResponse> {
        if !self.hook_failed || self.pending.is_none() {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXT_RETRY",
                "no failed text operation to retry",
            ));
        }
        self.hook_failed = false;
        self.phase = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.tick()?;
            let choices = self.choice_display()?;
            self.scene.render(
                self.vm.state(),
                self.message.as_ref(),
                choices
                    .as_ref()
                    .map(|(labels, index)| (labels.as_slice(), *index)),
            )?;
            self.advance_inner(0, &[])
        }))
        .unwrap_or_else(|_| {
            Err(error(
                "ASTRA_EMU_MUSICA_SESSION_PANIC",
                "session panicked during retry",
            ))
        });
        if result.is_err() && !self.hook_failed {
            self.poisoned = true;
        }
        result
    }
}
