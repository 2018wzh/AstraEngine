use super::*;
impl MusicaSession {
    pub(super) fn advance_movie(&mut self, elapsed_ns: u64) -> FamilyResult<bool> {
        if let Some(movie) = &mut self.movie {
            let paused =
                self.suspended || self.vm.state().system_ui.page != crate::MusicaSystemPage::None;
            if let Some(frame) = movie.advance(&self.audio, elapsed_ns, paused)? {
                self.scene.render_movie(frame)?;
            }
            let state = self
                .vm
                .state()
                .movie
                .as_ref()
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_MOVIE_STATE",
                        "movie session has no VM state",
                    )
                })?
                .clone();
            self.vm
                .update_movie_position(&state.media_id, &state.fence_id, movie.position_us())
                .map_err(vm_error)?;
            if movie.ended() || (state.skippable && self.control_keys != 0) || self.finished {
                self.movie.take().unwrap().close(&self.audio)?;
                self.vm.resolve_wait(&state.fence_id).map_err(vm_error)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}
