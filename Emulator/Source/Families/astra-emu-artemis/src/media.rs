//! Host-event drain and Artemis media command execution.
//!
//! The core routes logs, media commands, and UI commands onto the enabled
//! [`HostEvents`] queue. The session drains it after each tick: media
//! commands drive the software mixer (and, without a bound FFmpeg provider,
//! immediate video completion), logs go to tracing, and UI commands are
//! recorded at debug level.

use std::sync::Arc;

use art3m1s_core::host_events::{HostEvents, EVENT_KIND_LOG, EVENT_KIND_MEDIA, EVENT_KIND_UI};
use art3m1s_core::host_files::HostResources;
use art3m1s_core::media::MediaSource;
use art3m1s_core::runtime::CoreRuntime;
use serde::Deserialize;

use crate::audio::{Channel, MixerCommand, SourcePair};

/// Wire envelope pushed by the core's `push_media`.
#[derive(Deserialize)]
struct CommandEnvelope {
    kind: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Deserialize, Default)]
struct PlayPayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    file: String,
    #[serde(rename = "resolved_file", default)]
    resolved_file: Option<String>,
    #[serde(default)]
    r#loop: bool,
    #[serde(default)]
    gain: Option<i64>,
    #[serde(default)]
    pan: Option<i64>,
    #[serde(rename = "fade_ms", default)]
    fade_ms: Option<u64>,
    #[serde(rename = "loop_file", default)]
    loop_file: Option<String>,
    #[serde(rename = "resolved_loop_file", default)]
    resolved_loop_file: Option<String>,
}

#[derive(Deserialize)]
struct StopPayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "fade_ms", default)]
    fade_ms: Option<u64>,
}

#[derive(Deserialize)]
struct FadePayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    gain: i64,
    #[serde(rename = "time_ms", default)]
    time_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PanPayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    pan: i64,
    #[serde(rename = "time_ms", default)]
    time_ms: Option<u64>,
}

#[derive(Deserialize)]
struct VolumePayload {
    #[serde(default)]
    channel: String,
    #[serde(default)]
    value: f64,
}

/// Per-tick drain result handed back to the session.
pub(crate) struct DrainOutcome {
    pub(crate) commands: Vec<MixerCommand>,
    /// Video ids the host must report finished because it does not decode
    /// video itself.
    pub(crate) finished_videos: Vec<Option<String>>,
}

/// Reads every queued host event. `events` must be the enabled handle of the
/// active session; the drain uses the core's fixed little-endian wire format.
pub(crate) fn drain(events: &HostEvents, resources: &HostResources) -> DrainOutcome {
    let mut outcome = DrainOutcome {
        commands: Vec::new(),
        finished_videos: Vec::new(),
    };
    loop {
        let next = events.next_event_bytes();
        if next == 0 {
            break;
        }
        let mut bytes = vec![0_u8; next];
        let mut count = 0_u32;
        let written = unsafe {
            art3m1s_core::host_events::art3m1s_poll_events_v1(
                events as *const HostEvents as *mut HostEvents,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut count,
            )
        };
        if written == 0 || count == 0 {
            break;
        }
        let mut offset = 0usize;
        for _ in 0..count {
            if offset + 24 > written {
                break;
            }
            let kind = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            let len =
                u32::from_le_bytes(bytes[offset + 16..offset + 20].try_into().unwrap()) as usize;
            let aux = u32::from_le_bytes(bytes[offset + 20..offset + 24].try_into().unwrap());
            offset += 24;
            if offset + len > written {
                break;
            }
            let payload = &bytes[offset..offset + len];
            offset += len;
            match kind {
                EVENT_KIND_LOG => {
                    let level = char::from_u32(aux).unwrap_or('I');
                    let message = String::from_utf8_lossy(payload);
                    // Diagnostic mode mirrors engine logs to stderr: the
                    // adapter itself only emits tracing events, which need a
                    // subscriber the headless driver may not install.
                    if std::env::var("ASTRA_ARTEMIS_TRACE_STATE").is_ok() {
                        eprintln!(
                            "event = astra.emu.artemis.engine_log, level = {level}, {message}"
                        );
                    }
                    match level {
                        'E' => tracing::warn!(
                            event = "astra.emu.artemis.engine_log",
                            level = "error",
                            "{message}"
                        ),
                        'W' => tracing::debug!(
                            event = "astra.emu.artemis.engine_log",
                            level = "warn",
                            "{message}"
                        ),
                        _ => tracing::trace!(
                            event = "astra.emu.artemis.engine_log",
                            level = %level,
                            "{message}"
                        ),
                    }
                }
                EVENT_KIND_MEDIA => {
                    apply_media_event(payload, resources, &mut outcome);
                }
                EVENT_KIND_UI => {
                    tracing::debug!(
                        event = "astra.emu.artemis.ui_command",
                        "{}",
                        String::from_utf8_lossy(payload)
                    );
                }
                _ => {}
            }
        }
    }
    outcome
}

