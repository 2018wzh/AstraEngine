use super::*;

impl AudioServiceSession {
    pub fn stream_is_completed(&self, stream_id: u64) -> Result<bool, AudioServiceError> {
        self.streams
            .get(&stream_id)
            .map(|stream| stream.handle.is_completed())
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))
    }

    pub fn submit_stream_owned(
        &mut self,
        stream_id: u64,
        samples: astra_byte_source::OwnedF32Buffer,
    ) -> Result<(), AudioServiceError> {
        self.ensure_healthy()?;
        self.streams
            .get_mut(&stream_id)
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))?
            .handle
            .submit_owned(samples)
            .map_err(AudioServiceError::InvalidCommand)
    }

    pub fn submit_stream_recyclable(
        &mut self,
        stream_id: u64,
        samples: Vec<f32>,
    ) -> Result<Vec<f32>, AudioServiceError> {
        self.ensure_healthy()?;
        self.streams
            .get_mut(&stream_id)
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))?
            .handle
            .submit_recyclable(samples)
            .map_err(AudioServiceError::InvalidCommand)
    }

    pub fn pause_stream(&mut self, stream_id: u64) -> Result<(), AudioServiceError> {
        self.stream(stream_id)?.handle.pause();
        Ok(())
    }

    pub fn resume_stream(&mut self, stream_id: u64) -> Result<(), AudioServiceError> {
        self.stream(stream_id)?.handle.resume();
        Ok(())
    }

    pub fn finish_stream(&mut self, stream_id: u64) -> Result<(), AudioServiceError> {
        self.stream(stream_id)?.handle.finish();
        Ok(())
    }

    pub fn destroy_stream(&mut self, stream_id: u64) -> Result<(), AudioServiceError> {
        let stream = self
            .streams
            .remove(&stream_id)
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))?;
        stream.handle.stop();
        self.retiring_streams.push(stream.handle);
        Ok(())
    }

    #[must_use]
    pub fn stream_underflow_count(&self) -> u64 {
        self.streams
            .values()
            .map(|stream| stream.handle.underflow_count())
            .sum()
    }

    /// Rebuilds Kira handles from the persisted logical timeline. Prepared PCM revisions must
    /// already exist in the session cache; missing assets are a blocking restore error.
    pub fn restore_timeline(
        &mut self,
        snapshot: AudioTimelineStateV1,
    ) -> Result<(), AudioServiceError> {
        self.validate_timeline_restore(&snapshot)?;
        for voice in self.voices.values() {
            voice.handle.stop();
        }
        self.voices.clear();
        self.timeline.voices.clear();
        self.events.clear();

        for (bus, state) in &snapshot.buses {
            self.ensure_bus(bus)?;
            self.buses
                .get_mut(bus)
                .expect("restored bus exists")
                .track
                .set_volume(linear_gain_to_decibels(state.gain), Tween::default());
            self.timeline.buses.insert(bus.clone(), state.clone());
            if let (Some(target), Some(_fade_id)) =
                (state.fade_target_gain, state.fade_id.as_deref())
            {
                let remaining = state
                    .fade_total_frames
                    .saturating_sub(state.fade_rendered_frames);
                if remaining == 0 {
                    return Err(AudioServiceError::InvalidCommand(
                        "completed fade must not remain in a restored timeline",
                    ));
                }
                self.buses
                    .get_mut(bus)
                    .expect("restored bus exists")
                    .track
                    .set_volume(
                        linear_gain_to_decibels(target),
                        Tween {
                            duration: frames_to_duration(
                                remaining,
                                self.timeline.device_sample_rate,
                            )?,
                            ..Tween::default()
                        },
                    );
            }
        }
        for (voice_id, state) in &snapshot.voices {
            self.play(
                state.command_sequence,
                voice_id.clone(),
                state.bus.clone(),
                state.asset.clone(),
                state.cursor_frames,
                state.looping,
            )?;
            if state.paused {
                self.voice(voice_id)?.handle.pause();
                self.timeline
                    .voices
                    .get_mut(voice_id)
                    .expect("restored voice exists")
                    .paused = true;
            }
        }
        let consumed = self.manager.backend_mut().telemetry().consumed_samples
            / u64::from(self.timeline.device_channels);
        self.timeline.command_sequence = snapshot.command_sequence;
        self.timeline.consumed_frames = consumed;
        self.last_consumed_frames = consumed;
        Ok(())
    }

    pub fn poll_backend(&mut self) -> Result<(), AudioServiceError> {
        self.manager
            .backend_mut()
            .take_worker_error()
            .map_err(|error| {
                self.poisoned = true;
                AudioServiceError::Kira(error.to_string())
            })
    }

    #[must_use]
    pub fn telemetry(&mut self) -> crate::AudioChunkTelemetry {
        self.manager.backend_mut().telemetry()
    }

    #[must_use]
    pub fn config(&self) -> AudioServiceConfig {
        self.config
    }

    pub(super) fn play(
        &mut self,
        sequence: u64,
        voice_id: String,
        bus: String,
        asset: AudioAssetRevision,
        start_frame: u64,
        looping: bool,
    ) -> Result<(), AudioServiceError> {
        if self.voices.contains_key(&voice_id) || self.voices.len() == self.config.max_voices {
            return Err(AudioServiceError::InvalidCommand(
                "voice id is duplicate or voice capacity is exhausted",
            ));
        }
        let key = self
            .pcm_cache
            .keys()
            .find(|key| key.asset == asset)
            .cloned()
            .ok_or(AudioServiceError::InvalidCommand(
                "PCM asset is not prepared",
            ))?;
        self.ensure_bus(&bus)?;
        self.lru_sequence = self
            .lru_sequence
            .checked_add(1)
            .ok_or(AudioServiceError::InvalidCommand("LRU sequence overflowed"))?;
        let cached = self.pcm_cache.get_mut(&key).expect("cache key exists");
        cached.last_used = self.lru_sequence;
        let sound = AstraPcmSoundData::new(
            Arc::clone(&cached.samples),
            self.timeline.device_channels,
            start_frame,
            looping,
        )
        .map_err(AudioServiceError::InvalidCommand)?;
        let handle = self
            .buses
            .get_mut(&bus)
            .expect("created bus must exist")
            .track
            .play(sound)
            .map_err(|error| self.poison_kira(error.to_string()))?;
        self.voices.insert(
            voice_id.clone(),
            ActiveVoice {
                handle,
                command_sequence: sequence,
                completion_target_samples: None,
            },
        );
        self.timeline.voices.insert(
            voice_id,
            AudioVoiceState {
                command_sequence: sequence,
                bus,
                asset,
                cursor_frames: start_frame,
                looping,
                paused: false,
            },
        );
        Ok(())
    }

    pub(super) fn stream(
        &mut self,
        stream_id: u64,
    ) -> Result<&mut ActiveStream, AudioServiceError> {
        self.ensure_healthy()?;
        self.streams
            .get_mut(&stream_id)
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))
    }

    pub(super) fn ensure_bus(&mut self, bus: &str) -> Result<(), AudioServiceError> {
        if self.buses.contains_key(bus) {
            return Ok(());
        }
        if self.buses.len() == self.config.max_buses {
            return Err(AudioServiceError::InvalidCommand(
                "bus capacity is exhausted",
            ));
        }
        let mut builder = TrackBuilder::new()
            .sound_capacity(self.config.max_voices)
            .sub_track_capacity(self.config.max_voices);
        let panning = builder.add_effect(PanningControlBuilder::default());
        let track = self
            .manager
            .add_sub_track(builder)
            .map_err(|error| self.poison_kira(error.to_string()))?;
        self.buses
            .insert(bus.to_string(), AudioBus { track, panning });
        self.timeline.buses.insert(
            bus.to_string(),
            AudioBusState {
                gain: 1.0,
                fade_id: None,
                fade_sequence: 0,
                fade_start_gain: None,
                fade_target_gain: None,
                fade_total_frames: 0,
                fade_rendered_frames: 0,
            },
        );
        Ok(())
    }

    pub(super) fn voice(&self, voice_id: &str) -> Result<&ActiveVoice, AudioServiceError> {
        self.voices
            .get(voice_id)
            .ok_or(AudioServiceError::InvalidCommand("voice does not exist"))
    }

    pub(super) fn voice_frame_count(&self, voice_id: &str) -> Result<u64, AudioServiceError> {
        let state = self
            .timeline
            .voices
            .get(voice_id)
            .ok_or(AudioServiceError::InvalidCommand("voice does not exist"))?;
        let cached = self
            .pcm_cache
            .iter()
            .find(|(key, _)| key.asset == state.asset)
            .map(|(_, value)| value)
            .ok_or(AudioServiceError::InvalidCommand(
                "active PCM asset is missing",
            ))?;
        Ok((cached.samples.len() / usize::from(self.timeline.device_channels)) as u64)
    }

    pub(super) fn evict_for(&mut self, additional_bytes: usize) -> Result<(), AudioServiceError> {
        while self
            .pcm_cache_bytes
            .checked_add(additional_bytes)
            .is_none_or(|total| total > self.config.pcm_cache_bytes)
        {
            let key = self
                .pcm_cache
                .iter()
                .filter(|(_, value)| Arc::strong_count(&value.samples) == 1)
                .min_by_key(|(_, value)| value.last_used)
                .map(|(key, _)| key.clone())
                .ok_or(AudioServiceError::CacheBudget {
                    required_bytes: self.pcm_cache_bytes.saturating_add(additional_bytes),
                    budget_bytes: self.config.pcm_cache_bytes,
                })?;
            let removed = self
                .pcm_cache
                .remove(&key)
                .expect("selected cache key exists");
            self.pcm_cache_bytes -= removed.bytes;
        }
        Ok(())
    }

    pub(super) fn push_event(&mut self, event: AudioServiceEvent) -> Result<(), AudioServiceError> {
        if self.events.len() == self.config.max_events {
            self.poisoned = true;
            return Err(AudioServiceError::InvalidCommand(
                "audio completion event queue overflowed",
            ));
        }
        self.events.push_back(event);
        Ok(())
    }

    pub(super) fn ensure_healthy(&self) -> Result<(), AudioServiceError> {
        if self.poisoned {
            Err(AudioServiceError::Poisoned)
        } else {
            Ok(())
        }
    }

    pub(super) fn poison_kira(&mut self, message: String) -> AudioServiceError {
        self.poisoned = true;
        tracing::error!(
            event = "audio.kira.session.poison",
            "Kira audio service session was poisoned"
        );
        AudioServiceError::Kira(message)
    }
}

