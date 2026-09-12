use super::*;

impl NativeVnHostCommandSource {
    pub(super) fn ensure_stage_texture_assets(
        &mut self,
        required: BTreeSet<String>,
        cpu_required: BTreeSet<String>,
    ) -> Result<(), NativeVnHostError> {
        for asset_id in &required {
            let gpu_resident = self.live_texture_ids.contains(asset_id)
                && self.texture_dimensions.contains_key(asset_id);
            if !self.textures.contains_key(asset_id)
                && (!gpu_resident || cpu_required.contains(asset_id))
            {
                let cache_hit = self.asset_store.is_image_cached(asset_id)?;
                let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
                let frame =
                    self.stage_texture_for_viewport(self.asset_store.load_image(asset_id)?)?;
                let duration_ns = performance_phase_duration(started)?;
                tracing::debug!(
                    event = "player.stage_texture.materialized",
                    cache_hit,
                    duration_ns,
                    byte_count = frame.rgba8.len(),
                    "materialized a stage texture for the current presentation state"
                );
                self.store_texture(asset_id.clone(), frame, &required)?;
            }
        }
        Ok(())
    }
}

pub(super) fn stage_texture_requirements(
    state: &ProductStageState,
    textures: &BTreeMap<String, TextureFrame>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut required = state
        .entities
        .values()
        .filter(|entity| entity.visible)
        .map(|entity| entity.asset.clone())
        .collect::<BTreeSet<_>>();
    let mut cpu_required = BTreeSet::new();
    for movie in state.movies.values() {
        if !textures.contains_key(&movie.asset) {
            if let Some(fallback) = &movie.fallback {
                required.insert(fallback.clone());
                cpu_required.insert(fallback.clone());
            }
        }
    }
    (required, cpu_required)
}

impl NativeVnHostCommandSource {
    pub(super) fn ensure_presentation_active(&mut self) -> Result<(), NativeVnHostError> {
        if self.presentation_failed
            || self.stage_director.is_failed()
            || self.media_scope.is_cancelled()
        {
            return Err(NativeVnHostError::RuntimeEvidence("ASTRA_PLAYER_PRESENTATION_SESSION_FAILED: restore a saved session or recreate the Player".into()));
        }
        if self.pending_wait().is_some_and(|wait| {
            self.stage_director.fence_status(&wait.fence)
                == Some(astra_vn_core::FenceStatus::Failed)
        }) {
            self.presentation_failed = true;
            self.media_scope.cancel();
            return Err(NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_PRESENTATION_FENCE_FAILED: the awaited presentation group failed"
                    .into(),
            ));
        }
        Ok(())
    }

    pub(super) fn check_wait_fence(
        &mut self,
        director: &ProductStageDirector,
    ) -> Result<(), NativeVnHostError> {
        if self.pending_wait().is_some_and(|wait| {
            director.fence_status(&wait.fence) == Some(astra_vn_core::FenceStatus::Failed)
        }) {
            self.presentation_failed = true;
            self.media_scope.cancel();
            return Err(NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_PRESENTATION_FENCE_FAILED: the awaited presentation group failed"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn tick_presentation(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<PlayerHostCommandBatch>, NativeVnHostError> {
        self.ensure_presentation_active()?;
        if delta_ns == 0 || delta_ns > 1_000_000_000 {
            return Err(NativeVnHostError::RuntimeEvidence("ASTRA_PLAYER_PRESENTATION_TICK_DELTA: presentation delta is outside the supported range".into()));
        }
        let result = self.tick_presentation_inner(delta_ns);
        if result.is_err() {
            self.presentation_failed = true;
            self.media_scope.cancel();
        }
        result
    }

    fn tick_presentation_inner(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<PlayerHostCommandBatch>, NativeVnHostError> {
        self.ensure_text_region()?;
        if !self.stage_director.requires_frame_tick() {
            return Ok(None);
        }
        let outputs = self
            .stage_director
            .tick(delta_ns)
            .map_err(stage_director_error)?;
        let mut completions = Vec::new();
        let mut videos = Vec::new();
        for output in outputs {
            match output {
                StageDirectorOutput::FenceCompleted { id, .. } => completions.push(id),
                StageDirectorOutput::Movie(movie) => {
                    let asset = self.asset_store.load_media(&movie.asset)?;
                    videos.push(NativeVnVideoRequest {
                        scope: self.replace_video_scope(&movie.layer),
                        layer: movie.layer,
                        asset_id: movie.asset,
                        codec: asset.codec.clone(),
                        encoded_bytes: asset.bytes.clone(),
                        encoded_length: asset.byte_length,
                        alpha_millionths: movie.alpha.millionths,
                        looping: matches!(movie.loop_mode, MovieLoopMode::Loop),
                        fence: movie.fence,
                    });
                }
                StageDirectorOutput::Preload { .. }
                | StageDirectorOutput::Audio(_)
                | StageDirectorOutput::AudioControl(_)
                | StageDirectorOutput::AudioBusEnabled { .. }
                | StageDirectorOutput::Effect(_) => {
                    return Err(NativeVnHostError::RuntimeEvidence(
                        "ASTRA_PLAYER_STAGE_TICK_OUTPUT_DOMAIN: frame tick output has no ordered host consumer".into(),
                    ));
                }
            }
        }
        let (required, cpu_required) =
            stage_texture_requirements(self.stage_director.state(), &self.textures);
        self.ensure_stage_texture_assets(required, cpu_required)?;
        self.scene_draw = stage_scene_commands(
            self.stage_director.state(),
            &self.textures,
            &self.texture_dimensions,
            self.width,
            self.height,
        )?;
        let batch = self.render(&[], 0)?;
        self.pending_stage_completions.extend(completions);
        self.pending_video.extend(videos);
        Ok(Some(batch))
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