fn apply_media_event(payload: &[u8], resources: &HostResources, outcome: &mut DrainOutcome) {
    let Ok(envelope) = serde_json::from_slice::<CommandEnvelope>(payload) else {
        tracing::debug!(
            event = "astra.emu.artemis.media_command_unparsed",
            "{}",
            String::from_utf8_lossy(payload)
        );
        return;
    };
    match envelope.kind.as_str() {
        "audio_set_volume" => {
            if let Ok(parsed) = serde_json::from_value::<VolumePayload>(envelope.payload) {
                let Some(channel) = channel_of(&parsed.channel) else {
                    return;
                };
                outcome.commands.push(MixerCommand::SetVolume {
                    channel,
                    value: parsed.value.clamp(0.0, 1.0) as f32,
                });
            }
        }
        "audio_bgm_play" => {
            if let Ok(parsed) = serde_json::from_value::<PlayPayload>(envelope.payload) {
                if let Some(command) = play_command(None, Channel::Bgm, &parsed, resources) {
                    outcome.commands.push(command);
                }
            }
        }
        "audio_bgm_crossfade" => {
            // Fade out the old BGM and start the new one; the mixer replaces
            // the single BGM voice on play, so a stop with the crossfade
            // duration precedes the play.
            if let Ok(parsed) = serde_json::from_value::<PlayPayload>(envelope.payload) {
                let fade_ms = parsed.fade_ms.unwrap_or(0);
                outcome
                    .commands
                    .push(MixerCommand::Stop { id: None, fade_ms });
                if let Some(command) = play_command(None, Channel::Bgm, &parsed, resources) {
                    outcome.commands.push(command);
                }
            }
        }
        "audio_bgm_stop" => {
            if let Ok(parsed) = serde_json::from_value::<StopPayload>(envelope.payload) {
                outcome.commands.push(MixerCommand::Stop {
                    id: None,
                    fade_ms: parsed.fade_ms.unwrap_or(0),
                });
            }
        }
        "audio_bgm_fade" => {
            if let Ok(parsed) = serde_json::from_value::<FadePayload>(envelope.payload) {
                outcome.commands.push(MixerCommand::Fade {
                    id: None,
                    gain: compat_gain(Some(parsed.gain)),
                    time_ms: parsed.time_ms.unwrap_or(0),
                });
            }
        }
        "audio_bgm_pan" => {
            if let Ok(parsed) = serde_json::from_value::<PanPayload>(envelope.payload) {
                outcome.commands.push(MixerCommand::Pan {
                    id: None,
                    pan: compat_pan(Some(parsed.pan)),
                    time_ms: parsed.time_ms.unwrap_or(0),
                });
            }
        }
        "audio_se_play" => {
            if let Ok(parsed) = serde_json::from_value::<PlayPayload>(envelope.payload) {
                if let Some(command) =
                    play_command(parsed.id.clone(), Channel::Se, &parsed, resources)
                {
                    outcome.commands.push(command);
                }
            }
        }
        "audio_se_stop" => {
            if let Ok(parsed) = serde_json::from_value::<StopPayload>(envelope.payload) {
                outcome.commands.push(MixerCommand::Stop {
                    id: parsed.id,
                    fade_ms: parsed.fade_ms.unwrap_or(0),
                });
            }
        }
        "audio_se_fade" => {
            if let Ok(parsed) = serde_json::from_value::<FadePayload>(envelope.payload) {
                outcome.commands.push(MixerCommand::Fade {
                    id: parsed.id,
                    gain: compat_gain(Some(parsed.gain)),
                    time_ms: parsed.time_ms.unwrap_or(0),
                });
            }
        }
        "audio_se_pan" => {
            if let Ok(parsed) = serde_json::from_value::<PanPayload>(envelope.payload) {
                outcome.commands.push(MixerCommand::Pan {
                    id: parsed.id,
                    pan: compat_pan(Some(parsed.pan)),
                    time_ms: parsed.time_ms.unwrap_or(0),
                });
            }
        }
        "audio_voice_play" => {
            if let Ok(parsed) = serde_json::from_value::<PlayPayload>(envelope.payload) {
                if let Some(command) =
                    play_command(parsed.id.clone(), Channel::Voice, &parsed, resources)
                {
                    outcome.commands.push(command);
                }
            }
        }
        "audio_stop_all" => {
            outcome.commands.push(MixerCommand::StopAll { fade_ms: 0 });
        }
        "video_play" => {
            let id: Option<String> = serde_json::from_value::<PlayPayload>(envelope.payload)
                .ok()
                .and_then(|parsed| parsed.id);
            outcome.finished_videos.push(id);
        }
        "video_stop_all" => {}
        other => {
            tracing::debug!(
                event = "astra.emu.artemis.media_command_ignored",
                kind = other
            );
        }
    }
}

