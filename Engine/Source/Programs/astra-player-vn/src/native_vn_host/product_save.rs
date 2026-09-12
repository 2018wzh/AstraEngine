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

    pub fn restore_product_session(
        &mut self,
        bytes: &[u8],
        media: &mut NativeVnProductMediaHost,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let envelope = decode_save_envelope(bytes)?;
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
            .validate_restore(&snapshot)
            .map_err(|error| NativeVnHostError::Save(error.to_string()))?;
        let present = self.restore(bytes)?;
        if let Err(error) = media.restore(snapshot) {
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
    fn product_save_restores_media_instead_of_retaining_live_signals() {
        let mut source = source();
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
        source.restore_product_session(&bytes, &mut media).unwrap();
        assert_eq!(
            serde_json::to_value(media.snapshot()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[test]
    fn invalid_product_media_is_rejected_before_the_source_commits() {
        let mut source = source();
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
                .restore_product_session(&invalid, &mut media)
                .is_err());
            assert!(!scope.is_cancelled());
            assert!(!source.presentation_failed);
            assert_eq!(serde_json::to_value(media.snapshot()).unwrap(), expected);
        }
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }
}
