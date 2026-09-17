use super::preferences::bus_index;
use super::*;
use kira::track::{TrackBuilder, TrackHandle};

struct CpuBackend {
    renderer: Option<Renderer>,
}
impl Backend for CpuBackend {
    type Settings = ();
    type Error = std::convert::Infallible;
    fn setup(_: (), _: usize) -> Result<(Self, u32), Self::Error> {
        Ok((Self { renderer: None }, FORMAT.sample_rate))
    }
    fn start(&mut self, r: Renderer) -> Result<(), Self::Error> {
        self.renderer = Some(r);
        Ok(())
    }
}
struct Sound {
    data: StaticSoundData,
    handle: Option<StaticSoundHandle>,
    state: SoundSnapshot,
}
pub(super) struct Mixer {
    manager: AudioManager<CpuBackend>,
    sounds: BTreeMap<u32, Sound>,
    buses: [TrackHandle; 3],
    preferences: AudioPreferences,
}
impl Mixer {
    pub(super) fn new() -> FamilyResult<Self> {
        Self::with_preferences(AudioPreferences::default())
    }
    fn with_preferences(preferences: AudioPreferences) -> FamilyResult<Self> {
        let gains = preferences.gains()?;
        let mut manager = AudioManager::new(AudioManagerSettings {
            backend_settings: (),
            ..Default::default()
        })
        .map_err(|_| error("ASTRA_EMU_MUSICA_MIXER", "mixer could not start"))?;
        let mut add_bus = |gain| {
            manager
                .add_sub_track(TrackBuilder::new().volume(db(gain)))
                .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_BUS", "mixer could not create bus"))
        };
        let buses = [add_bus(gains[0])?, add_bus(gains[1])?, add_bus(gains[2])?];
        Ok(Self {
            manager,
            sounds: BTreeMap::new(),
            buses,
            preferences,
        })
    }
    pub(super) fn set_preferences(&mut self, preferences: AudioPreferences) -> FamilyResult<()> {
        if self.sounds.is_empty() {
            *self = Self::with_preferences(preferences)?;
            return Ok(());
        }
        let gains = preferences.gains()?;
        for (bus, gain) in self.buses.iter_mut().zip(gains) {
            bus.set_volume(db(gain), immediate());
        }
        self.preferences = preferences;
        Ok(())
    }

