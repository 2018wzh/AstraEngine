use super::*;

impl AudioServiceSession {
    pub fn new(
        config: AudioServiceConfig,
        backend: AstraChunkBackendSettings,
    ) -> Result<Self, AudioServiceError> {
        if config.max_voices == 0
            || config.max_buses == 0
            || config.max_events == 0
            || config.pcm_cache_bytes == 0
        {
            return Err(AudioServiceError::InvalidConfig);
        }
        let sample_rate = backend.sample_rate;
        let channels = backend.channels;
        let deterministic_clock = backend.deterministic_fixed_tick_hz.is_some();
        if !matches!(channels, 1 | 2) {
            return Err(AudioServiceError::InvalidConfig);
        }
        let manager = AudioManager::<AstraChunkBackend>::new(AudioManagerSettings {
            capacities: Capacities {
                sub_track_capacity: config.max_buses,
                send_track_capacity: 0,
                clock_capacity: 0,
                modulator_capacity: 0,
                listener_capacity: 0,
            },
            main_track_builder: MainTrackBuilder::default(),
            internal_buffer_size: 128,
            backend_settings: backend,
        })
        .map_err(|error| AudioServiceError::Kira(error.to_string()))?;
        tracing::info!(
            event = "audio.kira.session.start",
            sample_rate,
            channels,
            deterministic_clock,
            "started Kira audio service session"
        );
        Ok(Self {
            manager,
            timeline: AudioTimelineStateV1::new(sample_rate, channels),
            events: VecDeque::with_capacity(config.max_events),
            buses: BTreeMap::new(),
            voices: BTreeMap::new(),
            streams: BTreeMap::new(),
            retiring_streams: Vec::with_capacity(config.max_voices),
            pcm_cache: BTreeMap::new(),
            pcm_cache_bytes: 0,
            lru_sequence: 0,
            last_consumed_frames: 0,
            config,
            poisoned: false,
        })
    }

    /// Moves one decoder allocation into the device-format cache. The allocation pointer remains
    /// unchanged; mismatched formats must be converted by the decoder/resampler owner first.
    pub fn prepare_pcm(
        &mut self,
        asset: AudioAssetRevision,
        source_sample_rate: u32,
        source_channels: u16,
        samples: Vec<f32>,
    ) -> Result<*const f32, AudioServiceError> {
        self.prepare_pcm_shared(
            asset,
            source_sample_rate,
            source_channels,
            Arc::new(samples),
        )
    }