fn play_command(
    id: Option<String>,
    channel: Channel,
    parsed: &PlayPayload,
    resources: &HostResources,
) -> Option<MixerCommand> {
    let sources = resolve_sources(parsed, resources)?;
    Some(MixerCommand::Play {
        id,
        channel,
        sources,
        loop_play: parsed.r#loop,
        gain: compat_gain(parsed.gain),
        pan: compat_pan(parsed.pan),
        fade_ms: parsed.fade_ms.unwrap_or(0),
    })
}

/// Opens the media sources through the mounted host resources. `None` keeps
/// the session alive; the missing file is visible in tracing.
fn resolve_sources(parsed: &PlayPayload, resources: &HostResources) -> Option<SourcePair> {
    let base = open_source(
        resources,
        parsed.resolved_file.as_deref().unwrap_or(&parsed.file),
    )?;
    let loop_file = match parsed
        .resolved_loop_file
        .as_deref()
        .or(parsed.loop_file.as_deref())
    {
        Some(path) => match open_source(resources, path) {
            Some(source) => Some(source),
            None => {
                tracing::warn!(
                    event = "astra.emu.artemis.audio.loop_file_missing",
                    "loop file is not readable; looping the base source"
                );
                None
            }
        },
        None => None,
    };
    Some(SourcePair { base, loop_file })
}

fn open_source(resources: &HostResources, path: &str) -> Option<Arc<dyn MediaSource>> {
    match resources.open_media_source(path) {
        Ok(source) => Some(source),
        Err(open_error) => {
            tracing::warn!(
                event = "astra.emu.artemis.audio.source_missing",
                detail = %open_error
            );
            None
        }
    }
}

fn channel_of(name: &str) -> Option<Channel> {
    match name {
        "bgm" => Some(Channel::Bgm),
        "se" => Some(Channel::Se),
        "voice" => Some(Channel::Voice),
        _ => None,
    }
}

/// Artemis gain is a raw 0..1000 script value; values above 1 are raw, values
/// at or below 1 are already linear. Null keeps the previous gain (1.0 on a
/// fresh play).
fn compat_gain(raw: Option<i64>) -> f32 {
    match raw {
        None => 1.0,
        Some(value) if value > 1 => (value as f32 / 1000.0).clamp(0.0, 1.0),
        Some(value) => (value as f32).clamp(0.0, 1.0),
    }
}

/// Artemis pan is a raw -1000..1000 script value; magnitudes above 1 are raw.
fn compat_pan(raw: Option<i64>) -> f32 {
    let value = raw.unwrap_or(0) as f32;
    let scaled = if value.abs() > 1.0 {
        value / 1000.0
    } else {
        value
    };
    scaled.clamp(-1.0, 1.0)
}

/// Reports video completion when the host does not decode video itself.
pub(crate) fn notify_videos_finished(rt: &mut CoreRuntime, ids: &[Option<String>]) {
    for id in ids {
        rt.notify_video_finished(id.as_deref());
    }
}
