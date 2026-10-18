//! Host-event drain and Artemis media command execution.
//!
//! The core routes logs, media commands, and UI commands onto the enabled
//! [`HostEvents`] queue. The session drains it after each tick: media
//! commands drive audio; runtime video uses the core's FFmpeg/GPU path.
//! Unsupported requests fail explicitly.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::Mutex;

use crate::error;
use crate::error::{NativeError, NativeResult};
use art3m1s_core::host_events::{HostEvents, EVENT_KIND_LOG, EVENT_KIND_MEDIA, EVENT_KIND_UI};
use art3m1s_core::host_files::HostResources;
use art3m1s_core::media::MediaSource;
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

#[derive(Deserialize)]
struct VideoAudioPayload {
    id: Option<String>,
    path: String,
    #[serde(default)]
    r#loop: bool,
}

/// Adapt the native temporary WAV to its media-source contract. Keeping the
/// open handle permits core cleanup while the mixer is still reading it.
struct VideoAudioSource {
    file: Mutex<File>,
    length: u64,
}

impl VideoAudioSource {
    fn open(path: &str) -> NativeResult<Self> {
        let source_error = |cause: std::io::Error| {
            error::invalid("ASTRA_EMU_ARTEMIS_VIDEO_AUDIO_SOURCE", cause.to_string())
        };
        let file = File::open(path).map_err(source_error)?;
        let length = file.metadata().map_err(source_error)?.len();
        std::fs::remove_file(path).map_err(source_error)?;
        Ok(Self {
            file: Mutex::new(file),
            length,
        })
    }
}

