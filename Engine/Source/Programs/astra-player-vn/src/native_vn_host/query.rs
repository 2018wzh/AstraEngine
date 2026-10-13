use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn last_step_evidence(&self) -> Option<&NativeVnStepEvidence> {
        self.last_step_evidence.as_ref()
    }

    pub fn ui_performance_report(&self) -> UiPerformanceReport {
        self.ui_performance.report()
    }

    pub fn take_last_ui_performance_sample(&mut self) -> Option<UiPerformanceSample> {
        self.last_ui_performance_sample.take()
    }

    pub fn set_ui_host_performance_sampling_enabled(&mut self, enabled: bool) {
        self.ui_host_performance_sampling_enabled = enabled;
        if !enabled {
            self.last_ui_host_performance_sample = None;
        }
    }

    pub fn take_last_ui_host_performance_sample(
        &mut self,
    ) -> Option<NativeVnUiHostPerformanceSample> {
        self.last_ui_host_performance_sample.take()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn take_timeline_tasks(&mut self) -> Vec<PlayerTimelineTask> {
        std::mem::take(&mut self.pending_timeline)
    }

    pub(crate) fn restore_timeline_tasks(&mut self, mut tasks: Vec<PlayerTimelineTask>) {
        tasks.append(&mut self.pending_timeline);
        self.pending_timeline = tasks;
    }

    pub fn take_audio_requests(&mut self) -> Vec<NativeVnAudioOutput> {
        std::mem::take(&mut self.pending_audio)
    }

    pub fn take_video_requests(&mut self) -> Vec<NativeVnVideoRequest> {
        std::mem::take(&mut self.pending_video)
            .into_iter()
            .filter(|request| !request.is_cancelled())
            .collect()
    }

    pub fn take_stage_completions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_stage_completions)
    }

    pub fn invalidate_gpu_resources(&mut self) {
        self.gpu_resources_invalidated = true;
        let released_count = self.live_texture_ids.len();
        let released_glyph_count = self.text_resources.invalidate_device_resources();
        self.live_texture_ids.clear();
        self.live_texture_bytes.clear();
        self.pending_gpu_lifecycle.clear();
        self.texture_last_used.clear();
        self.live_layout_ids.clear();
        self.resident_texture_bytes = 0;
        self.ui_frame_reuse = None;
        tracing::warn!(
            event = "player.vn.gpu_resources.invalidated",
            released_count,
            released_glyph_count,
            "invalidated session GPU residency; package manifests remain authoritative for rebuild"
        );
    }

    pub fn decoded_image_cached(&self, asset_id: &str) -> Result<bool, NativeVnHostError> {
        self.asset_store.is_image_cached(asset_id)
    }

    pub fn take_ui_host_request(&mut self) -> Option<VnUiHostRequest> {
        self.pending_ui_host_request.take()
    }

    /// Dispatch physical input and capture the last gameplay frame before the
    /// first system-page presentation (or a quick-save request). Hosts must cache
    /// any Captured result before handling the resulting save request.
    pub fn prepare_ui_input(
        &mut self,
        kind: UiInputEventKind,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let gameplay = self
            .runtime_state
            .as_ref()
            .is_some_and(|state| state.system_stack.is_empty());
        let capture_sequence = if gameplay {
            Some(self.next_command_sequence()?)
        } else {
            None
        };
        let mut batch = self.dispatch_ui_event(kind)?;
        let opens_system = self
            .runtime_state
            .as_ref()
            .is_some_and(|state| !state.system_stack.is_empty());
        let saves_gameplay = matches!(
            self.pending_ui_host_request,
            Some(VnUiHostRequest::Save { .. })
        );
        if let Some(sequence) = capture_sequence.filter(|_| opens_system || saves_gameplay) {
            batch.commands.insert(
                0,
                PlayerHostCommand::CaptureSurface {
                    sequence,
                    surface: self.surface,
                },
            );
        }
        Ok(batch)
    }

    pub fn prepare_surface_capture(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::CaptureSurface {
                sequence: self.next_command_sequence()?,
                surface: self.surface,
            },
        ])?)
    }

    pub fn has_gameplay_thumbnail_capture(&self) -> bool {
        self.gameplay_thumbnail_capture.is_some()
    }

    pub fn cache_gameplay_surface(
        &mut self,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    ) -> Result<(), NativeVnHostError> {
        const THUMBNAIL_WIDTH: u32 = 160;
        const THUMBNAIL_HEIGHT: u32 = 120;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_CAPTURE_DIMENSIONS: captured gameplay surface dimensions are invalid"
                    .into(),
            ));
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| {
                NativeVnHostError::Save(
                    "ASTRA_PLAYER_SAVE_CAPTURE_SIZE: captured gameplay surface size overflowed"
                        .into(),
                )
            })?;
        if rgba8.len() != expected {
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_SAVE_CAPTURE_BYTES: captured gameplay surface expected {expected} bytes but received {}",
                rgba8.len()
            )));
        }
        let source = image::RgbaImage::from_raw(width, height, rgba8).ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_CAPTURE_IMAGE: captured gameplay surface could not form an RGBA image"
                    .into(),
            )
        })?;
        let thumbnail = image::imageops::resize(
            &source,
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        );
        let rgba8 = thumbnail.into_raw();
        self.gameplay_thumbnail_capture = Some(
            TextureFrame::from_vec(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT, rgba8).map_err(|error| {
                NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_CAPTURE_TEXTURE: {error}"))
            })?,
        );
        tracing::debug!(
            event = "player.vn.save.thumbnail_cached",
            width = THUMBNAIL_WIDTH,
            height = THUMBNAIL_HEIGHT,
            "cached a bounded gameplay thumbnail from the final composed surface"
        );
        Ok(())
    }

    pub fn prepare_save_metadata(
        &mut self,
        slot_id: &str,
        timestamp_text: String,
        playtime_ms: u64,
    ) -> Result<(), NativeVnHostError> {
        if !self.ui_save_slots.contains_key(slot_id) {
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            )));
        }
        if !self.ui_save_slots[slot_id].can_write {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_PROTECTED: unreadable save cannot be overwritten".into(),
            ));
        }
        if timestamp_text.trim().is_empty() || timestamp_text.len() > 64 {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_TIMESTAMP: save timestamp must be a bounded non-empty string"
                    .into(),
            ));
        }
        let thumbnail = self.gameplay_thumbnail_capture.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_THUMBNAIL_MISSING: no gameplay surface was captured before opening the save UI"
                    .into(),
            )
        })?;
        self.pending_save_metadata = Some(NativeVnSaveMetadata {
            slot_id: slot_id.to_string(),
            thumbnail_asset: format!("astra.internal.save_thumbnail.{slot_id}"),
            thumbnail,
            timestamp_text,
            playtime_text: format_playtime(playtime_ms),
        });
        if self.pending_save_completion.is_none()
            && self.system_ui_policy.quick_slot_id.as_deref() == Some(slot_id)
        {
            self.pending_save_completion = Some(SaveCompletionPolicy::Stay);
        }
        Ok(())
    }

    pub fn exit_requested(&self) -> bool {
        self.exit_requested
    }

    pub fn ui_semantics(&self) -> Option<&UiSemanticSnapshot> {
        self.ui_semantics.as_ref()
    }

    pub fn product_observation_evidence(
        &self,
    ) -> Result<NativeVnProductObservationEvidence, NativeVnHostError> {
        let state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_STATE_MISSING: product observation requires runtime state".into(),
            )
        })?;
        Ok(NativeVnProductObservationEvidence {
            schema: "astra.player_vn_product_observation.v3".into(),
            ui_profile: self.ui_profile.clone(),
            locale: state.locale.clone(),
            active_system_page: state.system_stack.last().map(|frame| frame.page),
            focused_semantic_id: self.ui_semantics.as_ref().and_then(|snapshot| {
                snapshot
                    .nodes
                    .iter()
                    .find(|node| node.focused)
                    .map(|node| node.id.clone())
            }),
            auto_enabled: state.system.auto_enabled,
            skip_mode: state.system.skip_mode,
            reading_mode: state.system.reading_mode,
            audio_enabled: state.system.audio_enabled,
            skip_allowed: state.system.skip_allowed,
            system_config: state.system.config.clone(),
            text_reveal_complete: self
                .stage_director
                .text_reveal_state()
                .is_none_or(|text| text.complete()),
            backlog_count: self.runtime_backlog_count,
            occupied_save_slot_count: self
                .ui_save_slots
                .values()
                .filter(|slot| slot.occupied)
                .count(),
        })
    }

    pub fn mark_save_committed(
        &mut self,
        slot_id: &str,
    ) -> Result<Option<PlayerHostCommandBatch>, NativeVnHostError> {
        let metadata = self.pending_save_metadata.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_MISSING: committed save has no prepared metadata"
                    .into(),
            )
        })?;
        if metadata.slot_id != slot_id {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: prepared metadata belongs to another slot".into(),
            ));
        }
        {
            let slot = self.ui_save_slots.get_mut(slot_id).ok_or_else(|| {
                NativeVnHostError::Save(format!(
                    "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
                ))
            })?;
            slot.occupied = true;
            slot.thumbnail_asset = Some(metadata.thumbnail_asset.clone());
            slot.has_thumbnail = true;
            slot.timestamp_text = Some(metadata.timestamp_text.clone());
            slot.playtime_text = Some(metadata.playtime_text.clone());
            slot.metadata_text = Some(format!(
                "{} | {}",
                metadata.timestamp_text, metadata.playtime_text
            ));
            slot.can_load = true;
            slot.migration_status = "current".to_string();
        }
        self.ui_backend
            .renderer_mut()
            .upsert_image_resource(metadata.thumbnail_asset.clone(), metadata.thumbnail.clone())?;
        self.textures
            .insert(metadata.thumbnail_asset, metadata.thumbnail);
        match self.pending_save_completion.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_COMPLETION_MISSING: committed save has no declared completion policy".into(),
            )
        })? {
            SaveCompletionPolicy::Stay => Ok(None),
            SaveCompletionPolicy::ReturnSystem => self
                .command(VnPlayerCommand::ReturnSystem)
                .map(Some),
        }
    }

    pub fn mark_save_failed(&mut self, slot_id: &str) -> Result<(), NativeVnHostError> {
        let metadata = self.pending_save_metadata.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_MISSING: failed save has no prepared metadata".into(),
            )
        })?;
        if metadata.slot_id != slot_id {
            self.pending_save_metadata = Some(metadata);
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: failed save belongs to another slot".into(),
            ));
        }
        self.pending_save_completion.take().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_COMPLETION_MISSING: failed save has no declared completion policy"
                    .into(),
            )
        })?;
        tracing::warn!(
            event = "vn.save.persistence_failed",
            slot_id,
            "save persistence failed; the current system page remains open"
        );
        Ok(())
    }

    pub fn mark_save_deleted(&mut self, slot_id: &str) -> Result<(), NativeVnHostError> {
        let slot = self.ui_save_slots.get_mut(slot_id).ok_or_else(|| {
            NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            ))
        })?;
        let thumbnail_asset = slot.thumbnail_asset.take();
        slot.occupied = false;
        slot.can_load = false;
        slot.has_thumbnail = false;
        slot.title_key = None;
        slot.timestamp_text = None;
        slot.playtime_text = None;
        slot.metadata_text = None;
        slot.migration_status = "empty".to_string();
        if let Some(thumbnail_asset) = thumbnail_asset {
            self.ui_backend
                .renderer_mut()
                .remove_image_resource(&thumbnail_asset);
            self.remove_texture(&thumbnail_asset)?;
        }
        Ok(())
    }

    pub fn pending_wait(&self) -> Option<&astra_vn_core::VnWaitState> {
        self.runtime_state
            .as_ref()
            .and_then(|state| state.pending_wait.as_ref())
    }
}
