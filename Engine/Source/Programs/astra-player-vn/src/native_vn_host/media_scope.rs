use super::*;

impl NativeVnVideoRequest {
    /// Workers may stop before returning obsolete decoded frames.
    pub fn is_cancelled(&self) -> bool {
        self.scope.is_cancelled()
    }
}

impl NativeVnHostCommandSource {
    pub(super) fn skip_waiting_video(&mut self) -> Result<bool, NativeVnHostError> {
        let Some(state) = self.runtime_state.as_ref() else {
            return Ok(false);
        };
        if !state.system_stack.is_empty() || !state.system.skip_allowed {
            return Ok(false);
        }
        let Some(wait) = state
            .pending_wait
            .as_ref()
            .filter(|wait| wait.kind == VnWaitKind::MovieEnd)
        else {
            return Ok(false);
        };
        let Some(layer) = self
            .stage_director
            .state()
            .movies
            .values()
            .find(|movie| {
                movie.fence.as_deref() == Some(wait.fence.as_str())
                    && self
                        .video_scopes
                        .get(&movie.layer)
                        .is_some_and(|scope| !scope.is_cancelled())
            })
            .map(|movie| movie.layer.clone())
        else {
            return Ok(false);
        };
        let completed = self
            .stage_director
            .complete_video(&layer)
            .map_err(stage_director_error)?;
        self.pending_stage_completions.extend(completed);
        if let Some(scope) = self.video_scopes.get(&layer) {
            scope.cancel();
        }
        self.pending_video.retain(|request| !request.is_cancelled());
        tracing::info!(
            event = "astra.player.video.skipped",
            layer,
            "physical advance completed the current skippable movie wait"
        );
        Ok(true)
    }

    pub(super) fn replace_video_scope(&mut self, layer: &str) -> astra_runtime::TaskScope {
        let scope = self.media_scope.child();
        if let Some(old) = self.video_scopes.insert(layer.into(), scope.clone()) {
            old.cancel();
        }
        scope
    }

    pub(super) fn validate_video_request(
        &mut self,
        request: &NativeVnVideoRequest,
    ) -> Result<(), NativeVnHostError> {
        self.ensure_presentation_active()?;
        if request.is_cancelled() || self.video_scopes.get(&request.layer) != Some(&request.scope) {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_VIDEO_REQUEST_STALE: video request belongs to inactive work".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn cancel_removed_video_scopes(&mut self) {
        self.video_scopes.retain(|layer, scope| {
            let retained = self.stage_director.state().movies.contains_key(layer);
            if !retained {
                scope.cancel();
            }
            retained
        });
    }

    pub(super) fn reset_pending_work(&mut self) {
        self.media_scope.cancel();
        self.media_scope = astra_runtime::TaskScope::new();
        self.video_scopes.clear();
        self.pending_timeline.clear();
        self.pending_audio.clear();
        self.pending_audio_preloads.clear();
        self.audio_preload_story_ids.clear();
        self.pending_video.clear();
        self.pending_stage_completions.clear();
        self.pending_ui_host_request = None;
        self.pending_save_metadata = None;
        self.pending_save_completion = None;
        self.gameplay_thumbnail_capture = None;
    }
}

impl Drop for NativeVnHostCommandSource {
    fn drop(&mut self) {
        self.media_scope.cancel();
    }
}
