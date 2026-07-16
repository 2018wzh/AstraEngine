use std::{collections::VecDeque, sync::Arc};

use astra_emu_family_api::{LegacyVideoCommandV1, LegacyVideoMode};
use astra_emu_fvp::{
    fvp_movie_compatibility, open_fvp_movie_stream, FvpMovieAudioChunk, FvpMovieCompatibility,
    FvpMovieFrame, FvpMoviePacket, FvpMovieStreamDecoder,
};
use astra_media::{
    open_windows_audio_stream, open_windows_video_stream, PlayerDecodedAudio,
    WindowsAudioStreamDecoder, WindowsVideoStreamDecoder,
};

use crate::audio_executor::HostAudioExecutor;

pub(crate) const MAX_ENCODED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAX_AUDIO_SAMPLES: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 60 * 60 * 4;
const MOVIE_AUDIO_STREAM_BASE: u32 = 0xF000_0000;
const VIDEO_RING_FRAMES: usize = 16;
const VIDEO_PREFETCH_NS: u64 = 500_000_000;

#[derive(Clone)]
pub(crate) struct HostVideoFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba8: Arc<[u8]>,
    pub(crate) stage_width: u32,
    pub(crate) stage_height: u32,
    pub(crate) mode: LegacyVideoMode,
}

struct TimelineFrame {
    pts_ns: u64,
    width: u32,
    height: u32,
    rgba8: Arc<[u8]>,
}

struct ActiveMovie {
    playback_id: String,
    mode: LegacyVideoMode,
    stage_width: u32,
    stage_height: u32,
    frames: VecDeque<TimelineFrame>,
    decoder: MovieDecoder,
    duration_ns: Option<u64>,
    elapsed_ns: u64,
    audio_stream_id: Option<u32>,
    audio_started: bool,
    previous_frame_pts_ns: Option<u64>,
    inferred_frame_step_ns: u64,
    eof: bool,
}

enum MovieDecoder {
    Native(FvpMovieStreamDecoder),
    Platform {
        video: WindowsVideoStreamDecoder,
        audio: Option<WindowsAudioStreamDecoder>,
        next_video: Option<FvpMovieFrame>,
        next_audio: Option<FvpMovieAudioChunk>,
        video_eof: bool,
        audio_eof: bool,
    },
    #[cfg(test)]
    Buffered(VecDeque<FvpMoviePacket>),
}

impl MovieDecoder {
    fn next_packet(&mut self) -> Result<FvpMoviePacket, String> {
        match self {
            Self::Native(decoder) => decoder.next_packet().map_err(|error| error.to_string()),
            Self::Platform {
                video,
                audio,
                next_video,
                next_audio,
                video_eof,
                audio_eof,
            } => {
                if next_video.is_none() && !*video_eof {
                    *next_video = video
                        .next_frame()
                        .map_err(|_| "ASTRA_EMU_VIDEO_PLATFORM_DECODE_FAILED".to_owned())?
                        .map(|frame| {
                            let mut rgba8 = frame.bgra8;
                            for pixel in rgba8.chunks_exact_mut(4) {
                                pixel.swap(0, 2);
                            }
                            FvpMovieFrame {
                                pts_ms: frame.pts_us / 1_000,
                                width: frame.width,
                                height: frame.height,
                                rgba8,
                            }
                        });
                    *video_eof = next_video.is_none();
                }
                if next_audio.is_none() && !*audio_eof {
                    *next_audio = match audio.as_mut() {
                        Some(decoder) => decoder
                            .next_chunk()
                            .map_err(|_| "ASTRA_EMU_VIDEO_AUDIO_DECODE_FAILED".to_owned())?
                            .map(|chunk| {
                                let parsed = PlayerDecodedAudio::parse(
                                    &format!("pcm_s16le:{}:{}", chunk.sample_rate, chunk.channels),
                                    &chunk.pcm_s16le,
                                    chunk.pcm_s16le.len() / 2,
                                )
                                .map_err(|_| "ASTRA_EMU_VIDEO_AUDIO_OUTPUT_INVALID".to_owned())?;
                                Ok::<_, String>(FvpMovieAudioChunk {
                                    pts_ms: chunk.pts_us / 1_000,
                                    sample_rate: parsed.sample_rate,
                                    channels: parsed.channels,
                                    samples: parsed.samples,
                                })
                            })
                            .transpose()?,
                        None => None,
                    };
                    *audio_eof = next_audio.is_none();
                }
                let take_audio = match (next_audio.as_ref(), next_video.as_ref()) {
                    (Some(audio), Some(video)) => audio.pts_ms <= video.pts_ms,
                    (Some(_), None) => true,
                    _ => false,
                };
                if take_audio {
                    Ok(FvpMoviePacket::Audio(next_audio.take().unwrap()))
                } else if let Some(frame) = next_video.take() {
                    Ok(FvpMoviePacket::Video(frame))
                } else {
                    Ok(FvpMoviePacket::End)
                }
            }
            #[cfg(test)]
            Self::Buffered(packets) => Ok(packets.pop_front().unwrap_or(FvpMoviePacket::End)),
        }
    }
}

