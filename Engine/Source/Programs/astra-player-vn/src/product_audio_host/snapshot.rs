use super::*;

impl NativeVnProductAudioHost {
    pub fn last_meter(&self) -> Option<NativeVnAudioMeterSnapshot> {
        self.last_meter
    }

    pub fn submitted_timeline(&self) -> Result<Vec<f32>, PlatformError> {
        self.evidence_capture
            .as_ref()
            .map(AudioCaptureReader::take_samples)
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub fn snapshot(&self) -> NativeVnProductAudioSnapshot {
        let timeline = self
            .service
            .as_ref()
            .map(|service| service.timeline().clone())
            .or_else(|| self.pending_restore.clone())
            .unwrap_or_else(|| {
                AudioTimelineStateV1::new(CANONICAL_SAMPLE_RATE, CANONICAL_CHANNELS)
            });
        NativeVnProductAudioSnapshot {
            schema: "astra.audio_timeline.v1".into(),
            timeline,
            voice_kinds: self.voice_kinds.clone(),
            known_bgm_targets: self.known_bgm_targets.clone(),
            pending_fade_stops: self.pending_fade_stops.clone(),
        }
    }

    pub(crate) fn validate_snapshot_data(
        snapshot: &NativeVnProductAudioSnapshot,
    ) -> Result<(), PlatformError> {
        if snapshot.schema != "astra.audio_timeline.v1"
            || snapshot.timeline.schema != "astra.audio_timeline.v1"
            || snapshot.timeline.device_sample_rate != CANONICAL_SAMPLE_RATE
            || snapshot.timeline.device_channels != CANONICAL_CHANNELS
            || snapshot.voice_kinds.len() != snapshot.timeline.voices.len()
            || snapshot
                .voice_kinds
                .keys()
                .any(|voice_id| !snapshot.timeline.voices.contains_key(voice_id))
            || snapshot
                .pending_fade_stops
                .iter()
                .any(|(fade_id, pending)| {
                    pending.fence.is_empty()
                        || !snapshot.voice_kinds.contains_key(&pending.voice_id)
                        || !snapshot
                            .timeline
                            .buses
                            .values()
                            .any(|bus| bus.fade_id.as_deref() == Some(fade_id.as_str()))
                })
        {
            return Err(player_platform_error(
                "player.audio.restore",
                "ASTRA_PLAYER_AUDIO_TIMELINE_INVALID",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_restore(
        &self,
        snapshot: &NativeVnProductAudioSnapshot,
    ) -> Result<(), PlatformError> {
        Self::validate_snapshot_data(snapshot)?;
        if let Some(service) = self.service.as_ref() {
            service
                .validate_timeline_restore(&snapshot.timeline)
                .map_err(|error| player_platform_error("player.audio.restore.validate", error))?;
        } else if !snapshot.timeline.voices.is_empty()
            || snapshot
                .timeline
                .buses
                .values()
                .any(|bus| bus.fade_id.is_some())
        {
            return Err(player_platform_error(
                "player.audio.restore",
                "ASTRA_PLAYER_AUDIO_RESTORE_REQUIRES_OPEN_SESSION",
            ));
        }
        Ok(())
    }

    pub fn restore(&mut self, snapshot: NativeVnProductAudioSnapshot) -> Result<(), PlatformError> {
        self.validate_restore(&snapshot)?;
        if let Some(service) = self.service.as_mut() {
            service
                .restore_timeline(snapshot.timeline.clone())
                .map_err(|error| player_platform_error("player.audio.restore", error))?;
        } else {
            self.pending_restore = Some(snapshot.timeline.clone());
        }
        self.voice_kinds = snapshot.voice_kinds;
        self.known_bgm_targets = snapshot.known_bgm_targets;
        self.pending_fade_stops = snapshot.pending_fade_stops;
        Ok(())
    }
}
