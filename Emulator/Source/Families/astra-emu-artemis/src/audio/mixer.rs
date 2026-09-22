use super::*;
use kira::{
    backend::{Backend, Renderer},
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        PlaybackState,
    },
    track::{TrackBuilder, TrackHandle},
    AudioManager, AudioManagerSettings, Decibels, Panning, Tween,
};
use std::time::Duration;
struct PcmBackend {
    renderer: Option<Renderer>,
}
impl Backend for PcmBackend {
    type Settings = ();
    type Error = std::convert::Infallible;
    fn setup(_: (), _: usize) -> Result<(Self, u32), Self::Error> {
        Ok((Self { renderer: None }, SAMPLE_RATE))
    }
    fn start(&mut self, renderer: Renderer) -> Result<(), Self::Error> {
        self.renderer = Some(renderer);
        Ok(())
    }
}
struct Voice {
    id: Option<String>,
    channel: Channel,
    handle: StaticSoundHandle,
    notify: bool,
    frames: usize,
}
fn tween(ms: u64) -> Tween {
    Tween {
        duration: Duration::from_millis(ms),
        ..Default::default()
    }
}
fn db(value: f32) -> Decibels {
    Decibels(if value <= 0.0 {
        Decibels::SILENCE.0
    } else {
        20.0 * value.log10()
    })
}
fn bus(channel: Channel) -> usize {
    match channel {
        Channel::Bgm => 0,
        Channel::Se => 1,
        Channel::Voice => 2,
    }
}
fn fail() -> FamilyError {
    error::invalid(
        "ASTRA_EMU_ARTEMIS_AUDIO_MIXER",
        "audio mixer capacity or data limit exceeded",
    )
}
fn decode(
    source: &dyn MediaSource,
    stop: &AtomicBool,
    budget: usize,
) -> FamilyResult<StaticSoundData> {
    let length = source.len().map_err(|_| fail())?;
    if length == 0 || length > 64 * 1024 * 1024 {
        return Err(fail());
    }
    let mut bytes = vec![0; length as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        if stop.load(Ordering::Acquire) {
            return Err(fail());
        }
        let end = (offset + 65536).min(bytes.len());
        let read = source
            .read_at(offset as u64, &mut bytes[offset..end])
            .map_err(|_| fail())?;
        if read == 0 || read > end - offset {
            return Err(fail());
        }
        offset += read;
    }
    astra_emu_sdk::decode_audio(&bytes, budget, stop)
        .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_DECODE", e.to_string()))
}
pub(super) fn run(
    rx: Receiver<MixerCommand>,
    state: &WorkerState,
    sink: &AudioSinkBox,
) -> FamilyResult<()> {
    let mut manager = AudioManager::<PcmBackend>::new(AudioManagerSettings {
        backend_settings: (),
        ..Default::default()
    })
    .map_err(|_| fail())?;
    let mut add_bus = || {
        manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| fail())
    };
    let mut buses: [TrackHandle; 3] = [add_bus()?, add_bus()?, add_bus()?];
    let mut voices: Vec<Voice> = Vec::new();
    let mut suspended = false;
    loop {
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut commands = Vec::new();
        if suspended || voices.is_empty() {
            match rx.recv() {
                Ok(command) => commands.push(command),
                Err(_) => return Ok(()),
            }
        }
        commands.extend(rx.try_iter());
        for command in commands {
            match command {
                MixerCommand::Shutdown => return Ok(()),
                MixerCommand::Suspend(value) => suspended = value,
                MixerCommand::SetVolume { channel, value } => {
                    buses[bus(channel)].set_volume(db(value), tween(0))
                }
                MixerCommand::Play {
                    id,
                    channel,
                    sources,
                    loop_play,
                    gain,
                    pan,
                    fade_ms,
                } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|v| v.id == id && v.channel == channel)
                    {
                        voice.handle.stop(tween(0));
                        voice.notify = false;
                    }
                    let used = voices.iter().map(|v| v.frames).sum::<usize>();
                    if voices.len() >= 64 {
                        return Err(fail());
                    }
                    let mut data = decode(
                        sources.base.as_ref(),
                        &state.cancelled,
                        (32 * 1024 * 1024usize).saturating_sub(used),
                    )?;
                    let intro_frames = data.frames.len();
                    if let Some(source) = sources.loop_file {
                        let loop_data = decode(
                            source.as_ref(),
                            &state.cancelled,
                            (32 * 1024 * 1024usize).saturating_sub(used + intro_frames),
                        )?;
                        if loop_data.sample_rate != data.sample_rate {
                            return Err(fail());
                        }
                        let start = intro_frames as f64 / f64::from(data.sample_rate);
                        let mut frames = data.frames.to_vec();
                        frames.extend_from_slice(&loop_data.frames);
                        data.frames = frames.into();
                        if loop_play {
                            data = data.loop_region(start..);
                        }
                    } else if loop_play {
                        data = data.loop_region(..);
                    }
                    let frames = data.frames.len();
                    data = data.volume(db(gain)).panning(Panning((pan + 1.0) / 2.0));
                    if fade_ms > 0 {
                        data = data.fade_in_tween(tween(fade_ms));
                    }
                    let handle = buses[bus(channel)].play(data).map_err(|_| fail())?;
                    voices.push(Voice {
                        id,
                        channel,
                        handle,
                        notify: true,
                        frames,
                    });
                }
                MixerCommand::Stop { id, fade_ms } => {
                    for voice in voices.iter_mut().filter(|v| v.id == id) {
                        voice.handle.stop(tween(fade_ms));
                        voice.notify = false;
                    }
                }
                MixerCommand::StopAll { fade_ms } => {
                    for voice in &mut voices {
                        voice.handle.stop(tween(fade_ms));
                        voice.notify = false;
                    }
                }
                MixerCommand::Fade { id, gain, time_ms } => {
                    for voice in voices.iter_mut().filter(|v| v.id == id) {
                        voice.handle.set_volume(db(gain), tween(time_ms));
                    }
                }
                MixerCommand::Pan { id, pan, time_ms } => {
                    for voice in voices.iter_mut().filter(|v| v.id == id) {
                        voice
                            .handle
                            .set_panning(Panning((pan + 1.0) / 2.0), tween(time_ms));
                    }
                }
            }
        }
        if suspended {
            continue;
        }
        let mut samples = vec![0.0; CHUNK_FRAMES * 2];
        let renderer = manager.backend_mut().renderer.as_mut().ok_or_else(fail)?;
        renderer.on_start_processing();
        renderer.process(&mut samples, 2);
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        match sink.write(PcmChunk::F32(samples.into())).into_result()? {
            AudioWriteStatus::Accepted => {}
            AudioWriteStatus::Cancelled | AudioWriteStatus::Closed => {
                if state.cancelled.load(Ordering::Acquire) {
                    return Ok(());
                }
                return Err(error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_CLOSED",
                    "host audio queue closed",
                ));
            }
        }
        voices.retain(|voice| {
            if voice.handle.state() != PlaybackState::Stopped {
                return true;
            }
            if voice.notify {
                let _ = state.tx.try_send(SoundFinished {
                    id: voice.id.clone(),
                });
            }
            false
        });
    }
}