    /// Registers an already shared decoder allocation without rebuilding its sample storage.
    pub fn prepare_pcm_shared(
        &mut self,
        asset: AudioAssetRevision,
        source_sample_rate: u32,
        source_channels: u16,
        samples: Arc<Vec<f32>>,
    ) -> Result<*const f32, AudioServiceError> {
        self.ensure_healthy()?;
        if asset.package_id.trim().is_empty()
            || asset.uri.trim().is_empty()
            || asset.revision.trim().is_empty()
            || asset.byte_len == 0
            || source_sample_rate != self.timeline.device_sample_rate
            || source_channels != self.timeline.device_channels
            || samples.is_empty()
            || !samples.len().is_multiple_of(usize::from(source_channels))
            || samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(AudioServiceError::InvalidCommand(
                "prepared PCM must match the selected device format",
            ));
        }
        let bytes = samples
            .len()
            .checked_mul(size_of::<f32>())
            .ok_or(AudioServiceError::InvalidCommand("PCM size overflowed"))?;
        if bytes > self.config.pcm_cache_bytes {
            return Err(AudioServiceError::CacheBudget {
                required_bytes: bytes,
                budget_bytes: self.config.pcm_cache_bytes,
            });
        }
        let key = DevicePcmKey {
            asset,
            source_sample_rate,
            source_channels,
            target_sample_rate: self.timeline.device_sample_rate,
            target_channels: self.timeline.device_channels,
        };
        if self.pcm_cache.contains_key(&key) {
            return Err(AudioServiceError::InvalidCommand(
                "PCM revision is already prepared",
            ));
        }
        self.evict_for(bytes)?;
        self.lru_sequence = self
            .lru_sequence
            .checked_add(1)
            .ok_or(AudioServiceError::InvalidCommand("LRU sequence overflowed"))?;
        let pointer = samples.as_ptr();
        self.pcm_cache.insert(
            key,
            CachedPcm {
                samples,
                bytes,
                last_used: self.lru_sequence,
            },
        );
        self.pcm_cache_bytes += bytes;
        Ok(pointer)
    }

    /// Clones only the allocation owners needed to rebuild this session after an endpoint loss.
    /// The PCM allocations themselves are not copied.
    #[must_use]
    pub fn is_pcm_prepared(&self, asset: &AudioAssetRevision) -> bool {
        self.pcm_cache.keys().any(|key| &key.asset == asset)
    }

    pub fn prepared_pcm_assets(&self) -> Vec<(AudioAssetRevision, u32, u16, Arc<Vec<f32>>)> {
        self.pcm_cache
            .iter()
            .map(|(key, cached)| {
                (
                    key.asset.clone(),
                    key.source_sample_rate,
                    key.source_channels,
                    Arc::clone(&cached.samples),
                )
            })
            .collect()
    }

    pub fn apply(&mut self, command: AudioServiceCommand) -> Result<u64, AudioServiceError> {
        self.ensure_healthy()?;
        validate_command(&command)?;
        let sequence = self.timeline.command_sequence.checked_add(1).ok_or(
            AudioServiceError::InvalidCommand("command sequence overflowed"),
        )?;

        match command {
            AudioServiceCommand::Play {
                voice_id,
                bus,
                asset,
                start_frame,
                looping,
            } => self.play(sequence, voice_id, bus, asset, start_frame, looping)?,
            AudioServiceCommand::Stop { voice_id } => {
                self.voice(&voice_id)?.handle.stop();
                self.timeline.voices.remove(&voice_id);
                self.voices.remove(&voice_id);
            }
            AudioServiceCommand::Pause { voice_id } => {
                self.voice(&voice_id)?.handle.pause();
                self.timeline
                    .voices
                    .get_mut(&voice_id)
                    .expect("validated live voice must have timeline state")
                    .paused = true;
            }
            AudioServiceCommand::Resume { voice_id } => {
                self.voice(&voice_id)?.handle.resume();
                self.timeline
                    .voices
                    .get_mut(&voice_id)
                    .expect("validated live voice must have timeline state")
                    .paused = false;
            }
            AudioServiceCommand::Seek { voice_id, frame } => {
                let total_frames = self.voice_frame_count(&voice_id)?;
                if frame >= total_frames {
                    return Err(AudioServiceError::InvalidCommand(
                        "seek frame is outside the prepared asset",
                    ));
                }
                self.voice(&voice_id)?.handle.seek(frame);
                self.timeline
                    .voices
                    .get_mut(&voice_id)
                    .expect("validated live voice must have timeline state")
                    .cursor_frames = frame;
            }
            AudioServiceCommand::SetBusGain { bus, gain } => {
                self.ensure_bus(&bus)?;
                self.buses
                    .get_mut(&bus)
                    .expect("created bus must exist")
                    .track
                    .set_volume(linear_gain_to_decibels(gain), Tween::default());
                self.timeline.buses.insert(
                    bus,
                    AudioBusState {
                        gain,
                        fade_id: None,
                        fade_sequence: 0,
                        fade_start_gain: None,
                        fade_target_gain: None,
                        fade_total_frames: 0,
                        fade_rendered_frames: 0,
                    },
                );
            }
            AudioServiceCommand::FadeBus {
                fade_id,
                bus,
                target_gain,
                duration_frames,
            } => {
                self.ensure_bus(&bus)?;
                if self
                    .timeline
                    .buses
                    .values()
                    .any(|state| state.fade_id.as_deref() == Some(fade_id.as_str()))
                {
                    return Err(AudioServiceError::InvalidCommand("fade id is duplicate"));
                }
                let duration =
                    frames_to_duration(duration_frames, self.timeline.device_sample_rate)?;
                self.buses
                    .get_mut(&bus)
                    .expect("created bus must exist")
                    .track
                    .set_volume(
                        linear_gain_to_decibels(target_gain),
                        Tween {
                            duration,
                            ..Tween::default()
                        },
                    );
                let state = self.timeline.buses.get_mut(&bus).expect("bus state exists");
                state.fade_id = Some(fade_id);
                state.fade_sequence = sequence;
                state.fade_start_gain = Some(state.gain);
                state.fade_target_gain = Some(target_gain);
                state.fade_total_frames = duration_frames;
                state.fade_rendered_frames = 0;
            }
            AudioServiceCommand::CancelFade { fade_id } => {
                let (bus, state) = self
                    .timeline
                    .buses
                    .iter_mut()
                    .find(|(_, state)| state.fade_id.as_deref() == Some(fade_id.as_str()))
                    .ok_or(AudioServiceError::InvalidCommand("fade does not exist"))?;
                let gain = interpolated_fade_gain(state);
                self.buses
                    .get_mut(bus)
                    .expect("timeline bus must have Kira track")
                    .track
                    .set_volume(linear_gain_to_decibels(gain), Tween::default());
                state.gain = gain;
                state.fade_id = None;
                state.fade_sequence = 0;
                state.fade_start_gain = None;
                state.fade_target_gain = None;
                state.fade_total_frames = 0;
                state.fade_rendered_frames = 0;
            }
        }
        self.timeline.command_sequence = sequence;
        Ok(sequence)
    }

    /// Called exactly once at a fixed-tick boundary. Completion is delayed until the endpoint
    /// reports that all samples rendered before completion have actually been consumed.
    pub fn poll_fixed_tick(&mut self) -> Result<(), AudioServiceError> {
        self.manager
            .backend_mut()
            .advance_fixed_tick()
            .map_err(|error| {
                self.poisoned = true;
                AudioServiceError::Kira(error.to_string())
            })?;
        self.poll_backend()?;
        self.retiring_streams
            .retain(|handle| !handle.is_completed());
        let telemetry = self.manager.backend_mut().telemetry();
        self.timeline.consumed_frames =
            telemetry.consumed_samples / u64::from(self.timeline.device_channels);
        let consumed_delta = self
            .timeline
            .consumed_frames
            .checked_sub(self.last_consumed_frames)
            .ok_or(AudioServiceError::InvalidCommand(
                "endpoint consumed sample counter moved backwards",
            ))?;
        self.last_consumed_frames = self.timeline.consumed_frames;

        let mut completed = Vec::new();
        for (voice_id, voice) in &mut self.voices {
            if let Some(state) = self.timeline.voices.get_mut(voice_id) {
                state.cursor_frames = voice.handle.cursor_frames();
            }
            if voice.handle.is_completed() && voice.completion_target_samples.is_none() {
                voice.completion_target_samples = Some(telemetry.submitted_samples);
            }
            if voice
                .completion_target_samples
                .is_some_and(|target| telemetry.consumed_samples >= target)
            {
                completed.push((voice_id.clone(), voice.command_sequence));
            }
        }
        for (voice_id, sequence) in completed {
            self.voices.remove(&voice_id);
            self.timeline.voices.remove(&voice_id);
            self.push_event(AudioServiceEvent::VoiceCompleted { sequence, voice_id })?;
        }

        let mut completed_fades = Vec::new();
        for state in self.timeline.buses.values_mut() {
            let Some(fade_id) = state.fade_id.clone() else {
                continue;
            };
            state.fade_rendered_frames = state.fade_rendered_frames.saturating_add(consumed_delta);
            if state.fade_rendered_frames >= state.fade_total_frames {
                state.gain = state.fade_target_gain.expect("active fade has target gain");
                let sequence = state.fade_sequence;
                state.fade_id = None;
                state.fade_sequence = 0;
                state.fade_start_gain = None;
                state.fade_target_gain = None;
                state.fade_total_frames = 0;
                state.fade_rendered_frames = 0;
                completed_fades.push((fade_id, sequence));
            }
        }
        for (fade_id, sequence) in completed_fades {
            self.push_event(AudioServiceEvent::FadeCompleted { sequence, fade_id })?;
        }
        Ok(())
    }

    #[must_use]
    pub fn timeline(&self) -> &AudioTimelineStateV1 {
        &self.timeline
    }

    #[must_use]
    pub fn take_events(&mut self) -> Vec<AudioServiceEvent> {
        self.events.drain(..).collect()
    }

    #[must_use]
    pub fn active_voice_count(&self) -> usize {
        self.voices.len()
    }

    #[must_use]
    pub fn voice_bus(&self, voice_id: &str) -> Option<&str> {
        self.timeline
            .voices
            .get(voice_id)
            .map(|voice| voice.bus.as_str())
    }

    #[must_use]
    pub fn active_bus_fade_id(&self, bus: &str) -> Option<&str> {
        self.timeline
            .buses
            .get(bus)
            .and_then(|state| state.fade_id.as_deref())
    }

    pub fn create_stream(
        &mut self,
        stream_id: u64,
        bus: &str,
        chunk_frames: usize,
        chunk_capacity: usize,
    ) -> Result<(), AudioServiceError> {
        self.ensure_healthy()?;
        if stream_id == 0 || self.streams.contains_key(&stream_id) {
            return Err(AudioServiceError::InvalidCommand(
                "stream id is zero or duplicate",
            ));
        }
        self.ensure_bus(bus)?;
        let sound =
            AstraStreamSoundData::new(self.timeline.device_channels, chunk_frames, chunk_capacity)
                .map_err(AudioServiceError::InvalidCommand)?;
        let handle = self
            .buses
            .get_mut(bus)
            .expect("created stream bus exists")
            .track
            .play(sound)
            .map_err(|error| self.poison_kira(error.to_string()))?;
        self.streams.insert(
            stream_id,
            ActiveStream {
                handle,
                bus: bus.to_owned(),
            },
        );
        Ok(())
    }

    pub fn set_stream_mix(
        &mut self,
        stream_id: u64,
        gain: f32,
        pan: f32,
        duration_frames: u64,
    ) -> Result<(), AudioServiceError> {
        self.ensure_healthy()?;
        if !gain.is_finite() || gain < 0.0 || !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
            return Err(AudioServiceError::InvalidCommand(
                "stream gain or panning is invalid",
            ));
        }
        let bus = self
            .streams
            .get(&stream_id)
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))?
            .bus
            .clone();
        let tween = Tween {
            duration: frames_to_duration(duration_frames, self.timeline.device_sample_rate)?,
            ..Tween::default()
        };
        let bus = self
            .buses
            .get_mut(&bus)
            .expect("active stream bus must exist");
        bus.track.set_volume(linear_gain_to_decibels(gain), tween);
        bus.panning.set_panning(Panning(pan), Tween::default());
        Ok(())
    }

    pub fn stream_has_capacity(&self, stream_id: u64) -> Result<bool, AudioServiceError> {
        self.streams
            .get(&stream_id)
            .map(|stream| stream.handle.has_capacity())
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))
    }

    pub fn stream_has_recyclable_capacity(
        &self,
        stream_id: u64,
    ) -> Result<bool, AudioServiceError> {
        self.streams
            .get(&stream_id)
            .map(|stream| stream.handle.has_recyclable_capacity())
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))
    }
}
