use super::stage::*;
use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn save(&mut self, slot: impl Into<String>) -> Result<Vec<u8>, NativeVnHostError> {
        self.save_with_product_media_snapshot(slot, None)
    }

    pub fn save_with_product_media_snapshot(
        &mut self,
        slot: impl Into<String>,
        product_media_snapshot_json: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, NativeVnHostError> {
        self.ensure_presentation_active()?;
        let slot = slot.into();
        if slot.trim().is_empty() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_INVALID: save slot must not be empty".into(),
            ));
        }
        if self
            .ui_save_slots
            .get(&slot)
            .is_some_and(|entry| !entry.can_write)
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_PROTECTED: unreadable save cannot be overwritten".into(),
            ));
        }
        if self.runtime_state.is_none() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_STATE_MISSING: runtime has not launched".into(),
            ));
        }
        let step_evidence = self.last_step_evidence.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_EVIDENCE_MISSING: runtime has no completed step".into(),
            )
        })?;
        let runtime = self.host.save()?;
        let save_metadata = self.pending_save_metadata.clone().ok_or_else(|| {
            NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_MISSING: save serialization requires prepared metadata"
                    .into(),
            )
        })?;
        if save_metadata.slot_id != slot {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: prepared metadata belongs to another slot".into(),
            ));
        }
        let payload = NativeVnPlayerSavePayload {
            schema: "astra.player.native_vn_save_payload.v10".into(),
            slot,
            session_id: self.session_id.clone(),
            runtime,
            stage_director: self.stage_director.clone(),
            director_transition_snapshot: save_director_transition_snapshot(
                self.director_transition_snapshot.as_ref(),
            )?,
            step_evidence,
            product_media_snapshot_json,
            save_metadata: save_metadata_for_persistence(&save_metadata),
        };
        postcard::to_allocvec(&NativeVnPlayerSaveEnvelope {
            schema: "astra.player.native_vn_save.v10".into(),
            payload,
        })
        .map_err(|error| NativeVnHostError::Save(error.to_string()))
    }

    pub fn restore(&mut self, bytes: &[u8]) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let mut committed = false;
        let result = self.restore_inner(bytes, &mut committed);
        if committed {
            self.presentation_failed = result.is_err();
            if self.presentation_failed {
                self.media_scope.cancel();
            }
        }
        result
    }

    pub(super) fn restore_inner(
        &mut self,
        bytes: &[u8],
        committed: &mut bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let envelope = decode_save_envelope(bytes)?;
        if envelope.payload.session_id != self.session_id {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SESSION_MISMATCH: save belongs to another runtime session"
                    .into(),
            ));
        }
        let save_metadata = restore_save_metadata(envelope.payload.save_metadata.clone())?;
        validate_save_metadata(&save_metadata, &envelope.payload.slot)?;
        let state = product_save::NativeVnRestoreState {
            runtime: envelope.payload.runtime,
            stage_director: envelope.payload.stage_director,
            director_transition_snapshot: envelope.payload.director_transition_snapshot,
            step_evidence: envelope.payload.step_evidence,
        };
        self.restore_explicit_state(state, Some(save_metadata), committed)
    }

    pub(super) fn restore_explicit_state(
        &mut self,
        state: product_save::NativeVnRestoreState,
        save_metadata: Option<NativeVnSaveMetadata>,
        committed: &mut bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let restored_runtime_state = saved_runtime_state(&state.runtime)?;
        let restored_locale = restored_runtime_state.locale.clone();
        if !self.localizations.contains_key(&restored_locale) {
            return Err(NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_RESTORE_LOCALE_UNAVAILABLE: locale {restored_locale} is not packaged"
            )));
        }
        // Decode and validate every saved presentation reference before World changes.
        self.stage_director
            .validate_restore_candidate(&state.stage_director)
            .map_err(stage_director_error)?;
        if save_metadata
            .as_ref()
            .is_some_and(|metadata| !self.ui_save_slots.contains_key(&metadata.slot_id))
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_METADATA_SLOT: metadata references an undeclared slot".into(),
            ));
        }
        let next_ui_generation = self
            .ui_generation
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        self.command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        let mut required = presentation::stage_texture_requirements(
            state.stage_director.state(),
            &BTreeMap::new(),
        )
        .0;
        let saved_transition_id = state
            .stage_director
            .state()
            .transition
            .as_ref()
            .and_then(|transition| transition.descriptor_id.as_deref());
        if saved_transition_id
            != state
                .director_transition_snapshot
                .as_ref()
                .map(|snapshot| snapshot.descriptor_id.as_str())
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_RESTORE_TRANSITION_IDENTITY".into(),
            ));
        }
        if let Some(snapshot) = state.director_transition_snapshot.as_ref() {
            let target = state.stage_director.state();
            if snapshot.source_state.schema != target.schema
                || snapshot.source_state.profile != target.profile
                || target
                    .transition
                    .as_ref()
                    .and_then(|transition| transition.descriptor_id.as_deref())
                    != Some(snapshot.descriptor_id.as_str())
            {
                return Err(NativeVnHostError::Save(
                    "ASTRA_PLAYER_RESTORE_TRANSITION_IDENTITY".into(),
                ));
            }
            required.extend(
                presentation::stage_texture_requirements(&snapshot.source_state, &BTreeMap::new())
                    .0,
            );
        }
        let mut prepared_textures = BTreeMap::new();
        let mut prepared_dimensions = BTreeMap::new();
        let mut prepared_bytes = 0u64;
        for asset_id in &required {
            let frame = self.stage_texture_for_viewport(self.asset_store.load_image(asset_id)?)?;
            prepared_bytes = prepared_bytes
                .checked_add(frame.rgba8.len() as u64)
                .ok_or(NativeVnHostError::SequenceOverflow)?;
            if prepared_bytes > self.texture_cpu_budget_bytes
                || prepared_bytes > self.gpu_texture_budget_bytes
            {
                return Err(NativeVnHostError::Save(
                    "ASTRA_PLAYER_RESTORE_TEXTURE_BUDGET".into(),
                ));
            }
            prepared_dimensions.insert(asset_id.clone(), (frame.width, frame.height));
            prepared_textures.insert(asset_id.clone(), frame);
        }
        stage_scene_commands(
            state.stage_director.state(),
            &prepared_textures,
            &prepared_dimensions,
            self.width,
            self.height,
        )?;
        let restored_transition_snapshot = restore_director_transition_snapshot(
            state.director_transition_snapshot.as_ref(),
            &prepared_textures,
            &prepared_dimensions,
            self.width,
            self.height,
        )?;
        let report = self.host.restore(state.runtime)?;
        *committed = true;
        self.reset_pending_work();
        self.image_prefetcher.reset_generation();
        self.image_prefetch_inflight.clear();
        self.image_prefetch_failure = None;
        self.fixed_step = report.step;
        self.session_seed = report.seed;
        self.next_step_mode = astra_runtime::TickMode::RestoreContinuation;
        self.runtime_state = Some(restored_runtime_state);
        self.runtime_backlog_count = self
            .runtime_state
            .as_ref()
            .map_or(0, |state| state.backlog.len());
        self.activate_locale(&restored_locale)?;
        self.stage_director = state.stage_director;
        for (asset_id, frame) in prepared_textures {
            self.store_texture(asset_id, frame, &required)?;
        }
        self.director_transition_snapshot = restored_transition_snapshot;
        self.last_step_evidence = Some(state.step_evidence);
        if let Some(metadata) = save_metadata {
            self.apply_save_metadata(metadata)?;
        }
        self.ui_controller_sessions.clear();
        self.base_ui_instance_id = None;
        self.base_ui_theme_id = None;
        self.active_ui_controller = None;
        self.ui_modals.clear();
        self.pending_ui_focus = None;
        self.ui_semantics = None;
        self.ui_animations.clear();
        self.ui_text_measurer.begin_frame()?;
        let mut restore_lifecycle = Vec::new();
        for layout_id in self.live_layout_ids.iter().cloned().collect::<Vec<_>>() {
            restore_lifecycle.extend(self.text_resources.remove_layout(&layout_id)?);
        }
        self.live_layout_ids.clear();
        self.ui_generation = next_ui_generation;
        // The UI model has changed under the same session identity. The Yakui
        // backend emits an ordered release/resync transaction so the retained
        // Scene2D resources remain synchronized with this restored state.
        if self.ui_backend_started {
            self.ui_backend
                .context_restored(&format!("vn.ui.{}", self.session_id), self.ui_generation)
                .map_err(NativeVnHostError::Ui)?;
        }
        self.presentation_failed = false;
        let mut batch = self.render_with_stage_refresh(&[], 0, true)?;
        let [PlayerHostCommand::PresentScene { commands, .. }] = batch.commands.as_mut_slice()
        else {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_RESTORE_PRESENTATION_BATCH: restore render must emit exactly one scene"
                    .into(),
            ));
        };
        commands.splice(0..0, restore_lifecycle);
        Ok(batch)
    }

    pub fn prepare_save_transaction(
        &mut self,
        slot: impl Into<String>,
        transaction: PlayerHostResourceId,
    ) -> Result<PlayerSaveTransactionPlan, NativeVnHostError> {
        let slot = slot.into();
        self.prepare_save_transaction_with_product_media_snapshot(slot, transaction, None)
    }

    pub fn prepare_save_transaction_with_product_media_snapshot(
        &mut self,
        slot: impl Into<String>,
        transaction: PlayerHostResourceId,
        product_media_snapshot_json: Option<Vec<u8>>,
    ) -> Result<PlayerSaveTransactionPlan, NativeVnHostError> {
        let slot = slot.into();
        let bytes =
            self.save_with_product_media_snapshot(slot.clone(), product_media_snapshot_json)?;
        let begin = PlayerHostCommandBatch::new(vec![PlayerHostCommand::BeginSave {
            sequence: self.next_command_sequence()?,
            slot,
            transaction,
        }])?;
        let write = PlayerHostCommandBatch::new(vec![PlayerHostCommand::WriteSave {
            sequence: self.next_command_sequence()?,
            transaction,
            bytes,
        }])?;
        let commit = PlayerHostCommandBatch::new(vec![PlayerHostCommand::CommitSave {
            sequence: self.next_command_sequence()?,
            transaction,
        }])?;
        let abort = PlayerHostCommandBatch::new(vec![PlayerHostCommand::AbortSave {
            sequence: self.next_command_sequence()?,
            transaction,
        }])?;
        Ok(PlayerSaveTransactionPlan {
            begin,
            write,
            commit,
            abort,
        })
    }

    pub fn read_save(
        &mut self,
        slot: impl Into<String>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = slot.into();
        if slot.trim().is_empty() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_INVALID: save slot must not be empty".into(),
            ));
        }
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::ReadSave {
                sequence: self.next_command_sequence()?,
                slot,
            },
        ])?)
    }

    pub fn list_saves(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::ListSaves {
                sequence: self.next_command_sequence()?,
            },
        ])?)
    }

    pub fn ingest_save_catalog_entry(
        &mut self,
        expected_slot: &str,
        bytes: &[u8],
    ) -> Result<(), NativeVnHostError> {
        let envelope = decode_save_envelope(bytes)?;
        if envelope.payload.slot != expected_slot || envelope.payload.session_id != self.session_id
        {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_CATALOG_IDENTITY: catalog entry does not belong to the requested slot and session"
                    .into(),
            ));
        }
        let metadata = restore_save_metadata(envelope.payload.save_metadata)?;
        validate_save_metadata(&metadata, expected_slot)?;
        self.host.validate_save(&envelope.payload.runtime)?;
        self.apply_save_metadata(metadata)
    }

    pub(super) fn apply_save_metadata(
        &mut self,
        metadata: NativeVnSaveMetadata,
    ) -> Result<(), NativeVnHostError> {
        {
            let slot = self
                .ui_save_slots
                .get_mut(&metadata.slot_id)
                .ok_or_else(|| {
                    NativeVnHostError::Save(
                        "ASTRA_PLAYER_SAVE_METADATA_SLOT: metadata references an undeclared slot"
                            .into(),
                    )
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
            slot.migration_status = "current".into();
        }
        self.ui_backend
            .renderer_mut()
            .upsert_image_resource(metadata.thumbnail_asset.clone(), metadata.thumbnail.clone())?;
        self.textures
            .insert(metadata.thumbnail_asset, metadata.thumbnail);
        Ok(())
    }

    pub fn delete_save(
        &mut self,
        slot: impl Into<String>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = slot.into();
        if slot.trim().is_empty() {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_INVALID: save slot must not be empty".into(),
            ));
        }
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::DeleteSave {
                sequence: self.next_command_sequence()?,
                slot,
            },
        ])?)
    }

    pub(super) fn next_command_sequence(&mut self) -> Result<u64, NativeVnHostError> {
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(self.command_sequence)
    }

    pub(super) fn next_media_resource(
        &mut self,
    ) -> Result<PlayerHostResourceId, NativeVnHostError> {
        self.next_media_resource_id = self
            .next_media_resource_id
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(PlayerHostResourceId(self.next_media_resource_id))
    }
}
