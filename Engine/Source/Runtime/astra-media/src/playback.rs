mod contract;
mod validation;

use std::collections::VecDeque;

use astra_core::{Diagnostic, Hash256};

use crate::MediaError;

pub use contract::*;
use validation::*;

impl MediaPlaybackSession {
    pub fn new(config: MediaPlaybackConfig) -> Result<Self, MediaError> {
        validate_config(&config)?;
        Ok(Self {
            schema: MEDIA_PLAYBACK_SCHEMA.to_string(),
            config,
            state: MediaPlaybackState::Ready,
            generation: 1,
            position_us: 0,
            last_tick_sequence: 0,
            last_audio_playhead_us: None,
            video_eos_us: None,
            audio_eos_us: None,
            video_queue: VecDeque::new(),
            audio_queue: VecDeque::new(),
            next_video_sequence: 1,
            next_audio_sequence: 1,
            resume_after_seek: false,
        })
    }

    pub fn queue_video(&mut self, packet: VideoFramePacket) -> Result<(), MediaError> {
        let mut next = self.clone();
        next.queue_video_inner(packet)?;
        *self = next;
        Ok(())
    }

    pub fn queue_audio(&mut self, packet: AudioFramePacket) -> Result<(), MediaError> {
        let mut next = self.clone();
        next.queue_audio_inner(packet)?;
        *self = next;
        Ok(())
    }

    pub fn mark_eos(&mut self, track: MediaTrackKind, final_pts_us: u64) -> Result<(), MediaError> {
        let mut next = self.clone();
        next.ensure_accepts_packets("media.playback.eos")?;
        if final_pts_us > next.config.duration_us {
            return Err(playback_error(
                "ASTRA_MEDIA_EOS_RANGE",
                "end-of-stream timestamp exceeds the declared duration",
            ));
        }
        match track {
            MediaTrackKind::Audio if !next.config.has_audio => {
                return Err(playback_error(
                    "ASTRA_MEDIA_TRACK_DISABLED",
                    "audio EOS was submitted to a session without audio",
                ))
            }
            MediaTrackKind::Video if !next.config.has_video => {
                return Err(playback_error(
                    "ASTRA_MEDIA_TRACK_DISABLED",
                    "video EOS was submitted to a session without video",
                ))
            }
            MediaTrackKind::Audio if next.audio_eos_us.is_some() => {
                return Err(playback_error(
                    "ASTRA_MEDIA_DUPLICATE_EOS",
                    "audio EOS was submitted more than once",
                ))
            }
            MediaTrackKind::Video if next.video_eos_us.is_some() => {
                return Err(playback_error(
                    "ASTRA_MEDIA_DUPLICATE_EOS",
                    "video EOS was submitted more than once",
                ))
            }
            MediaTrackKind::Audio => next.audio_eos_us = Some(final_pts_us),
            MediaTrackKind::Video => next.video_eos_us = Some(final_pts_us),
        }
        next.validate_eos_after_queued_packets(track, final_pts_us)?;
        *self = next;
        Ok(())
    }

    pub fn play(&mut self) -> Result<(), MediaError> {
        match self.state {
            MediaPlaybackState::Ready | MediaPlaybackState::Paused => {
                self.ensure_initial_buffer()?;
                self.state = MediaPlaybackState::Playing;
                Ok(())
            }
            _ => Err(playback_error(
                "ASTRA_MEDIA_PLAY_STATE",
                "only a ready or paused media session can play",
            )),
        }
    }

    pub fn pause(&mut self) -> Result<(), MediaError> {
        if self.state != MediaPlaybackState::Playing {
            return Err(playback_error(
                "ASTRA_MEDIA_PAUSE_STATE",
                "only a playing media session can pause",
            ));
        }
        self.state = MediaPlaybackState::Paused;
        Ok(())
    }

