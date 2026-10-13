use super::stage::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn launch(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.ensure_presentation_active()?;
        // Prepare only the authored launch-state image prefix before the
        // runtime step schedules asynchronous look-ahead. This prevents the
        // same image from racing through both the synchronous upload path and
        // the prefetch cache after its CPU RGBA has already been released.
        let prewarmed_images = self.prewarm_default_gameplay_story_images()?;
        let mut batch = self.step(NativeVnStepCommand::LaunchDefault)?;
        self.queue_default_gameplay_story_audio_preloads()?;
        if !prewarmed_images.is_empty() {
            let [PlayerHostCommand::PresentScene { commands, .. }] = batch.commands.as_mut_slice()
            else {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_IMAGE_GPU_PREWARM_PRESENTATION_BATCH".into(),
                ));
            };
            let mut uploads = Vec::with_capacity(prewarmed_images.len());
            let mut uploaded_ids = Vec::new();
            for asset_id in prewarmed_images {
                if self.live_texture_ids.insert(asset_id.clone()) {
                    let frame = self.texture(&asset_id)?.clone();
                    let frame_bytes = frame.rgba8.len() as u64;
                    self.resident_texture_bytes = self
                        .resident_texture_bytes
                        .checked_add(frame_bytes)
                        .ok_or_else(|| {
                            NativeVnHostError::Asset(
                                "ASTRA_PLAYER_GPU_RESIDENT_BYTES_OVERFLOW".into(),
                            )
                        })?;
                    if self.resident_texture_bytes > self.gpu_texture_budget_bytes {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_GPU_RESIDENT_BUDGET_EXCEEDED: {} > {}",
                            self.resident_texture_bytes, self.gpu_texture_budget_bytes
                        )));
                    }
                    self.live_texture_bytes
                        .insert(asset_id.clone(), frame_bytes);
                    self.mark_texture_used(&asset_id)?;
                    uploads.push(SceneCommand::UploadTexture {
                        resource_id: asset_id.clone(),
                        frame,
                    });
                    uploaded_ids.push(asset_id);
                }
            }
            commands.splice(0..0, uploads);
            for asset_id in uploaded_ids {
                self.remove_texture(&asset_id)?;
                self.asset_store.release_uploaded_image(&asset_id)?;
            }
        }
        Ok(batch)
    }

    pub fn take_audio_preload_requests(&mut self) -> Vec<NativeVnAudioPreloadRequest> {
        std::mem::take(&mut self.pending_audio_preloads)
    }

    pub(super) fn queue_current_story_audio_preloads(&mut self) -> Result<(), NativeVnHostError> {
        let Some(story_id) = self
            .runtime_state
            .as_ref()
            .and_then(|state| state.cursor.as_ref())
            .map(|cursor| cursor.story_id.clone())
        else {
            return Ok(());
        };
        self.queue_story_audio_preloads(&story_id)
    }

    pub(super) fn queue_default_gameplay_story_audio_preloads(
        &mut self,
    ) -> Result<(), NativeVnHostError> {
        let Some(story_id) = self.default_gameplay_story_id() else {
            return Ok(());
        };
        self.queue_story_audio_preloads(&story_id)
    }

    pub(super) fn default_gameplay_story_id(&self) -> Option<String> {
        let system_story_ids = self
            .story
            .system_story_manifest
            .entries
            .values()
            .map(|entry| entry.story_id.as_str())
            .collect::<BTreeSet<_>>();
        self.story
            .story_manifest
            .stories
            .iter()
            .find(|story| !system_story_ids.contains(story.id.as_str()))
            .map(|story| story.id.clone())
    }

    pub(super) fn prewarm_default_gameplay_story_images(
        &mut self,
    ) -> Result<Vec<String>, NativeVnHostError> {
        const MAX_ENTRY_IMAGE_PRELOADS: usize = 4_096;
        // The launch batch is outside the interactive frame budget.  Fill the
        // complete profile-bound resident window here so the first authored
        // stage cannot synchronously decrypt/decode a background after input.
        const MAX_ENTRY_GPU_PREWARM_BYTES: u64 = DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES;

        let mut seen = BTreeSet::new();
        let mut asset_ids = Vec::new();
        // Only the launch state may perform synchronous work.  The full route
        // graph is scheduled by the bounded asynchronous prefetcher after the
        // first frame, otherwise opening a title window can decode an entire
        // game's image set before the first present.
        let mut ordered_state_ids = Vec::new();
        if let Some(state_id) = default_runtime_launch_state(&self.story.stories) {
            ordered_state_ids.push(state_id);
        }
        if ordered_state_ids.is_empty() {
            return Ok(Vec::new());
        }
        for state_id in &ordered_state_ids {
            let state = self.story.states.get(state_id).ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_IMAGE_PREWARM_STATE_MISSING: {state_id}"
                ))
            })?;
            for command in state.scenes.iter().flat_map(|scene| scene.commands.iter()) {
                let CompiledCommand::Presentation {
                    command: PresentationCommand::Stage(stage),
                    ..
                } = command
                else {
                    continue;
                };
                let asset_id = match stage {
                    StageCommand::Preload { asset }
                    | StageCommand::Background { asset, .. }
                    | StageCommand::Show { asset, .. } => Some(asset),
                    StageCommand::Movie { fallback, .. } => fallback.as_ref(),
                    _ => None,
                };
                if let Some(asset_id) = asset_id.filter(|id| self.asset_store.contains_image(id)) {
                    if seen.insert(asset_id.clone()) {
                        asset_ids.push(asset_id.clone());
                    }
                }
            }
        }
        if asset_ids.len() > MAX_ENTRY_IMAGE_PRELOADS {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_IMAGE_PREWARM_COUNT_EXCEEDED: {} > {MAX_ENTRY_IMAGE_PRELOADS}",
                asset_ids.len()
            )));
        }
        let requested = asset_ids.len();
        let retained = self.asset_store.prewarm_image_prefix(&asset_ids)?;
        let retained_asset_ids = asset_ids.into_iter().take(retained).collect::<Vec<_>>();
        let retained_set = retained_asset_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut gpu_prewarms = Vec::new();
        let mut gpu_prewarm_bytes = 0u64;
        for asset_id in retained_asset_ids {
            let frame = self.stage_texture_for_viewport(self.asset_store.load_image(&asset_id)?)?;
            let frame_bytes = frame.rgba8.len() as u64;
            // CPU-ready frames and GPU uploads have independent budgets. Keep
            // every retained entry in the bounded decoded window so authored
            // transition frames never re-decrypt on their first use, but only
            // submit the smaller GPU resident prefix at launch.
            self.store_texture(asset_id.clone(), frame, &retained_set)?;
            if gpu_prewarm_bytes
                .checked_add(frame_bytes)
                .is_none_or(|total| total > MAX_ENTRY_GPU_PREWARM_BYTES)
            {
                // A large authored image must not terminate the whole bounded
                // window.  Keep scanning so later small entry assets can still
                // become resident; oversize frames remain CPU-ready and are
                // uploaded by the normal LRU when they are actually visible.
                continue;
            }
            gpu_prewarm_bytes += frame_bytes;
            gpu_prewarms.push(asset_id);
        }
        tracing::info!(
            event = "player.image.prewarm.completed",
            requested_count = requested,
            retained_count = retained,
            gpu_prewarm_count = gpu_prewarms.len(),
            gpu_prewarm_bytes,
            cache_bytes = self.asset_store.cache_bytes(),
            "prewarmed an authored-order image prefix within the decoded asset cache budget"
        );
        Ok(gpu_prewarms)
    }

    pub(super) fn queue_story_audio_preloads(
        &mut self,
        story_id: &str,
    ) -> Result<(), NativeVnHostError> {
        const MAX_ENTRY_AUDIO_PRELOADS: usize = 4_096;

        if self.audio_preload_story_ids.contains(story_id) {
            return Ok(());
        }
        let mut asset_ids = BTreeSet::new();
        for state in self
            .story
            .states
            .values()
            .filter(|state| state.story_id == story_id)
        {
            for scene in &state.scenes {
                for command in &scene.commands {
                    let CompiledCommand::Presentation {
                        command: PresentationCommand::Stage(StageCommand::Audio(cue)),
                        ..
                    } = command
                    else {
                        continue;
                    };
                    if matches!(cue.bus, VnAudioBus::Bgm | VnAudioBus::Se) {
                        asset_ids.insert(cue.asset.clone());
                    }
                }
            }
        }
        if asset_ids.len() > MAX_ENTRY_AUDIO_PRELOADS {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_AUDIO_PRELOAD_COUNT_EXCEEDED: {} > {MAX_ENTRY_AUDIO_PRELOADS}",
                asset_ids.len()
            )));
        }
        let mut requests = Vec::with_capacity(asset_ids.len());
        for asset_id in asset_ids {
            if !self.asset_store.contains_audio(&asset_id) {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_AUDIO_PRELOAD_ASSET_KIND: {asset_id}"
                )));
            }
            let asset = self.asset_store.load_media(&asset_id)?;
            requests.push(NativeVnAudioPreloadRequest {
                asset_id,
                codec: asset.codec,
                encoded_bytes: asset.bytes,
                encoded_length: asset.byte_length,
            });
        }
        let pending_assets = self
            .pending_audio_preloads
            .iter()
            .map(|request| {
                (
                    request.asset_id.clone(),
                    request.codec.clone(),
                    request.encoded_length,
                )
            })
            .collect::<BTreeSet<_>>();
        self.pending_audio_preloads
            .extend(requests.into_iter().filter(|request| {
                !pending_assets.contains(&(
                    request.asset_id.clone(),
                    request.codec.clone(),
                    request.encoded_length,
                ))
            }));
        self.audio_preload_story_ids.insert(story_id.to_string());
        Ok(())
    }
}