impl MediaSource for VideoAudioSource {
    fn len(&self) -> Result<u64, String> {
        Ok(self.length)
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<usize, String> {
        let mut file = self.file.lock().map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        file.read(output).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub(crate) enum WindowCommand {
    Title(String),
    Pointer {
        x: Option<i32>,
        y: Option<i32>,
        visible: Option<bool>,
        autohide_ms: Option<u64>,
    },
}
#[derive(Default)]
pub(crate) struct WindowRequests(std::collections::VecDeque<WindowCommand>);
impl WindowRequests {
    fn push(&mut self, command: WindowCommand) -> NativeResult<()> {
        if self.0.len() == 128 {
            return Err(invalid());
        }
        self.0.push_back(command);
        Ok(())
    }
    pub(crate) fn take(&mut self) -> Option<WindowCommand> {
        self.0.pop_front()
    }
}

pub(crate) fn drain(
    events: &HostEvents,
    resources: &HostResources,
    window: &mut WindowRequests,
) -> NativeResult<Vec<MixerCommand>> {
    if events.dropped_events() != 0 {
        return Err(invalid());
    }
    let mut commands = Vec::new();
    loop {
        let next = events.next_event_bytes();
        if next == 0 {
            break;
        }
        if next > 4 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut bytes = vec![0; next];
        let mut count = 0;
        let written = unsafe {
            art3m1s_core::host_events::art3m1s_poll_events_v1(
                events as *const HostEvents as *mut HostEvents,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut count,
            )
        };
        if written == 0 || written > bytes.len() || count == 0 {
            return Err(invalid());
        }
        let mut offset = 0;
        for _ in 0..count {
            let header = bytes.get(offset..offset + 24).ok_or_else(invalid)?;
            let kind = u32::from_le_bytes(header[4..8].try_into().unwrap());
            let len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
            let aux = u32::from_le_bytes(header[20..24].try_into().unwrap());
            offset += 24;
            let payload = bytes
                .get(offset..offset + len)
                .filter(|_| offset + len <= written)
                .ok_or_else(invalid)?;
            offset += len;
            match kind {
                EVENT_KIND_LOG => {
                    let message = String::from_utf8_lossy(payload);
                    match char::from_u32(aux) {
                        Some('E') => {
                            return Err(error::invalid(
                                "ASTRA_EMU_ARTEMIS_ENGINE",
                                message.to_string(),
                            ))
                        }
                        Some('W') => {
                            tracing::warn!(event = "astra.emu.artemis.engine_log", "{message}")
                        }
                        Some('D') => {
                            tracing::debug!(event = "astra.emu.artemis.engine_log", "{message}")
                        }
                        Some('T') => {
                            tracing::trace!(event = "astra.emu.artemis.engine_log", "{message}")
                        }
                        _ => tracing::info!(event = "astra.emu.artemis.engine_log", "{message}"),
                    }
                }
                EVENT_KIND_MEDIA => commands.extend(parse_media(payload, resources)?),
                EVENT_KIND_UI => {
                    window.push(parse_ui(payload)?)?;
                }
                _ => return Err(invalid()),
            }
        }
        if offset != written {
            return Err(invalid());
        }
    }
    Ok(commands)
}
fn parse_ui(bytes: &[u8]) -> NativeResult<WindowCommand> {
    let envelope: CommandEnvelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if envelope.kind == "caption" {
        #[derive(Deserialize)]
        struct Caption {
            data: String,
        }
        let caption: Caption = parse(envelope.payload)?;
        if caption.data.len() > 4096 || caption.data.chars().any(char::is_control) {
            return Err(invalid());
        }
        Ok(WindowCommand::Title(caption.data))
    } else if envelope.kind == "mouse" {
        #[derive(Deserialize)]
        struct Mouse {
            left: Option<i32>,
            top: Option<i32>,
            hide: Option<i32>,
            autohide: Option<u64>,
        }
        let mouse: Mouse = parse(envelope.payload)?;
        Ok(WindowCommand::Pointer {
            x: mouse.left,
            y: mouse.top,
            visible: mouse.hide.map(|hide| hide == 0),
            autohide_ms: mouse.autohide,
        })
    } else {
        Err(error::invalid(
            "ASTRA_EMU_ARTEMIS_UI_UNSUPPORTED",
            format!(
                "native UI request is not connected: {}",
                envelope.kind.chars().take(128).collect::<String>()
            ),
        ))
    }
}
fn invalid() -> NativeError {
    error::invalid(
        "ASTRA_EMU_ARTEMIS_MEDIA_COMMAND",
        "invalid native media command",
    )
}
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> NativeResult<T> {
    serde_json::from_value(value).map_err(|_| invalid())
}
fn parse_media(bytes: &[u8], resources: &HostResources) -> NativeResult<Vec<MixerCommand>> {
    let envelope: CommandEnvelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let command = match envelope.kind.as_str() {
        "video_stop_all" => MixerCommand::StopVideoAll,
        "video_audio_stop" => {
            let p: StopPayload = parse(envelope.payload)?;
            MixerCommand::StopVideo { id: p.id }
        }
        "video_audio_play" => {
            let p: VideoAudioPayload = parse(envelope.payload)?;
            MixerCommand::Play {
                id: p.id,
                channel: Channel::Video,
                sources: SourcePair {
                    base: Arc::new(VideoAudioSource::open(&p.path)?),
                    loop_file: None,
                },
                loop_play: p.r#loop,
                gain: 1.0,
                pan: 0.0,
                fade_ms: 0,
            }
        }
        "audio_set_volume" => {
            let p: VolumePayload = parse(envelope.payload)?;
            if !p.value.is_finite() || !(0.0..=1.0).contains(&p.value) {
                return Err(invalid());
            }
            MixerCommand::SetVolume {
                channel: channel_of(&p.channel).ok_or_else(invalid)?,
                value: p.value as f32,
            }
        }
        "audio_bgm_play" | "audio_se_play" | "audio_voice_play" => {
            let p: PlayPayload = parse(envelope.payload)?;
            let channel = match envelope.kind.as_str() {
                "audio_bgm_play" => Channel::Bgm,
                "audio_voice_play" => Channel::Voice,
                _ => Channel::Se,
            };
            let id = if channel == Channel::Bgm {
                None
            } else {
                p.id.clone()
            };
            MixerCommand::Play {
                id,
                channel,
                sources: resolve_sources(&p, resources)?,
                loop_play: p.r#loop,
                gain: compat_gain(p.gain),
                pan: compat_pan(p.pan),
                fade_ms: p.fade_ms.unwrap_or(0),
            }
        }
        "audio_bgm_stop" | "audio_se_stop" => {
            let p: StopPayload = parse(envelope.payload)?;
            MixerCommand::Stop {
                id: p.id,
                fade_ms: p.fade_ms.unwrap_or(0),
            }
        }
        "audio_bgm_fade" | "audio_se_fade" => {
            let p: FadePayload = parse(envelope.payload)?;
            MixerCommand::Fade {
                id: p.id,
                gain: compat_gain(Some(p.gain)),
                time_ms: p.time_ms.unwrap_or(0),
            }
        }
        "audio_bgm_pan" | "audio_se_pan" => {
            let p: PanPayload = parse(envelope.payload)?;
            MixerCommand::Pan {
                id: p.id,
                pan: compat_pan(Some(p.pan)),
                time_ms: p.time_ms.unwrap_or(0),
            }
        }
        "audio_stop_all" => MixerCommand::StopAll { fade_ms: 0 },
        _ => {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_MEDIA_UNSUPPORTED",
                format!("native command is not connected: {}", envelope.kind),
            ))
        }
    };
    Ok(vec![command])
}
fn resolve_sources(p: &PlayPayload, resources: &HostResources) -> NativeResult<SourcePair> {
    let open = |path: &str| -> NativeResult<Arc<dyn MediaSource>> {
        resources
            .open_media_source(path)
            .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_SOURCE", e.to_string()))
    };
    Ok(SourcePair {
        base: open(p.resolved_file.as_deref().unwrap_or(&p.file))?,
        loop_file: p
            .resolved_loop_file
            .as_deref()
            .or(p.loop_file.as_deref())
            .map(open)
            .transpose()?,
    })
}
fn channel_of(name: &str) -> Option<Channel> {
    match name {
        "bgm" => Some(Channel::Bgm),
        "se" => Some(Channel::Se),
        "voice" => Some(Channel::Voice),
        _ => None,
    }
}

fn compat_gain(raw: Option<i64>) -> f32 {
    raw.map_or(1.0, |v| (v as f32 / 1000.0).max(0.0))
}
fn compat_pan(raw: Option<i64>) -> f32 {
    (raw.unwrap_or(0) as f32 / 1000.0).clamp(-1.0, 1.0)
}

#[cfg(test)]
#[path = "media/tests.rs"]
mod tests;
