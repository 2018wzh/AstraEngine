use super::*;

impl NativeVnProductMediaHost {
    pub(crate) async fn prepare_restore_assets(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        snapshot: &NativeVnProductMediaSnapshot,
    ) -> Result<(), PlatformError> {
        Self::validate_snapshot_data(snapshot)?;
        let package_id = executor
            .sink()
            .client()
            .launch_profile()
            .package_id()
            .to_owned();
        let mut required = BTreeMap::new();
        for voice in snapshot.audio.timeline.voices.values() {
            let revision = &voice.asset;
            if revision.package_id != package_id
                || revision.revision != package_id
                || required
                    .insert(revision.uri.clone(), revision.clone())
                    .is_some_and(|previous| previous != *revision)
            {
                return Err(media_error(
                    "player.audio.restore.asset",
                    "ASTRA_PLAYER_AUDIO_ASSET_REVISION_CONFLICT",
                ));
            }
        }
        if required.is_empty()
            && snapshot
                .audio
                .timeline
                .buses
                .values()
                .all(|bus| bus.fade_id.is_none())
        {
            return Ok(());
        }
        self.audio.ensure_open(source, executor).await?;
        for revision in required.into_values() {
            if self.audio.has_prepared_asset(&revision) {
                continue;
            }
            let request = source
                .restored_audio_request(&revision.uri)
                .map_err(|error| media_error("player.audio.restore.asset", error))?;
            let key = AudioCacheKey::new(&request.asset_id, &request.codec, request.encoded_length);
            let asset = if let Some(cached) = self.cached_audio(&key) {
                cached.asset
            } else {
                self.decode_audio(source, executor, &request).await?.asset
            };
            let bytes = (asset.samples.len() as u64)
                .checked_mul(size_of::<f32>() as u64)
                .ok_or_else(|| {
                    media_error(
                        "player.audio.restore.asset",
                        "ASTRA_PLAYER_AUDIO_PCM_LENGTH_OVERFLOW",
                    )
                })?;
            if bytes != revision.byte_len {
                return Err(media_error(
                    "player.audio.restore.asset",
                    "ASTRA_PLAYER_AUDIO_PCM_LENGTH_MISMATCH",
                ));
            }
            let prepared = self.audio.prepare_canonical_asset(asset, &package_id)?;
            if prepared != revision {
                return Err(media_error(
                    "player.audio.restore.asset",
                    "ASTRA_PLAYER_AUDIO_ASSET_REVISION_CONFLICT",
                ));
            }
        }
        self.audio.validate_restore(&snapshot.audio)
    }
}
