use super::*;
impl FamilySession for MusicaSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        if self.poisoned {
            return Err(error(
                "ASTRA_EMU_MUSICA_POISONED",
                "failed session must be closed",
            ));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.advance_inner(elapsed_ns, events)
        }))
        .unwrap_or_else(|_| {
            Err(error(
                "ASTRA_EMU_MUSICA_SESSION_PANIC",
                "session panicked and must be closed",
            ))
        });
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        visitor.accept(FrameView::from_slice(&self.scene.pixels, self.info)?)
    }
    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        let text = self.cancel_text();
        let movie = self
            .movie
            .take()
            .map(|movie| movie.close(&self.audio))
            .transpose();
        let audio = self.audio.shutdown();
        tracing::info!(event = "astra.emu.musica.session.close");
        text?;
        movie?;
        audio
    }
}
impl Drop for MusicaSession {
    fn drop(&mut self) {
        let _ = self.cancel_text();
        if let Some(movie) = self.movie.take() {
            let _ = movie.close(&self.audio);
        }
        let _ = self.audio.shutdown();
    }
}

pub(super) fn vm_error(cause: crate::MusicaRuntimeError) -> FamilyError {
    if let crate::MusicaRuntimeError::UnsupportedOpcode { ordinal, .. } = &cause {
        return error(
            cause.diagnostic_code(),
            &format!("script command at ordinal {ordinal} is not implemented"),
        );
    }
    error(
        cause.diagnostic_code(),
        "script execution failed at an unsupported or invalid operation",
    )
}
