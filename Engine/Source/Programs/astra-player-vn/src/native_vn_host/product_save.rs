use super::*;
use crate::{NativeVnProductMediaHost, NativeVnProductMediaSnapshot};

#[derive(Clone, serde::Serialize)]
pub(super) struct NativeVnRestoreState {
    pub runtime: astra_runtime::SaveBlob,
    pub stage_director: ProductStageDirector,
    pub director_transition_snapshot: Option<SavedDirectorTransitionSnapshot>,
    pub step_evidence: NativeVnStepEvidence,
}
impl NativeVnHostCommandSource {
    pub fn prepare_product_save_transaction(
        &mut self,
        slot: impl Into<String>,
        transaction: PlayerHostResourceId,
        media: &NativeVnProductMediaHost,
    ) -> Result<PlayerSaveTransactionPlan, NativeVnHostError> {
        let snapshot = serde_json::to_vec(&media.snapshot())
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        self.prepare_save_transaction_with_product_media_snapshot(slot, transaction, Some(snapshot))
    }

    pub(crate) fn restored_audio_request(
        &self,
        asset_id: &str,
    ) -> Result<crate::NativeVnAudioPreloadRequest, NativeVnHostError> {
        let asset = self.asset_store.load_media(asset_id)?;
        Ok(crate::NativeVnAudioPreloadRequest {
            asset_id: asset_id.into(),
            codec: asset.codec.clone(),
            encoded_bytes: asset.bytes.clone(),
            encoded_length: asset.byte_length,
        })
    }

