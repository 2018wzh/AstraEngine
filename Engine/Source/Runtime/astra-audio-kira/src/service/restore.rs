use super::*;

impl AudioServiceSession {
    /// Validate a save against this session without stopping voices or changing tracks.
    pub fn validate_timeline_restore(
        &self,
        snapshot: &AudioTimelineStateV1,
    ) -> Result<(), AudioServiceError> {
        self.ensure_healthy()?;
        if snapshot.schema != crate::timeline::AUDIO_TIMELINE_SCHEMA
            || snapshot.device_sample_rate != self.timeline.device_sample_rate
            || snapshot.device_channels != self.timeline.device_channels
            || snapshot.voices.len() > self.config.max_voices
        {
            return Err(AudioServiceError::InvalidCommand(
                "audio timeline schema, format or voice capacity is incompatible",
            ));
        }
        let new_buses = snapshot
            .buses
            .keys()
            .filter(|bus| !self.buses.contains_key(*bus))
            .count();
        if self
            .buses
            .len()
            .checked_add(new_buses)
            .is_none_or(|count| count > self.config.max_buses)
            || self
                .lru_sequence
                .checked_add(snapshot.voices.len() as u64)
                .is_none()
        {
            return Err(AudioServiceError::InvalidCommand(
                "audio restore bus capacity or LRU sequence is exhausted",
            ));
        }
        for (id, voice) in &snapshot.voices {
            if id.trim().is_empty()
                || !snapshot.buses.contains_key(&voice.bus)
                || voice.command_sequence == 0
                || voice.command_sequence > snapshot.command_sequence
            {
                return Err(AudioServiceError::InvalidCommand(
                    "restored voice identity, bus or sequence is invalid",
                ));
            }
            let pcm = self
                .pcm_cache
                .iter()
                .find(|(key, _)| key.asset == voice.asset)
                .map(|(_, pcm)| pcm)
                .ok_or(AudioServiceError::InvalidCommand(
                    "restored PCM asset is not prepared",
                ))?;
            let frames = pcm.samples.len() / usize::from(snapshot.device_channels);
            if voice.cursor_frames >= frames as u64 {
                return Err(AudioServiceError::InvalidCommand(
                    "restored PCM cursor is out of bounds",
                ));
            }
        }
        let valid_gain = |gain: f32| gain.is_finite() && gain >= 0.0;
        let mut fade_ids = std::collections::BTreeSet::new();
        for (id, bus) in &snapshot.buses {
            if id.trim().is_empty() || !valid_gain(bus.gain) {
                return Err(AudioServiceError::InvalidCommand(
                    "restored bus identity or gain is invalid",
                ));
            }
            match (&bus.fade_id, bus.fade_start_gain, bus.fade_target_gain) {
                (None, None, None)
                    if bus.fade_sequence == 0
                        && bus.fade_total_frames == 0
                        && bus.fade_rendered_frames == 0 => {}
                (Some(id), Some(start), Some(target))
                    if !id.trim().is_empty()
                        && fade_ids.insert(id)
                        && valid_gain(start)
                        && valid_gain(target)
                        && bus.fade_sequence > 0
                        && bus.fade_sequence <= snapshot.command_sequence
                        && bus.fade_rendered_frames < bus.fade_total_frames =>
                {
                    frames_to_duration(
                        bus.fade_total_frames - bus.fade_rendered_frames,
                        snapshot.device_sample_rate,
                    )?;
                }
                _ => {
                    return Err(AudioServiceError::InvalidCommand(
                        "restored fade state is inconsistent or already complete",
                    ))
                }
            }
        }
        Ok(())
    }
}
