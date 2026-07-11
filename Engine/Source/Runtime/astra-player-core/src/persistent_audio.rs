use std::{collections::BTreeMap, sync::Arc};

use crate::PlayerDecodedAudio;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerPersistentVoiceSpec {
    pub id: String,
    pub bus: String,
    pub audio: PlayerDecodedAudio,
    pub looping: bool,
    pub gain: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerAudioCompletion {
    pub voice_id: String,
    pub rendered_frames: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerMixedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
    pub completed: Vec<PlayerAudioCompletion>,
}

#[derive(Debug, Clone)]
pub struct PlayerPersistentAudioMixer {
    sample_rate: u32,
    channels: u16,
    max_voices: usize,
    max_render_frames: usize,
    buses: BTreeMap<String, PlayerAudioBus>,
    voices: BTreeMap<String, PlayerAudioVoice>,
}

#[derive(Debug, Clone)]
pub struct PlayerAudioQueueController {
    target_queued_frames: usize,
    max_render_frames: usize,
    submitted_packets: u64,
    underflow_baseline: Option<u64>,
}

impl PlayerAudioQueueController {
    pub fn new(
        target_queued_frames: usize,
        max_render_frames: usize,
    ) -> Result<Self, PlayerPersistentAudioError> {
        if target_queued_frames == 0
            || max_render_frames == 0
            || max_render_frames > target_queued_frames
        {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_AUDIO_QUEUE_DESCRIPTOR",
                "audio queue controller descriptor is invalid",
            ));
        }
        Ok(Self {
            target_queued_frames,
            max_render_frames,
            submitted_packets: 0,
            underflow_baseline: None,
        })
    }

    pub fn observe(
        &mut self,
        queued_frames: usize,
        underflow_count: u64,
    ) -> Result<usize, PlayerPersistentAudioError> {
        if self.submitted_packets > 0 {
            if let Some(previous) = self.underflow_baseline {
                if underflow_count > previous {
                    return Err(PlayerPersistentAudioError::new(
                        "ASTRA_PLAYER_AUDIO_UNDERFLOW",
                        format!(
                            "callback underflow count increased from {previous} to {underflow_count}"
                        ),
                    ));
                }
            }
            self.underflow_baseline = Some(underflow_count);
        }
        Ok(self
            .target_queued_frames
            .saturating_sub(queued_frames)
            .min(self.max_render_frames))
    }

    pub fn record_submit(&mut self) -> Result<(), PlayerPersistentAudioError> {
        self.submitted_packets = self.submitted_packets.checked_add(1).ok_or_else(|| {
            PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_AUDIO_PACKET_COUNT",
                "submitted packet counter overflowed",
            )
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct PlayerAudioBus {
    gain: f32,
    fade: Option<PlayerAudioFade>,
}

#[derive(Debug, Clone)]
struct PlayerAudioFade {
    start: f32,
    target: f32,
    total_frames: u64,
    rendered_frames: u64,
}

#[derive(Debug, Clone)]
struct PlayerAudioVoice {
    bus: String,
    samples: Arc<[f32]>,
    frame_count: usize,
    frame_cursor: usize,
    rendered_frames: u64,
    looping: bool,
    gain: f32,
}

impl PlayerPersistentAudioMixer {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        max_voices: usize,
        max_render_frames: usize,
    ) -> Result<Self, PlayerPersistentAudioError> {
        if !(8_000..=384_000).contains(&sample_rate)
            || !(1..=8).contains(&channels)
            || max_voices == 0
            || max_render_frames == 0
        {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_DESCRIPTOR",
                "persistent mixer descriptor is invalid",
            ));
        }
        Ok(Self {
            sample_rate,
            channels,
            max_voices,
            max_render_frames,
            buses: BTreeMap::new(),
            voices: BTreeMap::new(),
        })
    }

    pub fn start_voice(
        &mut self,
        spec: PlayerPersistentVoiceSpec,
    ) -> Result<(), PlayerPersistentAudioError> {
        if spec.id.is_empty() || spec.bus.is_empty() || !valid_gain(spec.gain) {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_VOICE_DESCRIPTOR",
                "persistent voice id, bus, or gain is invalid",
            ));
        }
        if spec.audio.sample_rate != self.sample_rate || spec.audio.channels != self.channels {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_FORMAT_MISMATCH",
                "persistent voice format does not match the mixer output",
            ));
        }
        if spec.audio.frame_count() == 0
            || spec.audio.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_VOICE_EMPTY",
                "persistent voice has no valid audio frames",
            ));
        }
        if self.voices.contains_key(&spec.id) {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_VOICE_DUPLICATE",
                "persistent voice id is already active",
            ));
        }
        if self.voices.len() >= self.max_voices {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_VOICE_BUDGET",
                "persistent voice count exceeds the configured budget",
            ));
        }
        self.buses
            .entry(spec.bus.clone())
            .or_insert(PlayerAudioBus {
                gain: 1.0,
                fade: None,
            });
        self.voices.insert(
            spec.id,
            PlayerAudioVoice {
                bus: spec.bus,
                frame_count: spec.audio.frame_count(),
                samples: spec.audio.samples.into(),
                frame_cursor: 0,
                rendered_frames: 0,
                looping: spec.looping,
                gain: spec.gain,
            },
        );
        Ok(())
    }

    pub fn stop_voice(
        &mut self,
        id: &str,
    ) -> Result<PlayerAudioCompletion, PlayerPersistentAudioError> {
        let voice = self.voices.remove(id).ok_or_else(|| {
            PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_VOICE_MISSING",
                "persistent voice is not active",
            )
        })?;
        Ok(PlayerAudioCompletion {
            voice_id: id.to_string(),
            rendered_frames: voice.rendered_frames,
        })
    }

    pub fn set_bus_gain(&mut self, bus: &str, gain: f32) -> Result<(), PlayerPersistentAudioError> {
        if bus.is_empty() || !valid_gain(gain) {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_BUS_GAIN",
                "audio bus id or gain is invalid",
            ));
        }
        self.buses
            .insert(bus.to_string(), PlayerAudioBus { gain, fade: None });
        Ok(())
    }

    pub fn fade_bus(
        &mut self,
        bus: &str,
        target: f32,
        duration_frames: u64,
    ) -> Result<(), PlayerPersistentAudioError> {
        if bus.is_empty() || !valid_gain(target) || duration_frames == 0 {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_BUS_FADE",
                "audio bus fade descriptor is invalid",
            ));
        }
        let state = self.buses.entry(bus.to_string()).or_insert(PlayerAudioBus {
            gain: 1.0,
            fade: None,
        });
        state.fade = Some(PlayerAudioFade {
            start: state.gain,
            target,
            total_frames: duration_frames,
            rendered_frames: 0,
        });
        Ok(())
    }

    pub fn render(
        &mut self,
        frames: usize,
    ) -> Result<PlayerMixedAudio, PlayerPersistentAudioError> {
        if frames == 0 || frames > self.max_render_frames {
            return Err(PlayerPersistentAudioError::new(
                "ASTRA_PLAYER_MIXER_RENDER_BUDGET",
                "requested render size is empty or exceeds the configured budget",
            ));
        }
        let sample_count = frames
            .checked_mul(usize::from(self.channels))
            .ok_or_else(|| {
                PlayerPersistentAudioError::new(
                    "ASTRA_PLAYER_MIXER_RENDER_OVERFLOW",
                    "requested render sample count overflowed",
                )
            })?;
        let mut samples = vec![0.0_f32; sample_count];
        let mut completed = Vec::new();
        let mut completed_ids = Vec::new();
        for frame in 0..frames {
            self.advance_fades();
            for (id, voice) in &mut self.voices {
                let bus_gain = self.buses.get(&voice.bus).map_or(1.0, |bus| bus.gain);
                let source = voice.frame_cursor * usize::from(self.channels);
                let target = frame * usize::from(self.channels);
                for channel in 0..usize::from(self.channels) {
                    samples[target + channel] +=
                        voice.samples[source + channel] * voice.gain * bus_gain;
                }
                voice.frame_cursor += 1;
                voice.rendered_frames += 1;
                if voice.frame_cursor == voice.frame_count {
                    if voice.looping {
                        voice.frame_cursor = 0;
                    } else {
                        completed_ids.push(id.clone());
                    }
                }
            }
            for id in completed_ids.drain(..) {
                if let Some(voice) = self.voices.remove(&id) {
                    completed.push(PlayerAudioCompletion {
                        voice_id: id,
                        rendered_frames: voice.rendered_frames,
                    });
                }
            }
        }
        for sample in &mut samples {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(PlayerMixedAudio {
            sample_rate: self.sample_rate,
            channels: self.channels,
            samples,
            completed,
        })
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.len()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    fn advance_fades(&mut self) {
        for bus in self.buses.values_mut() {
            let Some(fade) = &mut bus.fade else { continue };
            fade.rendered_frames += 1;
            let progress =
                fade.rendered_frames.min(fade.total_frames) as f32 / fade.total_frames as f32;
            bus.gain = fade.start + (fade.target - fade.start) * progress;
            if fade.rendered_frames == fade.total_frames {
                bus.gain = fade.target;
                bus.fade = None;
            }
        }
    }
}

fn valid_gain(gain: f32) -> bool {
    gain.is_finite() && (0.0..=4.0).contains(&gain)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPersistentAudioError {
    code: &'static str,
    message: String,
}

impl PlayerPersistentAudioError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for PlayerPersistentAudioError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PlayerPersistentAudioError {}
