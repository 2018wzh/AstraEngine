use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use kira::{
    effect::panning_control::{PanningControlBuilder, PanningControlHandle},
    track::{MainTrackBuilder, TrackBuilder, TrackHandle},
    AudioManager, AudioManagerSettings, Capacities, Decibels, Panning, Tween,
};

use crate::{
    AstraChunkBackend, AstraChunkBackendSettings, AstraPcmSoundData, AstraPcmSoundHandle,
    AstraStreamSoundData, AstraStreamSoundHandle, AudioAssetRevision, AudioBusState,
    AudioServiceCommand, AudioServiceEvent, AudioTimelineStateV1, AudioVoiceState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioServiceConfig {
    pub max_voices: usize,
    pub max_buses: usize,
    pub max_events: usize,
    pub pcm_cache_bytes: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioServiceError {
    #[error("audio service configuration is invalid")]
    InvalidConfig,
    #[error("audio service command is invalid: {0}")]
    InvalidCommand(&'static str),
    #[error("audio PCM cache budget exceeded; required={required_bytes}, budget={budget_bytes}")]
    CacheBudget {
        required_bytes: usize,
        budget_bytes: usize,
    },
    #[error("audio service session is poisoned")]
    Poisoned,
    #[error("Kira audio manager failed: {0}")]
    Kira(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DevicePcmKey {
    asset: AudioAssetRevision,
    source_sample_rate: u32,
    source_channels: u16,
    target_sample_rate: u32,
    target_channels: u16,
}

struct CachedPcm {
    samples: Arc<Vec<f32>>,
    bytes: usize,
    last_used: u64,
}

struct ActiveVoice {
    handle: AstraPcmSoundHandle,
    command_sequence: u64,
    completion_target_samples: Option<u64>,
}

struct ActiveStream {
    handle: AstraStreamSoundHandle,
    bus: String,
}

struct AudioBus {
    track: TrackHandle,
    panning: PanningControlHandle,
}

pub struct AudioServiceSession {
    manager: AudioManager<AstraChunkBackend>,
    timeline: AudioTimelineStateV1,
    events: VecDeque<AudioServiceEvent>,
    buses: BTreeMap<String, AudioBus>,
    voices: BTreeMap<String, ActiveVoice>,
    streams: BTreeMap<u64, ActiveStream>,
    retiring_streams: Vec<AstraStreamSoundHandle>,
    pcm_cache: BTreeMap<DevicePcmKey, CachedPcm>,
    pcm_cache_bytes: usize,
    lru_sequence: u64,
    last_consumed_frames: u64,
    config: AudioServiceConfig,
    poisoned: bool,
}

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
        self.ensure_healthy()?;
        if snapshot.schema != crate::timeline::AUDIO_TIMELINE_SCHEMA
            || snapshot.device_sample_rate != self.timeline.device_sample_rate
            || snapshot.device_channels != self.timeline.device_channels
            || snapshot
                .voices
                .values()
                .any(|voice| voice.command_sequence == 0)
        {
            return Err(AudioServiceError::InvalidCommand(
                "audio timeline schema or device format is incompatible",
            ));
        }
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

    fn play(
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

    fn stream(&mut self, stream_id: u64) -> Result<&mut ActiveStream, AudioServiceError> {
        self.ensure_healthy()?;
        self.streams
            .get_mut(&stream_id)
            .ok_or(AudioServiceError::InvalidCommand("stream does not exist"))
    }

    fn ensure_bus(&mut self, bus: &str) -> Result<(), AudioServiceError> {
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

    fn voice(&self, voice_id: &str) -> Result<&ActiveVoice, AudioServiceError> {
        self.voices
            .get(voice_id)
            .ok_or(AudioServiceError::InvalidCommand("voice does not exist"))
    }

    fn voice_frame_count(&self, voice_id: &str) -> Result<u64, AudioServiceError> {
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

    fn evict_for(&mut self, additional_bytes: usize) -> Result<(), AudioServiceError> {
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

    fn push_event(&mut self, event: AudioServiceEvent) -> Result<(), AudioServiceError> {
        if self.events.len() == self.config.max_events {
            self.poisoned = true;
            return Err(AudioServiceError::InvalidCommand(
                "audio completion event queue overflowed",
            ));
        }
        self.events.push_back(event);
        Ok(())
    }

    fn ensure_healthy(&self) -> Result<(), AudioServiceError> {
        if self.poisoned {
            Err(AudioServiceError::Poisoned)
        } else {
            Ok(())
        }
    }

    fn poison_kira(&mut self, message: String) -> AudioServiceError {
        self.poisoned = true;
        tracing::error!(
            event = "audio.kira.session.poison",
            "Kira audio service session was poisoned"
        );
        AudioServiceError::Kira(message)
    }
}

fn validate_command(command: &AudioServiceCommand) -> Result<(), AudioServiceError> {
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

fn linear_gain_to_decibels(gain: f32) -> Decibels {
    if gain == 0.0 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * gain.log10())
    }
}

fn frames_to_duration(frames: u64, sample_rate: u32) -> Result<Duration, AudioServiceError> {
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

fn interpolated_fade_gain(state: &AudioBusState) -> f32 {
    let start = state.fade_start_gain.unwrap_or(state.gain);
    let target = state.fade_target_gain.unwrap_or(state.gain);
    let progress = if state.fade_total_frames == 0 {
        1.0
    } else {
        state.fade_rendered_frames as f32 / state.fade_total_frames as f32
    };
    start + (target - start) * progress.clamp(0.0, 1.0)
}