pub(super) fn validate_command(command: &AudioServiceCommand) -> Result<(), AudioServiceError> {
    match command {
        AudioServiceCommand::Play {
            voice_id,
            bus,
            asset,
            ..
        } if voice_id.trim().is_empty()
            || bus.trim().is_empty()
            || asset.package_id.trim().is_empty()
            || asset.uri.trim().is_empty()
            || asset.revision.trim().is_empty()
            || asset.byte_len == 0 =>
        {
            Err(AudioServiceError::InvalidCommand(
                "play identity is invalid",
            ))
        }
        AudioServiceCommand::SetBusGain { bus, gain }
            if bus.trim().is_empty() || !gain.is_finite() || *gain < 0.0 =>
        {
            Err(AudioServiceError::InvalidCommand("bus gain is invalid"))
        }
        AudioServiceCommand::FadeBus {
            fade_id,
            bus,
            target_gain,
            duration_frames,
        } if fade_id.trim().is_empty()
            || bus.trim().is_empty()
            || !target_gain.is_finite()
            || *target_gain < 0.0
            || *duration_frames == 0 =>
        {
            Err(AudioServiceError::InvalidCommand("fade is invalid"))
        }
        AudioServiceCommand::Stop { voice_id }
        | AudioServiceCommand::Pause { voice_id }
        | AudioServiceCommand::Resume { voice_id }
        | AudioServiceCommand::Seek { voice_id, .. }
            if voice_id.trim().is_empty() =>
        {
            Err(AudioServiceError::InvalidCommand("voice id is empty"))
        }
        AudioServiceCommand::CancelFade { fade_id } if fade_id.trim().is_empty() => {
            Err(AudioServiceError::InvalidCommand("fade id is empty"))
        }
        _ => Ok(()),
    }
}

pub(super) fn linear_gain_to_decibels(gain: f32) -> Decibels {
    if gain == 0.0 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * gain.log10())
    }
}

pub(super) fn frames_to_duration(
    frames: u64,
    sample_rate: u32,
) -> Result<Duration, AudioServiceError> {
    let nanos =
        u128::from(frames)
            .checked_mul(1_000_000_000)
            .ok_or(AudioServiceError::InvalidCommand(
                "fade duration overflowed",
            ))?
            / u128::from(sample_rate);
    Ok(Duration::from_nanos(u64::try_from(nanos).map_err(
        |_| AudioServiceError::InvalidCommand("fade duration overflowed"),
    )?))
}

pub(super) fn interpolated_fade_gain(state: &AudioBusState) -> f32 {
    let start = state.fade_start_gain.unwrap_or(state.gain);
    let target = state.fade_target_gain.unwrap_or(state.gain);
    let progress = if state.fade_total_frames == 0 {
        1.0
    } else {
        state.fade_rendered_frames as f32 / state.fade_total_frames as f32
    };
    start + (target - start) * progress.clamp(0.0, 1.0)
}
