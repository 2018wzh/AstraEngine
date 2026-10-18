use super::*;
use kira::{
    backend::DefaultBackend,
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        PlaybackState,
    },
    track::{TrackBuilder, TrackHandle},
    AudioManager, AudioManagerSettings, Decibels, Panning, Tween,
};
use std::time::Duration;
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
        Channel::Video => 3,
    }
}
fn fail() -> NativeError {
    error::invalid(
        "ASTRA_EMU_ARTEMIS_AUDIO_MIXER",
        "audio mixer capacity or data limit exceeded",
    )
}
fn decode(
    source: &dyn MediaSource,
    stop: &AtomicBool,
    budget: usize,
) -> NativeResult<Option<StaticSoundData>> {
    if stop.load(Ordering::Acquire) {
        return Ok(None);
    }
    let length = source.len().map_err(|_| fail())?;
    if length == 0 || length > 64 * 1024 * 1024 {
        return Err(fail());
    }
    let mut bytes = vec![0; length as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
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
    match astra_emu_sdk::decode_audio(&bytes, budget, stop) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.code() == "ASTRA_EMU_AUDIO_CANCELLED" => Ok(None),
        Err(error) => Err(crate::error::invalid(
            "ASTRA_EMU_ARTEMIS_AUDIO_DECODE",
            error.to_string(),
        )),
    }
}
pub(super) fn run(
    rx: Receiver<MixerCommand>,
    state: &WorkerState,
    ready: SyncSender<bool>,
) -> NativeResult<()> {
    let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
        .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_DEVICE", e.to_string()))?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_manager(&mut manager, rx, state, ready)
    }));
    // This is the native audio owner, never the device's real-time callback.
    // Join even when decoding, setup or a mixer command failed or panicked.
    let shutdown = manager
        .backend_mut()
        .stop_and_join()
        .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_SHUTDOWN", e.to_string()));
    match result {
        Ok(result) => result.and(shutdown),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
fn run_manager(
    manager: &mut AudioManager<DefaultBackend>,
    rx: Receiver<MixerCommand>,
    state: &WorkerState,
    ready: SyncSender<bool>,
) -> NativeResult<()> {
    let mut add_bus = || {
        manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| fail())
    };
    let mut buses: [TrackHandle; 4] = [add_bus()?, add_bus()?, add_bus()?, add_bus()?];
    let _ = ready.send(true);
    let mut voices: Vec<Voice> = Vec::new();
    let mut suspended = false;
    loop {
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut commands = Vec::new();
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => commands.push(command),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        commands.extend(rx.try_iter());
        for command in commands {
            if state.cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            match command {
                MixerCommand::Shutdown => return Ok(()),
                MixerCommand::Suspend(value) => {
                    suspended = value;
                    for track in &mut buses {
                        if value {
                            track.pause(tween(0));
                        } else {
                            track.resume(tween(0));
                        }
                    }
                }
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
                    let Some(mut data) = decode(
                        sources.base.as_ref(),
                        &state.cancelled,
                        (32 * 1024 * 1024usize).saturating_sub(used),
                    )?
                    else {
                        return Ok(());
                    };
                    let intro_frames = data.frames.len();
                    if let Some(source) = sources.loop_file {
                        let Some(loop_data) = decode(
                            source.as_ref(),
                            &state.cancelled,
                            (32 * 1024 * 1024usize).saturating_sub(used + intro_frames),
                        )?
                        else {
                            return Ok(());
                        };
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
                        notify: channel != Channel::Video,
                        frames,
                    });
                }
                MixerCommand::Stop { id, fade_ms } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|v| v.channel != Channel::Video && v.id == id)
                    {
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
                MixerCommand::StopVideo { id } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|v| v.channel == Channel::Video && v.id == id)
                    {
                        voice.handle.stop(tween(0));
                    }
                }
                MixerCommand::StopVideoAll => {
                    for voice in voices.iter_mut().filter(|v| v.channel == Channel::Video) {
                        voice.handle.stop(tween(0));
                    }
                }
                MixerCommand::Fade { id, gain, time_ms } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|v| v.channel != Channel::Video && v.id == id)
                    {
                        voice.handle.set_volume(db(gain), tween(time_ms));
                    }
                }
                MixerCommand::Pan { id, pan, time_ms } => {
                    for voice in voices
                        .iter_mut()
                        .filter(|v| v.channel != Channel::Video && v.id == id)
                    {
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

#[cfg(test)]
mod tests {
    use super::*;

    struct CancelOnRead<'a> {
        cancelled: &'a AtomicBool,
        length: u64,
        fail_read: bool,
    }

    impl MediaSource for CancelOnRead<'_> {
        fn len(&self) -> Result<u64, String> {
            Ok(self.length)
        }

        fn read_at(&self, _: u64, output: &mut [u8]) -> Result<usize, String> {
            self.cancelled.store(true, Ordering::Release);
            if self.fail_read {
                return Err("source read failed".into());
            }
            output.fill(0);
            Ok(output.len())
        }
    }

    #[test]
    fn decode_cancellation_before_read_between_chunks_and_before_probe() {
        for (initially_cancelled, length) in [(true, 1), (false, 65537), (false, 1)] {
            let cancelled = AtomicBool::new(initially_cancelled);
            let source = CancelOnRead {
                cancelled: &cancelled,
                length,
                fail_read: false,
            };
            assert!(decode(&source, &cancelled, 1024).unwrap().is_none());
        }
    }

    #[test]
    fn source_failure_is_not_hidden_by_concurrent_cancellation() {
        let cancelled = AtomicBool::new(false);
        let source = CancelOnRead {
            cancelled: &cancelled,
            length: 1,
            fail_read: true,
        };
        assert_eq!(
            decode(&source, &cancelled, 1024).unwrap_err().code,
            "ASTRA_EMU_ARTEMIS_AUDIO_MIXER"
        );
    }
}