    /// Load a user-selected slot, retaining the current session if its candidate is rejected.
    pub async fn load_product_session(
        &mut self,
        slot: &str,
        bytes: &[u8],
        media: &mut NativeVnProductMediaHost,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let previous_scope = self.media_scope.child();
        let result = match decode_save_envelope(bytes) {
            Ok(envelope) if envelope.payload.slot == slot => {
                self.restore_product_session(bytes, media, executor).await
            }
            Ok(_) => Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SLOT_MISMATCH: save belongs to another slot".into(),
            )),
            Err(error) => Err(error),
        };
        match result {
            Ok(batch) => Ok(batch),
            Err(error) if self.presentation_failed || previous_scope.is_cancelled() => Err(error),
            Err(_) => {
                tracing::warn!(
                    target: "astra_player_vn::save",
                    event = "player.save.candidate_rejected",
                    diagnostic_code = "ASTRA_PLAYER_SAVE_CANDIDATE_REJECTED",
                    "Saved candidate rejected; current session retained"
                );
                self.reject_save_catalog_entry(slot);
                // Re-open the same view against the updated read-only catalog so
                // its controller selects an enabled target, without changing story state.
                self.base_ui_instance_id = None;
                self.pending_ui_focus = None;
                self.render_with_stage_refresh(&[], 0, true)
            }
        }
    }

    pub async fn restore_product_session(
        &mut self,
        bytes: &[u8],
        media: &mut NativeVnProductMediaHost,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let envelope = decode_save_envelope(bytes)?;
        if envelope.payload.session_id != self.session_id {
            return Err(NativeVnHostError::Save(
                "ASTRA_PLAYER_SAVE_SESSION_MISMATCH: save belongs to another runtime session"
                    .into(),
            ));
        }
        self.host.validate_save(&envelope.payload.runtime)?;
        let encoded = envelope
            .payload
            .product_media_snapshot_json
            .ok_or_else(|| {
                NativeVnHostError::Save(
                    "ASTRA_PLAYER_SAVE_MEDIA_SNAPSHOT_MISSING: product save has no media state"
                        .into(),
                )
            })?;
        let snapshot: NativeVnProductMediaSnapshot =
            serde_json::from_slice(&encoded).map_err(|error| {
                NativeVnHostError::Save(format!("ASTRA_PLAYER_MEDIA_SNAPSHOT_INVALID: {error}"))
            })?;
        media
            .prepare_restore_assets(self, executor, &snapshot)
            .await
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        media
            .validate_restore(&snapshot)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        let mut present = self.restore(bytes)?;
        if let Err(error) = media.restore_product_media(self, executor, snapshot).await {
            self.presentation_failed = true;
            self.media_scope.cancel();
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_MEDIA_RESTORE_FAILED: {error}"
            )));
        }
        // A user load resumes gameplay, not the transient system page from which
        // the save was requested. Explicit preview checkpoints use restore directly.
        while self
            .runtime_state
            .as_ref()
            .is_some_and(|state| !state.system_stack.is_empty())
        {
            present
                .commands
                .extend(self.command(VnPlayerCommand::ReturnSystem)?.commands);
        }
        Ok(PlayerHostCommandBatch::new(present.commands)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_native_package;

    fn executor(
    ) -> astra_player_core::PlayerHostCommandExecutor<astra_player_core::PlatformCommandSink> {
        let (client, _, _) = astra_platform::host_channel(
            astra_platform::PlatformHostProfile::windows_release(
                "nativevn-game",
                "com.example.player",
            ),
            1,
            1,
        )
        .unwrap();
        astra_player_core::PlayerHostCommandExecutor::new(
            astra_player_core::PlatformCommandSink::new(client),
        )
    }

    fn source() -> NativeVnHostCommandSource {
        let bytes = test_native_package::product_package_with_ui_and_request(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\nstory system #@id story.system\nstate save #@id state.system.save\n  scene save #@id scene.system.save\n    system_page kind:save policy:astra.policy.standard #@id page.save\n",
            &test_native_package::TEST_UI.replace("save_slots:\"slot.01\"", "save_slots:\"slot.01,slot.02\""),
            test_native_package::test_compile_options(), |_| {},
        );
        let package = astra_package::PackageReader::open(&bytes).unwrap();
        let mut source = NativeVnHostCommandSource::from_package(
            &package,
            VnRunConfig::classic("en"),
            320,
            180,
            PlayerHostResourceId(1),
        )
        .unwrap();
        source.launch().unwrap();
        source
            .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
            .unwrap();
        source
            .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 0)
            .unwrap();
        source
    }

    fn saved(source: &mut NativeVnHostCommandSource, media: &NativeVnProductMediaHost) -> Vec<u8> {
        let plan = source
            .prepare_product_save_transaction("slot.01", PlayerHostResourceId(90), media)
            .unwrap();
        let [PlayerHostCommand::WriteSave { bytes, .. }] = plan.write.commands.as_slice() else {
            panic!("expected save write")
        };
        bytes.clone()
    }

    #[test]
    fn v9_runtime_container_roundtrips_and_rejects_v8_corruption_and_foreign_identity() {
        use astra_plugin_abi::{RuntimeSaveSections, RuntimeSectionCodec, RuntimeSectionPayload};
        let mut source = source();
        let bytes = source.save("slot.01").unwrap();
        let envelope = decode_save_envelope(&bytes).unwrap();
        assert_eq!(envelope.schema, "astra.player.native_vn_save.v9");
        for schema in [
            "astra.player.native_vn_save.v7",
            "astra.player.native_vn_save.v8",
        ] {
            // No valid payload follows: version rejection must precede payload decoding.
            let old_bytes = postcard::to_allocvec(schema).unwrap();
            let before = old_bytes.clone();
            let error = source.restore(&old_bytes).unwrap_err().to_string();
            assert!(error.contains("ASTRA_PLAYER_SAVE_SCHEMA"));
            assert_eq!(old_bytes, before);
        }
        let mut cold = self::source();
        cold.restore(&bytes).unwrap();
        let restored = cold.runtime_state.as_ref().unwrap();
        let original = source.runtime_state.as_ref().unwrap();
        assert_eq!(restored.cursor, original.cursor);
        assert_eq!(restored.pending_wait, original.pending_wait);
        cold.cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
            .unwrap();
        cold.prepare_save_metadata("slot.01", "2000-01-01T00:00:01Z".into(), 1)
            .unwrap();
        let resaved = cold.save("slot.01").unwrap();
        cold.restore(&resaved).unwrap();
        cold.release_resources().unwrap();
        cold.shutdown().unwrap();
        let state = source.runtime_state.clone();
        let scope = source.media_scope.child();
        let payload = &envelope.payload;
        let legacy_sections = RuntimeSaveSections {
            session_id: payload.session_id.clone(),
            sections: vec![RuntimeSectionPayload {
                section_id: "runtime.world".into(),
                schema: "astra.runtime.save_blob.v5".into(),
                version: astra_core::SchemaVersion::new(5, 0, 0),
                codec: RuntimeSectionCodec::Raw,
                hash: Hash256::from_sha256(&payload.runtime.0),
                bytes: payload.runtime.0.clone(),
            }],
            diagnostics: vec![],
        };
        // Preserve rejection of the actual v7 field layout as well.
        let legacy = postcard::to_allocvec(&(
            "astra.player.native_vn_save.v7",
            (
                "astra.player.native_vn_save_payload.v7",
                &payload.slot,
                legacy_sections,
                &payload.stage_director,
                &payload.director_transition_snapshot,
                &payload.step_evidence,
                &payload.product_media_snapshot_json,
                &payload.save_metadata,
            ),
        ))
        .unwrap();
        assert!(source.restore(&legacy).is_err());
        for variant in 0..3 {
            let mut invalid = decode_save_envelope(&bytes).unwrap();
            match variant {
                0 => invalid.payload.session_id = GameRuntimeSessionId("foreign".into()),
                1 => {
                    let index = invalid.payload.runtime.0.len() - 1;
                    invalid.payload.runtime.0[index] ^= 1;
                }
                _ => invalid.payload.schema = "astra.player.native_vn_save_payload.v7".into(),
            }
            assert!(source
                .restore(&postcard::to_allocvec(&invalid).unwrap())
                .is_err());
            assert_eq!(source.runtime_state, state);
            assert!(!scope.is_cancelled());
        }
        source
            .command(VnPlayerCommand::SetAudioEnabled { enabled: false })
            .unwrap();
        source.restore(&bytes).unwrap();
        assert_eq!(
            source.runtime_state.as_ref().unwrap().system,
            state.as_ref().unwrap().system
        );
        assert!(scope.is_cancelled());
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[tokio::test]
    async fn foreign_catalog_and_rejected_user_load_preserve_the_live_session() {
        let mut source = source();
        let mut executor = executor();
        let mut media = NativeVnProductMediaHost::default();
        let bytes = saved(&mut source, &media);
        let mut envelope = decode_save_envelope(&bytes).unwrap();
        let mut snapshot = astra_runtime::read_runtime_save(
            &envelope.payload.runtime,
            &astra_core::SchemaMigrationRegistry::default(),
        )
        .unwrap();
        snapshot.package.as_mut().unwrap().package_id = "foreign.package".into();
        envelope.payload.runtime =
            astra_runtime::write_runtime_save(snapshot, astra_runtime::SaveRequest::default())
                .unwrap();
        let foreign = postcard::to_allocvec(&envelope).unwrap();
        source
            .command(VnPlayerCommand::OpenSystem {
                page: SystemPageKind::Save,
            })
            .unwrap();
        let state = source.runtime_state.clone();
        let scope = source.media_scope.child();
        let media_before = serde_json::to_value(media.snapshot()).unwrap();
        assert!(source
            .ingest_save_catalog_entry("slot.01", &foreign)
            .is_err());
        assert!(!source.ui_save_slots["slot.01"].can_load);
        for invalid in [&foreign[..], &[0xff][..]] {
            source
                .load_product_session("slot.01", invalid, &mut media, &mut executor)
                .await
                .unwrap();
            assert_eq!(source.runtime_state, state);
            assert!(!scope.is_cancelled());
            assert!(!source.presentation_failed);
            assert_eq!(
                serde_json::to_value(media.snapshot()).unwrap(),
                media_before
            );
            let rejected = &source.ui_save_slots["slot.01"];
            assert!(rejected.occupied);
            assert!(!rejected.can_load && !rejected.can_write);
        }
        source
            .command(VnPlayerCommand::SetAudioEnabled { enabled: false })
            .unwrap();
        source
            .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
            .unwrap();
        source
            .prepare_save_metadata("slot.02", "2000-01-01T00:00:01Z".into(), 1)
            .unwrap();
        assert!(source.save("slot.02").is_ok());
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[tokio::test]
    async fn user_load_returns_to_gameplay_but_explicit_restore_preserves_system_page() {
        let mut source = source();
        let mut executor = executor();
        let mut media = NativeVnProductMediaHost::default();
        let gameplay = source.runtime_state.clone().unwrap();
        source
            .command(VnPlayerCommand::OpenSystem {
                page: SystemPageKind::Save,
            })
            .unwrap();
        let bytes = saved(&mut source, &media);
        source.restore(&bytes).unwrap();
        assert!(!source
            .runtime_state
            .as_ref()
            .unwrap()
            .system_stack
            .is_empty());
        let batch = source
            .restore_product_session(&bytes, &mut media, &mut executor)
            .await
            .unwrap();
        assert!(batch.commands.len() >= 2);
        let restored = source.runtime_state.as_ref().unwrap();
        assert!(restored.system_stack.is_empty());
        assert_eq!(restored.cursor, gameplay.cursor);
        assert_eq!(restored.pending_wait, gameplay.pending_wait);
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[tokio::test]
    async fn product_save_restores_media_instead_of_retaining_live_signals() {
        let mut source = source();
        let mut executor = executor();
        let mut media = NativeVnProductMediaHost::default();
        let mut expected = media.snapshot();
        expected.completed_signals.push("saved.done".into());
        expected.audio.known_bgm_targets.insert("saved.bgm".into());
        expected.timeline.last_time_ms = Some(10);
        expected.playback_time_ms = 10;
        media.restore(expected.clone()).unwrap();
        let bytes = saved(&mut source, &media);
        let mut changed = media.snapshot();
        changed.completed_signals = vec!["old.live.done".into()];
        changed.audio.known_bgm_targets.clear();
        changed.timeline.last_time_ms = Some(20);
        changed.playback_time_ms = 20;
        media.restore(changed).unwrap();
        source
            .restore_product_session(&bytes, &mut media, &mut executor)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(media.snapshot()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[tokio::test]
    async fn invalid_product_media_is_rejected_before_the_source_commits() {
        let mut source = source();
        let mut executor = executor();
        let mut media = NativeVnProductMediaHost::default();
        let bytes = saved(&mut source, &media);
        let expected = serde_json::to_value(media.snapshot()).unwrap();
        let scope = source.media_scope.child();
        for variant in 0..4 {
            let mut envelope = decode_save_envelope(&bytes).unwrap();
            if variant == 0 {
                envelope.payload.product_media_snapshot_json = None;
            } else {
                let mut snapshot = media.snapshot();
                match variant {
                    1 => snapshot.schema = "invalid".into(),
                    2 => snapshot.timeline.schema = "invalid".into(),
                    _ => snapshot.audio.timeline.device_channels = 0,
                }
                envelope.payload.product_media_snapshot_json =
                    Some(serde_json::to_vec(&snapshot).unwrap());
            }
            let invalid = postcard::to_allocvec(&envelope).unwrap();
            assert!(source
                .restore_product_session(&invalid, &mut media, &mut executor)
                .await
                .is_err());
            assert!(!scope.is_cancelled());
            assert!(!source.presentation_failed);
            assert_eq!(serde_json::to_value(media.snapshot()).unwrap(), expected);
        }
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }
}