#[derive(Default)]
pub(crate) struct HostVideoExecutor {
    active: Option<ActiveMovie>,
    completed: Vec<String>,
    audio_sequence: u32,
}

impl HostVideoExecutor {
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn execute(
        &mut self,
        command: LegacyVideoCommandV1,
        resolved_resource: Option<Vec<u8>>,
        audio: &mut HostAudioExecutor,
    ) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        match command {
            LegacyVideoCommandV1::Play {
                playback_id,
                resource_uri,
                mode,
                stage_width,
                stage_height,
            } => self.play(
                playback_id,
                resource_uri,
                mode,
                stage_width,
                stage_height,
                resolved_resource.ok_or_else(|| "ASTRA_EMU_VIDEO_RESOURCE_MISSING".to_owned())?,
                audio,
            ),
            LegacyVideoCommandV1::Stop { playback_id } => self.stop(&playback_id, audio, true),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn play(
        &mut self,
        playback_id: String,
        resource_uri: String,
        mode: LegacyVideoMode,
        stage_width: u32,
        stage_height: u32,
        bytes: Vec<u8>,
        audio: &mut HostAudioExecutor,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_VIDEO_PLAYBACK_ALREADY_ACTIVE".into());
        }
        let extension = resource_uri
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .ok_or_else(|| "ASTRA_EMU_VIDEO_EXTENSION_MISSING".to_owned())?;
        let compatibility = fvp_movie_compatibility(extension);
        match compatibility {
            FvpMovieCompatibility::Native | FvpMovieCompatibility::PlatformProviderRequired => {}
            FvpMovieCompatibility::Unsupported => {
                return Err("ASTRA_EMU_VIDEO_CODEC_UNSUPPORTED".into());
            }
        }
        let (decoder, duration_ns) = match compatibility {
            FvpMovieCompatibility::Native => (
                MovieDecoder::Native(
                    open_fvp_movie_stream(
                        extension,
                        Arc::from(bytes),
                        MAX_FRAMES,
                        MAX_DECODED_BYTES,
                        MAX_AUDIO_SAMPLES,
                    )
                    .map_err(|error| error.to_string())?,
                ),
                None,
            ),
            FvpMovieCompatibility::PlatformProviderRequired => {
                let audio = if matches!(mode, LegacyVideoMode::ModalWithAudio) {
                    Some(
                        open_windows_audio_stream(&bytes, MAX_AUDIO_SAMPLES as u64)
                            .map_err(|_| "ASTRA_EMU_VIDEO_AUDIO_DECODE_FAILED".to_owned())?,
                    )
                } else {
                    None
                };
                (
                    MovieDecoder::Platform {
                        video: open_windows_video_stream(
                            &bytes,
                            MAX_FRAMES as u64,
                            MAX_DECODED_BYTES as u64,
                        )
                        .map_err(|_| "ASTRA_EMU_VIDEO_PLATFORM_DECODE_FAILED".to_owned())?,
                        audio,
                        next_video: None,
                        next_audio: None,
                        video_eof: false,
                        audio_eof: false,
                    },
                    None,
                )
            }
            FvpMovieCompatibility::Unsupported => unreachable!(),
        };
        let audio_stream_id = if matches!(mode, LegacyVideoMode::ModalWithAudio) {
            let stream_id = MOVIE_AUDIO_STREAM_BASE
                .checked_add(self.audio_sequence)
                .ok_or_else(|| "ASTRA_EMU_MOVIE_AUDIO_ID_BOUNDS".to_owned())?;
            self.audio_sequence = self.audio_sequence.saturating_add(1);
            Some(stream_id)
        } else {
            None
        };
        let mut active = ActiveMovie {
            playback_id,
            mode,
            stage_width,
            stage_height,
            frames: VecDeque::new(),
            decoder,
            duration_ns,
            elapsed_ns: 0,
            audio_stream_id,
            audio_started: false,
            previous_frame_pts_ns: None,
            inferred_frame_step_ns: 34_000_000,
            eof: false,
        };
        pump_decoder(&mut active, audio)?;
        if active.frames.is_empty() {
            return Err("ASTRA_EMU_VIDEO_DECODE_NO_FRAME".into());
        }
        self.active = Some(active);
        Ok(())
    }

