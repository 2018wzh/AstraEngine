use super::*;
use crate::{NativeVnProductMediaHost, NativeVnProductMediaSnapshot};

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
        let present = self.restore(bytes)?;
        if let Err(error) = media.restore_product_media(self, executor, snapshot).await {
            self.presentation_failed = true;
            self.media_scope.cancel();
            return Err(NativeVnHostError::Save(format!(
                "ASTRA_PLAYER_MEDIA_RESTORE_FAILED: {error}"
            )));
        }
        Ok(present)
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
        let bytes = test_native_package::product_package_with_request(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n", |_| {},
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
    fn v8_runtime_container_roundtrips_and_rejects_v7_corruption_and_foreign_identity() {
        use astra_plugin_abi::{RuntimeSaveSections, RuntimeSectionCodec, RuntimeSectionPayload};
        let mut source = source();
        let bytes = source.save("slot.01").unwrap();
        let envelope = decode_save_envelope(&bytes).unwrap();
        assert_eq!(envelope.schema, "astra.player.native_vn_save.v8");
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
        // Serialize the actual v7 field layout, not just a renamed v8 envelope.
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