    pub fn seek(&mut self, position_us: u64) -> Result<u64, MediaError> {
        if matches!(
            self.state,
            MediaPlaybackState::Seeking | MediaPlaybackState::Cancelled
        ) || position_us > self.config.duration_us
        {
            return Err(playback_error(
                "ASTRA_MEDIA_SEEK_STATE",
                "media seek state or target is invalid",
            ));
        }
        let generation = self.generation.checked_add(1).ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_GENERATION_OVERFLOW",
                "media playback generation overflowed",
            )
        })?;
        self.resume_after_seek = self.state == MediaPlaybackState::Playing;
        self.state = MediaPlaybackState::Seeking;
        self.generation = generation;
        self.position_us = position_us;
        self.last_audio_playhead_us = None;
        self.video_eos_us = None;
        self.audio_eos_us = None;
        self.video_queue.clear();
        self.audio_queue.clear();
        self.next_video_sequence = 1;
        self.next_audio_sequence = 1;
        Ok(generation)
    }

    pub fn complete_seek(&mut self) -> Result<(), MediaError> {
        if self.state != MediaPlaybackState::Seeking {
            return Err(playback_error(
                "ASTRA_MEDIA_SEEK_STATE",
                "complete_seek requires a pending seek",
            ));
        }
        self.ensure_initial_buffer()?;
        self.state = if self.resume_after_seek {
            MediaPlaybackState::Playing
        } else {
            MediaPlaybackState::Paused
        };
        self.resume_after_seek = false;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), MediaError> {
        if self.state == MediaPlaybackState::Cancelled {
            return Err(playback_error(
                "ASTRA_MEDIA_CANCEL_STATE",
                "media playback was already cancelled",
            ));
        }
        self.state = MediaPlaybackState::Cancelled;
        self.video_queue.clear();
        self.audio_queue.clear();
        Ok(())
    }

    pub fn tick(&mut self, request: PlaybackTickRequest) -> Result<PlaybackTickOutput, MediaError> {
        let mut next = self.clone();
        let output = next.tick_inner(request)?;
        *self = next;
        Ok(output)
    }

    pub fn deterministic_hash(&self) -> Result<Hash256, MediaError> {
        self.snapshot().map(|bytes| Hash256::from_sha256(&bytes))
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, MediaError> {
        postcard::to_allocvec(self).map_err(|error| {
            playback_error(
                "ASTRA_MEDIA_PLAYBACK_SERIALIZATION",
                format!("media playback snapshot serialization failed: {error}"),
            )
        })
    }

    pub fn restore(bytes: &[u8]) -> Result<Self, MediaError> {
        let session: Self = postcard::from_bytes(bytes).map_err(|error| {
            playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                format!("media playback snapshot decode failed: {error}"),
            )
        })?;
        session.validate_restored()?;
        Ok(session)
    }

    fn queue_video_inner(&mut self, packet: VideoFramePacket) -> Result<(), MediaError> {
        self.ensure_accepts_packets("media.playback.queue_video")?;
        if !self.config.has_video {
            return Err(playback_error(
                "ASTRA_MEDIA_TRACK_DISABLED",
                "video packet was submitted to a session without video",
            ));
        }
        if self.video_eos_us.is_some()
            || self.video_queue.len() >= self.config.max_video_frames
            || packet.generation != self.generation
            || packet.sequence != self.next_video_sequence
            || packet.duration_us == 0
            || packet.width == 0
            || packet.height == 0
            || !safe_resource_id(&packet.resource_id)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_VIDEO_PACKET",
                "video packet identity, state, dimensions, sequence, or queue budget is invalid",
            ));
        }
        validate_packet_time(
            packet.pts_us,
            packet.duration_us,
            self.config.duration_us,
            self.video_queue
                .back()
                .map(|previous| previous.pts_us + previous.duration_us),
        )?;
        self.next_video_sequence = self.next_video_sequence.checked_add(1).ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_SEQUENCE_OVERFLOW",
                "video packet sequence overflowed",
            )
        })?;
        self.video_queue.push_back(packet);
        Ok(())
    }

    fn queue_audio_inner(&mut self, packet: AudioFramePacket) -> Result<(), MediaError> {
        self.ensure_accepts_packets("media.playback.queue_audio")?;
        if !self.config.has_audio {
            return Err(playback_error(
                "ASTRA_MEDIA_TRACK_DISABLED",
                "audio packet was submitted to a session without audio",
            ));
        }
        if self.audio_eos_us.is_some()
            || self.audio_queue.len() >= self.config.max_audio_packets
            || packet.generation != self.generation
            || packet.sequence != self.next_audio_sequence
            || packet.duration_us == 0
            || packet.sample_rate == 0
            || packet.channels == 0
            || packet.channels > 8
            || packet.frame_count == 0
            || !valid_audio_packet_duration(&packet)
            || !safe_resource_id(&packet.resource_id)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_AUDIO_PACKET",
                "audio packet identity, format, duration, sequence, or queue budget is invalid",
            ));
        }
        validate_packet_time(
            packet.pts_us,
            packet.duration_us,
            self.config.duration_us,
            self.audio_queue
                .back()
                .map(|previous| previous.pts_us + previous.duration_us),
        )?;
        self.next_audio_sequence = self.next_audio_sequence.checked_add(1).ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_SEQUENCE_OVERFLOW",
                "audio packet sequence overflowed",
            )
        })?;
        self.audio_queue.push_back(packet);
        Ok(())
    }

    fn tick_inner(
        &mut self,
        request: PlaybackTickRequest,
    ) -> Result<PlaybackTickOutput, MediaError> {
        let expected_sequence = self.last_tick_sequence.checked_add(1).ok_or_else(|| {
            playback_error(
                "ASTRA_MEDIA_SEQUENCE_OVERFLOW",
                "media tick sequence overflowed",
            )
        })?;
        if self.state != MediaPlaybackState::Playing
            || request.sequence != expected_sequence
            || request.delta_us == 0
            || request.delta_us > self.config.max_tick_us
        {
            return Err(playback_error(
                "ASTRA_MEDIA_TICK",
                "media tick state, sequence, or delta is invalid",
            ));
        }
        let predicted = self
            .position_us
            .checked_add(request.delta_us)
            .ok_or_else(|| {
                playback_error(
                    "ASTRA_MEDIA_CLOCK_OVERFLOW",
                    "media playback clock overflowed",
                )
            })?;
        let master = if self.config.has_audio {
            let audio = request.audio_playhead_us.ok_or_else(|| {
                playback_error(
                    "ASTRA_MEDIA_AUDIO_CLOCK_MISSING",
                    "audio playback requires a host callback playhead",
                )
            })?;
            if self.last_audio_playhead_us.is_some_and(|last| audio < last)
                || audio > self.config.duration_us
                || self.last_audio_playhead_us.is_some_and(|last| {
                    audio.saturating_sub(last) > self.config.max_audio_clock_jump_us
                })
            {
                return Err(playback_error(
                    "ASTRA_MEDIA_AUDIO_CLOCK",
                    "audio playhead moved backward, jumped, or exceeded duration",
                ));
            }
            audio
        } else {
            if request.audio_playhead_us.is_some() {
                return Err(playback_error(
                    "ASTRA_MEDIA_AUDIO_CLOCK_UNEXPECTED",
                    "audio playhead was supplied to a video-only session",
                ));
            }
            predicted.min(self.config.duration_us)
        };
        if master < self.position_us {
            return Err(playback_error(
                "ASTRA_MEDIA_CLOCK_REWIND",
                "media master clock moved backward",
            ));
        }

        let mut released_audio = Vec::new();
        while self
            .audio_queue
            .front()
            .is_some_and(|packet| packet.pts_us + packet.duration_us <= master)
        {
            released_audio.push(self.audio_queue.pop_front().ok_or_else(|| {
                playback_error(
                    "ASTRA_MEDIA_QUEUE_CORRUPT",
                    "audio queue changed while releasing a validated packet",
                )
            })?);
        }

        let mut dropped_video = Vec::new();
        while self.video_queue.front().is_some_and(|packet| {
            packet.pts_us + packet.duration_us + self.config.max_video_lag_us < master
        }) {
            if self.config.late_video_policy == LateVideoPolicy::Block {
                return Err(playback_error(
                    "ASTRA_MEDIA_AV_SYNC_LATE",
                    "video frame exceeded the profile-bound A/V lag budget",
                ));
            }
            dropped_video.push(self.video_queue.pop_front().ok_or_else(|| {
                playback_error(
                    "ASTRA_MEDIA_QUEUE_CORRUPT",
                    "video queue changed while dropping a validated packet",
                )
            })?);
        }

        let presented_video = if self
            .video_queue
            .front()
            .is_some_and(|packet| packet.pts_us <= master + self.config.max_video_lead_us)
        {
            self.video_queue.pop_front()
        } else {
            None
        };
        let av_drift_us = presented_video
            .as_ref()
            .map(|frame| frame.pts_us as i64 - master as i64);

        self.position_us = master;
        self.last_tick_sequence = request.sequence;
        self.last_audio_playhead_us = request.audio_playhead_us;
        let ended = self.all_required_eos()
            && self.audio_queue.is_empty()
            && self.video_queue.is_empty()
            && master >= self.final_eos_us();
        if ended {
            self.state = MediaPlaybackState::Ended;
        }
        Ok(PlaybackTickOutput {
            sequence: request.sequence,
            position_us: master,
            presented_video,
            dropped_video,
            released_audio,
            av_drift_us,
            ended,
        })
    }

    fn ensure_accepts_packets(&self, operation: &'static str) -> Result<(), MediaError> {
        if matches!(
            self.state,
            MediaPlaybackState::Ended | MediaPlaybackState::Cancelled
        ) {
            return Err(playback_error(
                "ASTRA_MEDIA_SESSION_CLOSED",
                format!("{operation} cannot mutate a closed media session"),
            ));
        }
        Ok(())
    }

    fn ensure_initial_buffer(&self) -> Result<(), MediaError> {
        let audio_ready = !self.config.has_audio
            || !self.audio_queue.is_empty()
            || self.audio_eos_us == Some(self.position_us);
        let video_ready = !self.config.has_video
            || !self.video_queue.is_empty()
            || self.video_eos_us == Some(self.position_us);
        if !audio_ready || !video_ready {
            return Err(playback_error(
                "ASTRA_MEDIA_BUFFERING",
                "every enabled track must provide a packet or terminal EOS before playback",
            ));
        }
        Ok(())
    }

    fn validate_eos_after_queued_packets(
        &self,
        track: MediaTrackKind,
        eos_us: u64,
    ) -> Result<(), MediaError> {
        let queued_end = match track {
            MediaTrackKind::Audio => self
                .audio_queue
                .back()
                .map(|packet| packet.pts_us + packet.duration_us),
            MediaTrackKind::Video => self
                .video_queue
                .back()
                .map(|packet| packet.pts_us + packet.duration_us),
        };
        if queued_end.is_some_and(|end| eos_us < end) {
            return Err(playback_error(
                "ASTRA_MEDIA_EOS_ORDER",
                "end-of-stream timestamp precedes a queued packet",
            ));
        }
        Ok(())
    }

    fn all_required_eos(&self) -> bool {
        (!self.config.has_audio || self.audio_eos_us.is_some())
            && (!self.config.has_video || self.video_eos_us.is_some())
    }

    fn final_eos_us(&self) -> u64 {
        self.audio_eos_us
            .unwrap_or(0)
            .max(self.video_eos_us.unwrap_or(0))
    }

    fn validate_restored(&self) -> Result<(), MediaError> {
        if self.schema != MEDIA_PLAYBACK_SCHEMA
            || self.generation == 0
            || self.position_us > self.config.duration_us
            || self.video_queue.len() > self.config.max_video_frames
            || self.audio_queue.len() > self.config.max_audio_packets
            || self
                .video_eos_us
                .is_some_and(|eos| eos > self.config.duration_us)
            || self
                .audio_eos_us
                .is_some_and(|eos| eos > self.config.duration_us)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "media playback snapshot identity, clock, EOS, or queue budget is invalid",
            ));
        }
        validate_config(&self.config)?;
        validate_video_queue(self)?;
        validate_audio_queue(self)?;
        if self.state == MediaPlaybackState::Ended
            && (!self.all_required_eos()
                || !self.video_queue.is_empty()
                || !self.audio_queue.is_empty())
        {
            return Err(playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "ended media snapshot still has pending packets or missing EOS",
            ));
        }
        if self.state == MediaPlaybackState::Cancelled
            && (!self.video_queue.is_empty() || !self.audio_queue.is_empty())
        {
            return Err(playback_error(
                "ASTRA_MEDIA_PLAYBACK_SNAPSHOT",
                "cancelled media snapshot still owns queued packets",
            ));
        }
        Ok(())
    }
}

fn playback_error(code: impl Into<String>, message: impl Into<String>) -> MediaError {
    MediaError::Diagnostics(vec![Diagnostic::blocking(code, message)])
}