    pub(crate) fn advance(
        &mut self,
        delta_ns: u64,
        audio: &mut HostAudioExecutor,
    ) -> Result<(), String> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.elapsed_ns = active
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or_else(|| "ASTRA_EMU_VIDEO_TIMELINE_BOUNDS".to_owned())?;
        pump_decoder(active, audio)?;
        while active.frames.len() > 1
            && active
                .frames
                .get(1)
                .is_some_and(|frame| frame.pts_ns <= active.elapsed_ns)
        {
            active.frames.pop_front();
        }
        pump_decoder(active, audio)?;
        if active
            .duration_ns
            .is_some_and(|duration| active.eof && active.elapsed_ns >= duration)
        {
            let playback_id = active.playback_id.clone();
            self.stop(&playback_id, audio, true)?;
        }
        Ok(())
    }

    fn stop(
        &mut self,
        playback_id: &str,
        audio: &mut HostAudioExecutor,
        complete: bool,
    ) -> Result<(), String> {
        let active = self
            .active
            .take()
            .ok_or_else(|| "ASTRA_EMU_VIDEO_PLAYBACK_MISSING".to_owned())?;
        if active.playback_id != playback_id {
            self.active = Some(active);
            return Err("ASTRA_EMU_VIDEO_PLAYBACK_IDENTITY".into());
        }
        if let Some(stream_id) = active.audio_stream_id.filter(|_| active.audio_started) {
            audio.stop_movie_pcm(stream_id)?;
        }
        if complete {
            self.completed.push(playback_id.to_owned());
        }
        Ok(())
    }

    pub(crate) fn current_frame(&self) -> Option<HostVideoFrame> {
        let active = self.active.as_ref()?;
        let frame = active.frames.front()?;
        Some(HostVideoFrame {
            width: frame.width,
            height: frame.height,
            rgba8: Arc::clone(&frame.rgba8),
            stage_width: active.stage_width,
            stage_height: active.stage_height,
            mode: active.mode,
        })
    }

    pub(crate) fn take_completed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.completed)
    }

    pub(crate) fn reset(&mut self, audio: &mut HostAudioExecutor) -> Result<(), String> {
        if let Some(active) = self.active.take() {
            if let Some(stream_id) = active.audio_stream_id.filter(|_| active.audio_started) {
                audio.stop_movie_pcm(stream_id)?;
            }
        }
        self.completed.clear();
        Ok(())
    }
}

fn pump_decoder(active: &mut ActiveMovie, audio: &mut HostAudioExecutor) -> Result<(), String> {
    while !active.eof
        && active.frames.len() < VIDEO_RING_FRAMES
        && active
            .frames
            .back()
            .is_none_or(|frame| frame.pts_ns <= active.elapsed_ns.saturating_add(VIDEO_PREFETCH_NS))
    {
        match active.decoder.next_packet()? {
            FvpMoviePacket::Video(frame) => {
                let pts_ns = frame
                    .pts_ms
                    .checked_mul(1_000_000)
                    .ok_or_else(|| "ASTRA_EMU_VIDEO_TIMELINE_BOUNDS".to_owned())?;
                if let Some(previous) = active.previous_frame_pts_ns {
                    if pts_ns < previous {
                        return Err("ASTRA_EMU_VIDEO_TIMELINE_ORDER".into());
                    }
                    if pts_ns > previous {
                        active.inferred_frame_step_ns = pts_ns - previous;
                    }
                }
                active.previous_frame_pts_ns = Some(pts_ns);
                active.frames.push_back(TimelineFrame {
                    pts_ns,
                    width: frame.width,
                    height: frame.height,
                    rgba8: Arc::from(frame.rgba8),
                });
            }
            FvpMoviePacket::Audio(chunk) => {
                if let Some(stream_id) = active.audio_stream_id {
                    if active.audio_started {
                        audio.append_movie_stream(
                            stream_id,
                            chunk.sample_rate,
                            chunk.channels,
                            chunk.samples,
                        )?;
                    } else {
                        audio.begin_movie_stream(
                            stream_id,
                            chunk.sample_rate,
                            chunk.channels,
                            chunk.samples,
                        )?;
                        active.audio_started = true;
                    }
                }
            }
            FvpMoviePacket::End => {
                active.eof = true;
                if active.duration_ns.is_none() {
                    active.duration_ns = active
                        .previous_frame_pts_ns
                        .and_then(|pts| pts.checked_add(active.inferred_frame_step_ns));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_frame_tracks_fixed_timeline_without_wall_clock() {
        let mut executor = HostVideoExecutor {
            active: Some(ActiveMovie {
                playback_id: "movie.1".into(),
                mode: LegacyVideoMode::LayerNoAudio,
                stage_width: 1280,
                stage_height: 720,
                frames: VecDeque::from(vec![
                    TimelineFrame {
                        pts_ns: 0,
                        width: 1,
                        height: 1,
                        rgba8: Arc::from([1, 2, 3, 4]),
                    },
                    TimelineFrame {
                        pts_ns: 20,
                        width: 1,
                        height: 1,
                        rgba8: Arc::from([5, 6, 7, 8]),
                    },
                ]),
                decoder: MovieDecoder::Buffered(VecDeque::new()),
                duration_ns: Some(40),
                elapsed_ns: 0,
                audio_stream_id: None,
                audio_started: false,
                previous_frame_pts_ns: Some(20),
                inferred_frame_step_ns: 20,
                eof: true,
            }),
            ..Default::default()
        };
        executor.active.as_mut().unwrap().elapsed_ns = 20;
        while executor.active.as_ref().unwrap().frames.len() > 1
            && executor.active.as_ref().unwrap().frames[1].pts_ns
                <= executor.active.as_ref().unwrap().elapsed_ns
        {
            executor.active.as_mut().unwrap().frames.pop_front();
        }
        assert_eq!(&*executor.current_frame().unwrap().rgba8, &[5, 6, 7, 8]);
    }
}
