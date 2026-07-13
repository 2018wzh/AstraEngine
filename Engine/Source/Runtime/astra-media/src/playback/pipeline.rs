use std::collections::BTreeMap;

use astra_core::Hash256;

use super::{
    playback_error, AudioFramePacket, MediaPlaybackConfig, MediaPlaybackSession, MediaTrackKind,
    PlaybackTickOutput, PlaybackTickRequest, VideoFramePacket,
};
use crate::MediaError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedMediaPacket {
    Audio {
        packet: AudioFramePacket,
        pcm_s16le: Vec<u8>,
    },
    Video {
        packet: VideoFramePacket,
        bgra8: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueuedMediaOutput {
    Audio {
        packet: AudioFramePacket,
        pcm_s16le: Vec<u8>,
    },
    VideoBuffered {
        resource_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedVideoFrame {
    pub packet: VideoFramePacket,
    pub bgra8: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPipelineTickOutput {
    pub scheduler: PlaybackTickOutput,
    pub presented_video: Option<PresentedVideoFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPipelineLimits {
    pub max_live_audio_bytes: usize,
    pub max_live_video_bytes: usize,
}

impl Default for MediaPipelineLimits {
    fn default() -> Self {
        Self {
            max_live_audio_bytes: 32 * 1024 * 1024,
            max_live_video_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct MediaPlaybackPipeline {
    scheduler: MediaPlaybackSession,
    limits: MediaPipelineLimits,
    live_audio_resources: BTreeMap<String, usize>,
    video_payloads: BTreeMap<String, Vec<u8>>,
    live_audio_bytes: usize,
    live_video_bytes: usize,
}

impl MediaPlaybackPipeline {
    pub fn new(
        config: MediaPlaybackConfig,
        limits: MediaPipelineLimits,
    ) -> Result<Self, MediaError> {
        if limits.max_live_audio_bytes == 0 || limits.max_live_video_bytes == 0 {
            return Err(playback_error(
                "ASTRA_MEDIA_PIPELINE_LIMITS",
                "media payload budgets must be non-zero",
            ));
        }
        Ok(Self {
            scheduler: MediaPlaybackSession::new(config)?,
            limits,
            live_audio_resources: BTreeMap::new(),
            video_payloads: BTreeMap::new(),
            live_audio_bytes: 0,
            live_video_bytes: 0,
        })
    }

    pub fn scheduler(&self) -> &MediaPlaybackSession {
        &self.scheduler
    }

    pub fn queue_decoded(
        &mut self,
        decoded: DecodedMediaPacket,
    ) -> Result<QueuedMediaOutput, MediaError> {
        match decoded {
            DecodedMediaPacket::Audio { packet, pcm_s16le } => {
                validate_audio_payload(&packet, &pcm_s16le)?;
                if self.live_audio_resources.contains_key(&packet.resource_id)
                    || self.video_payloads.contains_key(&packet.resource_id)
                {
                    return Err(playback_error(
                        "ASTRA_MEDIA_RESOURCE_DUPLICATE",
                        "decoded media resource id is already live",
                    ));
                }
                let live_audio_bytes = self
                    .live_audio_bytes
                    .checked_add(pcm_s16le.len())
                    .filter(|bytes| *bytes <= self.limits.max_live_audio_bytes)
                    .ok_or_else(|| {
                        playback_error(
                            "ASTRA_MEDIA_AUDIO_PAYLOAD_BUDGET",
                            "live decoded PCM exceeds its profile-bound budget",
                        )
                    })?;
                self.scheduler.queue_audio(packet.clone())?;
                self.live_audio_resources
                    .insert(packet.resource_id.clone(), pcm_s16le.len());
                self.live_audio_bytes = live_audio_bytes;
                Ok(QueuedMediaOutput::Audio { packet, pcm_s16le })
            }
            DecodedMediaPacket::Video { packet, bgra8 } => {
                validate_video_payload(&packet, &bgra8)?;
                if self.video_payloads.contains_key(&packet.resource_id)
                    || self.live_audio_resources.contains_key(&packet.resource_id)
                {
                    return Err(playback_error(
                        "ASTRA_MEDIA_RESOURCE_DUPLICATE",
                        "decoded media resource id is already live",
                    ));
                }
                let live_video_bytes = self
                    .live_video_bytes
                    .checked_add(bgra8.len())
                    .filter(|bytes| *bytes <= self.limits.max_live_video_bytes)
                    .ok_or_else(|| {
                        playback_error(
                            "ASTRA_MEDIA_VIDEO_PAYLOAD_BUDGET",
                            "live decoded BGRA exceeds its profile-bound budget",
                        )
                    })?;
                self.scheduler.queue_video(packet.clone())?;
                self.video_payloads
                    .insert(packet.resource_id.clone(), bgra8);
                self.live_video_bytes = live_video_bytes;
                Ok(QueuedMediaOutput::VideoBuffered {
                    resource_id: packet.resource_id,
                })
            }
        }
    }

    pub fn mark_eos(&mut self, track: MediaTrackKind, final_pts_us: u64) -> Result<(), MediaError> {
        self.scheduler.mark_eos(track, final_pts_us)
    }

    pub fn play(&mut self) -> Result<(), MediaError> {
        self.scheduler.play()
    }

    pub fn pause(&mut self) -> Result<(), MediaError> {
        self.scheduler.pause()
    }

    pub fn begin_seek(&mut self, position_us: u64) -> Result<u64, MediaError> {
        let generation = self.scheduler.seek(position_us)?;
        self.live_audio_resources.clear();
        self.video_payloads.clear();
        self.live_audio_bytes = 0;
        self.live_video_bytes = 0;
        Ok(generation)
    }

    pub fn complete_seek(&mut self, generation: u64) -> Result<(), MediaError> {
        if generation != self.scheduler.generation {
            return Err(playback_error(
                "ASTRA_MEDIA_SEEK_GENERATION",
                "decoder generation does not match the playback scheduler",
            ));
        }
        self.scheduler.complete_seek()
    }

    pub fn cancel(&mut self) -> Result<(), MediaError> {
        self.scheduler.cancel()?;
        self.live_audio_resources.clear();
        self.video_payloads.clear();
        self.live_audio_bytes = 0;
        self.live_video_bytes = 0;
        Ok(())
    }

    pub fn tick(
        &mut self,
        request: PlaybackTickRequest,
    ) -> Result<MediaPipelineTickOutput, MediaError> {
        let mut next_scheduler = self.scheduler.clone();
        let scheduler = next_scheduler.tick(request)?;
        for packet in &scheduler.released_audio {
            if !self.live_audio_resources.contains_key(&packet.resource_id) {
                return Err(playback_error(
                    "ASTRA_MEDIA_RESOURCE_MISSING",
                    "released audio packet has no owned PCM payload",
                ));
            }
        }
        for packet in &scheduler.dropped_video {
            if !self.video_payloads.contains_key(&packet.resource_id) {
                return Err(playback_error(
                    "ASTRA_MEDIA_RESOURCE_MISSING",
                    "dropped video packet has no owned BGRA payload",
                ));
            }
        }
        if scheduler
            .presented_video
            .as_ref()
            .is_some_and(|packet| !self.video_payloads.contains_key(&packet.resource_id))
        {
            return Err(playback_error(
                "ASTRA_MEDIA_RESOURCE_MISSING",
                "presented video packet has no owned BGRA payload",
            ));
        }
        let released_audio_bytes =
            scheduler
                .released_audio
                .iter()
                .try_fold(0_usize, |total, packet| {
                    let bytes = self
                        .live_audio_resources
                        .get(&packet.resource_id)
                        .copied()
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_RESOURCE_MISSING",
                                "released audio packet has no byte accounting entry",
                            )
                        })?;
                    total.checked_add(bytes).ok_or_else(|| {
                        playback_error(
                            "ASTRA_MEDIA_RESOURCE_ACCOUNTING",
                            "released audio byte total overflowed",
                        )
                    })
                })?;
        let dropped_video_bytes =
            scheduler
                .dropped_video
                .iter()
                .try_fold(0_usize, |total, packet| {
                    let bytes = self
                        .video_payloads
                        .get(&packet.resource_id)
                        .map(Vec::len)
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_RESOURCE_MISSING",
                                "dropped video packet has no byte accounting entry",
                            )
                        })?;
                    total.checked_add(bytes).ok_or_else(|| {
                        playback_error(
                            "ASTRA_MEDIA_RESOURCE_ACCOUNTING",
                            "dropped video byte total overflowed",
                        )
                    })
                })?;
        let presented_video_bytes = scheduler
            .presented_video
            .as_ref()
            .map(|packet| {
                self.video_payloads
                    .get(&packet.resource_id)
                    .map(Vec::len)
                    .ok_or_else(|| {
                        playback_error(
                            "ASTRA_MEDIA_RESOURCE_MISSING",
                            "presented video packet has no byte accounting entry",
                        )
                    })
            })
            .transpose()?
            .unwrap_or(0);
        let next_audio_bytes = self
            .live_audio_bytes
            .checked_sub(released_audio_bytes)
            .ok_or_else(|| {
                playback_error(
                    "ASTRA_MEDIA_RESOURCE_ACCOUNTING",
                    "live audio byte accounting underflowed",
                )
            })?;
        let next_video_bytes = dropped_video_bytes
            .checked_add(presented_video_bytes)
            .and_then(|released| self.live_video_bytes.checked_sub(released))
            .ok_or_else(|| {
                playback_error(
                    "ASTRA_MEDIA_RESOURCE_ACCOUNTING",
                    "live video byte accounting underflowed",
                )
            })?;
        let presented_video = if let Some(packet) = &scheduler.presented_video {
            let bgra8 = self
                .video_payloads
                .remove(&packet.resource_id)
                .ok_or_else(|| {
                    playback_error(
                        "ASTRA_MEDIA_RESOURCE_MISSING",
                        "presented video packet disappeared before commit",
                    )
                })?;
            Some(PresentedVideoFrame {
                packet: packet.clone(),
                bgra8,
            })
        } else {
            None
        };
        for packet in &scheduler.released_audio {
            let _ = self.live_audio_resources.remove(&packet.resource_id);
        }
        for packet in &scheduler.dropped_video {
            let _ = self.video_payloads.remove(&packet.resource_id);
        }
        self.scheduler = next_scheduler;
        self.live_audio_bytes = next_audio_bytes;
        self.live_video_bytes = next_video_bytes;
        Ok(MediaPipelineTickOutput {
            scheduler,
            presented_video,
        })
    }
}

fn validate_audio_payload(packet: &AudioFramePacket, bytes: &[u8]) -> Result<(), MediaError> {
    let expected = usize::try_from(packet.frame_count)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(packet.channels)))
        .and_then(|samples| samples.checked_mul(2));
    if expected != Some(bytes.len()) || Hash256::from_sha256(bytes) != packet.content_hash {
        return Err(playback_error(
            "ASTRA_MEDIA_AUDIO_PAYLOAD",
            "decoded PCM size or content hash does not match its packet",
        ));
    }
    Ok(())
}

fn validate_video_payload(packet: &VideoFramePacket, bytes: &[u8]) -> Result<(), MediaError> {
    let expected = usize::try_from(packet.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .and_then(|row| {
            usize::try_from(packet.height)
                .ok()
                .and_then(|height| row.checked_mul(height))
        });
    if expected != Some(bytes.len()) || Hash256::from_sha256(bytes) != packet.content_hash {
        return Err(playback_error(
            "ASTRA_MEDIA_VIDEO_PAYLOAD",
            "decoded BGRA size or content hash does not match its packet",
        ));
    }
    Ok(())
}