    pub(super) fn duration_ms(&self, stream: u32) -> FamilyResult<u32> {
        let sound = self.sounds.get(&stream).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_AUDIO_UNKNOWN",
                "duration requested for an unloaded stream",
            )
        })?;
        let milliseconds =
            (sound.data.frames.len() as u64 * 1000).div_ceil(u64::from(sound.data.sample_rate));
        u32::try_from(milliseconds).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_AUDIO_DURATION",
                "audio duration exceeds supported range",
            )
        })
    }
    fn load(
        &mut self,
        id: u32,
        uri: &str,
        archive: &MusicaMountedVfs,
        stop: &AtomicBool,
    ) -> FamilyResult<()> {
        bus_index(uri)?;
        if self.sounds.len() >= 64 && !self.sounds.contains_key(&id) {
            return Err(error(
                "ASTRA_EMU_MUSICA_AUDIO_STREAMS",
                "too many audio streams",
            ));
        }
        let used = self
            .sounds
            .values()
            .map(|s| s.data.frames.len())
            .sum::<usize>();
        let bytes = read_asset(archive, uri, 64 * 1024 * 1024)?;
        let data = astra_emu_sdk::decode_audio(&bytes, MAX_FRAMES.saturating_sub(used), stop)
            .map_err(crate::scene::core_error)?;
        if let Some(mut previous) = self.sounds.remove(&id) {
            if let Some(h) = &mut previous.handle {
                h.stop(immediate());
            }
        }
        self.sounds.insert(
            id,
            Sound {
                data,
                handle: None,
                state: SoundSnapshot {
                    id,
                    uri: uri.into(),
                    position: 0.0,
                    volume: 1.0,
                    pan: 0.0,
                    repeat: false,
                    playing: false,
                    fade: None,
                },
            },
        );
        Ok(())
    }
    fn play(
        &mut self,
        id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade: u32,
    ) -> FamilyResult<()> {
        validate_params(volume, pan)?;
        let sound = self.sounds.get_mut(&id).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_AUDIO_UNKNOWN",
                "audio stream is not loaded",
            )
        })?;
        if let Some(handle) = &mut sound.handle {
            handle.stop(immediate());
        }
        let fade_state = FadeSnapshot::new(Decibels::SILENCE.0, db(volume).0, fade, false);
        let initial_gain = fade_state.as_ref().map_or(db(volume).0, |f| f.from_db);
        let mut data = sound
            .data
            .volume(Decibels(initial_gain))
            .panning(Panning(pan));
        if repeat {
            data = data.loop_region(0.0..);
        }
        data.settings.fade_in_tween = None;
        sound.handle = Some(
            self.buses[bus_index(&sound.state.uri)?]
                .play(data)
                .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_PLAY", "mixer rejected sound"))?,
        );
        if let Some(f) = &fade_state {
            sound
                .handle
                .as_mut()
                .unwrap()
                .set_volume(Decibels(f.to_db), f.remaining_tween());
        }
        sound.state.fade = fade_state;
        sound.state.position = 0.0;
        sound.state.volume = volume;
        sound.state.pan = pan;
        sound.state.repeat = repeat;
        sound.state.playing = true;
        Ok(())
    }
    pub(super) fn apply(
        &mut self,
        command: MusicaAudioCommand,
        archive: &MusicaMountedVfs,
        stop: &AtomicBool,
    ) -> FamilyResult<()> {
        match command {
            MusicaAudioCommand::LoadResource {
                stream_id,
                resource_uri,
                ..
            } => self.load(stream_id, &resource_uri, archive, stop),
            MusicaAudioCommand::Play {
                stream_id,
                volume,
                pan,
                repeat,
                fade_in_ms,
                ..
            } => self.play(stream_id, volume, pan, repeat, fade_in_ms),
            MusicaAudioCommand::Stop {
                stream_id, fade_ms, ..
            } => {
                if let Some(s) = self.sounds.get_mut(&stream_id) {
                    let current = s
                        .state
                        .fade
                        .as_ref()
                        .map_or(db(s.state.volume).0, FadeSnapshot::gain);
                    s.state.fade = if s.state.playing {
                        FadeSnapshot::new(current, Decibels::SILENCE.0, fade_ms, true)
                    } else {
                        None
                    };
                    if let Some(h) = &mut s.handle {
                        if let Some(f) = &s.state.fade {
                            h.set_volume(Decibels(f.to_db), f.remaining_tween());
                        } else {
                            h.stop(immediate());
                            s.state.playing = false;
                        }
                    }
                }
                Ok(())
            }
            MusicaAudioCommand::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
                ..
            } => {
                validate_params(volume, pan)?;
                let s = self.sounds.get_mut(&stream_id).ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_AUDIO_UNKNOWN",
                        "audio stream is not loaded",
                    )
                })?;
                if let Some(h) = &mut s.handle {
                    h.set_volume(db(volume), immediate());
                    h.set_panning(Panning(pan), immediate());
                    if repeat {
                        h.set_loop_region(0.0..)
                    } else {
                        h.set_loop_region(None)
                    }
                }
                s.state.fade = None;
                s.state.volume = volume;
                s.state.pan = pan;
                s.state.repeat = repeat;
                Ok(())
            }
        }
    }
    pub(super) fn render(&mut self, samples: &mut [f32]) {
        let mut offset = 0;
        while offset < samples.len() {
            let frames = self
                .sounds
                .values()
                .filter_map(|s| s.state.fade.as_ref())
                .map(|f| f.frames - f.elapsed)
                .min()
                .unwrap_or(u64::MAX)
                .min(((samples.len() - offset) / 2) as u64) as usize;
            let end = offset + frames * 2;
            let renderer = self.manager.backend_mut().renderer.as_mut().unwrap();
            renderer.on_start_processing();
            renderer.process(&mut samples[offset..end], 2);
            for sound in self.sounds.values_mut() {
                if let Some(fade) = &mut sound.state.fade {
                    fade.elapsed += frames as u64;
                    if fade.elapsed == fade.frames {
                        if fade.stop_after {
                            if let Some(handle) = &mut sound.handle {
                                handle.stop(immediate());
                            }
                            sound.state.playing = false;
                        }
                        sound.state.fade = None;
                    }
                }
            }
            offset = end;
        }
    }
    pub(super) fn snapshot(&mut self) -> Vec<SoundSnapshot> {
        self.manager
            .backend_mut()
            .renderer
            .as_mut()
            .unwrap()
            .on_start_processing();
        self.sounds
            .values()
            .map(|s| {
                let mut result = s.state.clone();
                if let Some(h) = &s.handle {
                    result.position = h.position();
                    result.playing = s.state.playing && h.state() != PlaybackState::Stopped;
                    if !result.playing {
                        result.fade = None;
                    }
                }
                result
            })
            .collect()
    }
    pub(super) fn restore(
        &mut self,
        snapshot: Vec<SoundSnapshot>,
        archive: &MusicaMountedVfs,
        stop: &AtomicBool,
    ) -> FamilyResult<()> {
        if snapshot.len() > 64 {
            return Err(error(
                "ASTRA_EMU_MUSICA_AUDIO_SNAPSHOT",
                "snapshot contains too many sounds",
            ));
        }
        let mut next = Self::with_preferences(self.preferences.clone())?;
        for s in snapshot {
            if next.sounds.contains_key(&s.id) || !s.position.is_finite() || s.position < 0.0 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_AUDIO_SNAPSHOT",
                    "snapshot sound identity or cursor is invalid",
                ));
            }
            validate_params(s.volume, s.pan)?;
            if let Some(fade) = &s.fade {
                fade.validate(s.playing)?;
            }
            next.load(s.id, &s.uri, archive, stop)?;
            let data = &next.sounds[&s.id].data;
            // Use the same frame/rate units as Kira's playback cursor, without
            // rounding through Duration. The endpoint is valid for ended sounds.
            let duration = data.num_frames() as f64 / f64::from(data.sample_rate);
            if s.position > duration {
                return Err(error(
                    "ASTRA_EMU_MUSICA_AUDIO_SNAPSHOT",
                    "snapshot audio cursor exceeds the loaded sound",
                ));
            }
            if s.playing && s.fade.is_none() {
                next.play(s.id, s.volume, s.pan, s.repeat, 0)?;
                next.sounds
                    .get_mut(&s.id)
                    .unwrap()
                    .handle
                    .as_mut()
                    .unwrap()
                    .seek_to(s.position);
            }
            let id = s.id;
            let sound = next.sounds.get_mut(&id).unwrap();
            if let Some(fade) = &s.fade {
                // Initial volume belongs to sound data, avoiding two pending
                // writes to Kira's single-value command slot during restore.
                if let Some(handle) = &mut sound.handle {
                    handle.stop(immediate());
                }
                let mut data = sound
                    .data
                    .volume(Decibels(fade.gain()))
                    .panning(Panning(s.pan));
                if s.repeat {
                    data = data.loop_region(0.0..);
                }
                data.settings.start_position = kira::sound::PlaybackPosition::Seconds(s.position);
                data.settings.fade_in_tween = None;
                let mut handle = next.buses[bus_index(&s.uri)?]
                    .play(data)
                    .map_err(|_| error("ASTRA_EMU_MUSICA_AUDIO_PLAY", "mixer rejected sound"))?;
                handle.set_volume(Decibels(fade.to_db), fade.remaining_tween());
                sound.handle = Some(handle);
            }
            sound.state = s;
        }
        if stop.load(Ordering::Acquire) {
            return Err(error(
                "ASTRA_EMU_AUDIO_CANCELLED",
                "audio restore cancelled",
            ));
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
#[path = "mixer_tests.rs"]
mod tests;
