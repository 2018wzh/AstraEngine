use std::collections::VecDeque;

use super::{DecodedMediaPacket, IncrementalMediaDecoder, MediaPlaybackConfig};
use crate::{DecodedVideoFrame, MediaError, PlayerDecodedAudio};

use super::playback_error;

/// Host-owned resource limits for the generic incremental cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalPlaybackLimits {
    pub max_video_frame_bytes: usize,
    pub max_audio_samples: usize,
    pub max_pending_audio_samples: usize,
}

impl Default for IncrementalPlaybackLimits {
    fn default() -> Self {
        Self {
            max_video_frame_bytes: 64 * 1024 * 1024,
            max_audio_samples: 64 * 1024 * 1024,
            max_pending_audio_samples: 16 * 1024 * 1024,
        }
    }
}

/// One decoded audio chunk retained until the host submits it to its mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct IncrementalAudioChunk {
    pub pts_us: u64,
    pub duration_us: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

/// Counters collected by the shared cursor after a packet passes validation.
///
/// These values are deliberately codec-neutral.  Encoded byte counts and
/// codec-specific drop reasons belong to the selected decoder/provider, not to
/// the playback scheduler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncrementalPlaybackTelemetry {
    pub video_packets: u64,
    pub audio_packets: u64,
    pub decoded_frames: u64,
    pub decoded_audio_samples: u64,
    pub dropped_video_packets: u64,
}

/// Shared timestamped playback cursor used by family adapters and hosts.
///
/// The cursor performs bounded read-ahead, timestamp selection, BGRA frame
/// validation, and PCM conversion. Container and codec work remains entirely
/// inside the bound `IncrementalMediaDecoder`; the cursor has no family or
/// codec branches and never materialises a complete movie.
pub struct IncrementalMediaPlayback {
    decoder: Box<dyn IncrementalMediaDecoder>,
    config: MediaPlaybackConfig,
    limits: IncrementalPlaybackLimits,
    current: Option<DecodedVideoFrame>,
    pending: Option<DecodedVideoFrame>,
    audio: VecDeque<IncrementalAudioChunk>,
    pending_audio_samples: usize,
    last_video_pts_us: Option<u64>,
    last_audio_pts_us: Option<u64>,
    last_elapsed_us: u64,
    last_frame_sequence: u64,
    last_audio_sequence: u64,
    active_generation: Option<u64>,
    telemetry: IncrementalPlaybackTelemetry,
    ended: bool,
}

impl IncrementalMediaPlayback {
    pub fn open(
        decoder: Box<dyn IncrementalMediaDecoder>,
        limits: IncrementalPlaybackLimits,
    ) -> Result<Self, MediaError> {
        if limits.max_video_frame_bytes == 0
            || limits.max_audio_samples == 0
            || limits.max_pending_audio_samples == 0
        {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_LIMITS",
                "incremental playback limits must be non-zero",
            ));
        }
        let config = decoder.playback_config();
        super::validation::validate_config(&config).map_err(|error| {
            playback_error(
                "ASTRA_MEDIA_INCREMENTAL_CONFIG",
                format!("incremental decoder returned an invalid playback contract: {error}"),
            )
        })?;
        Ok(Self {
            decoder,
            config,
            limits,
            current: None,
            pending: None,
            audio: VecDeque::new(),
            pending_audio_samples: 0,
            last_video_pts_us: None,
            last_audio_pts_us: None,
            last_elapsed_us: 0,
            last_frame_sequence: 0,
            last_audio_sequence: 0,
            active_generation: None,
            telemetry: IncrementalPlaybackTelemetry::default(),
            ended: false,
        })
    }

    pub fn provider_id(&self) -> &'static str {
        self.decoder.provider_id()
    }

    pub fn config(&self) -> &MediaPlaybackConfig {
        &self.config
    }

    pub fn duration_us(&self) -> u64 {
        self.config.duration_us
    }

    /// Reports whether the bound decoder has reached end-of-stream.  The
    /// current frame and queued audio remain readable until the host drains
    /// them; callers must not treat this flag as permission to discard those
    /// already-decoded outputs.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    pub fn current_frame(&self) -> Option<&DecodedVideoFrame> {
        self.current.as_ref()
    }

    /// Moves the currently selected frame out of the cursor.  This is useful
    /// for hosts that keep their own bounded presentation ring and avoids a
    /// second full-frame allocation.  The scheduler continues from its
    /// pending timestamp on the next `advance` call.
    pub fn take_current_frame(&mut self) -> Option<DecodedVideoFrame> {
        self.current.take()
    }

    pub fn telemetry(&self) -> IncrementalPlaybackTelemetry {
        self.telemetry
    }

    pub fn drain_audio(&mut self) -> Vec<IncrementalAudioChunk> {
        self.pending_audio_samples = 0;
        self.audio.drain(..).collect()
    }

    pub fn advance(&mut self, elapsed_us: u64) -> Result<bool, MediaError> {
        if elapsed_us < self.last_elapsed_us {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_CLOCK",
                "incremental playback clock moved backwards",
            ));
        }
        if elapsed_us - self.last_elapsed_us > self.config.max_tick_us {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_TICK",
                "incremental playback clock advanced beyond its profile-bound tick budget",
            ));
        }
        self.last_elapsed_us = elapsed_us;
        if self.ended {
            return Ok(false);
        }
        let previous_sequence = self.current.as_ref().map(|frame| frame.sequence);
        let mut decoded_video_this_tick = 0_usize;
        let mut decoded_audio_this_tick = 0_usize;
        if self.pending.as_ref().is_some_and(|frame| {
            frame.pts_us > elapsed_us.saturating_add(self.config.max_video_lead_us)
        }) {
            return Ok(false);
        }
        if let Some(frame) = self.pending.take() {
            self.current = Some(frame);
        }
        loop {
            let Some(packet) = self.decoder.read_next()? else {
                self.ended = true;
                return Ok(previous_sequence != self.current.as_ref().map(|f| f.sequence));
            };
            match packet {
                DecodedMediaPacket::Video { packet, bgra8 } => {
                    decoded_video_this_tick = decoded_video_this_tick
                        .checked_add(1)
                        .filter(|count| *count <= self.config.max_video_frames)
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_VIDEO_BUDGET",
                                "incremental video decode exceeded the profile-bound frame budget",
                            )
                        })?;
                    if !self.config.has_video {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_TRACK",
                            "incremental decoder emitted video for an audio-only contract",
                        ));
                    }
                    if !self.accept_generation(packet.generation) {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_GENERATION",
                            "incremental video packet belongs to a different decoder generation",
                        ));
                    }
                    let expected = usize::try_from(packet.width).ok().and_then(|width| {
                        usize::try_from(packet.height).ok().and_then(|height| {
                            width
                                .checked_mul(height)
                                .and_then(|pixels| pixels.checked_mul(4))
                        })
                    });
                    if packet.generation == 0
                        || packet.sequence == 0
                        || packet.sequence <= self.last_frame_sequence
                        || packet.duration_us == 0
                        || packet.pts_us >= self.config.duration_us
                        || !super::validation::safe_resource_id(&packet.resource_id)
                        || bgra8.len() > self.limits.max_video_frame_bytes
                        || expected != Some(bgra8.len())
                        || packet
                            .pts_us
                            .checked_add(packet.duration_us)
                            .is_none_or(|end| end > self.config.duration_us)
                    {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_VIDEO",
                            "incremental video packet violates timing or payload limits",
                        ));
                    }
                    let frame = DecodedVideoFrame {
                        sequence: packet.sequence,
                        pts_us: packet.pts_us,
                        duration_us: packet.duration_us,
                        width: packet.width,
                        height: packet.height,
                        bgra8,
                    };
                    frame.validate()?;
                    if self
                        .last_video_pts_us
                        .is_some_and(|previous| frame.pts_us < previous)
                    {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_VIDEO",
                            "incremental video timestamps moved backwards",
                        ));
                    }
                    let frame_end_us =
                        frame.pts_us.checked_add(frame.duration_us).ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_VIDEO",
                                "incremental video timestamp overflowed",
                            )
                        })?;
                    let late =
                        frame_end_us.saturating_add(self.config.max_video_lag_us) < elapsed_us;
                    if late && self.config.late_video_policy == super::LateVideoPolicy::Block {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_AV_SYNC_LATE",
                            "incremental video frame exceeded the profile-bound A/V lag budget",
                        ));
                    }
                    self.last_frame_sequence = frame.sequence;
                    self.last_video_pts_us = Some(frame.pts_us);
                    self.telemetry.video_packets =
                        self.telemetry.video_packets.checked_add(1).ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_TELEMETRY",
                                "incremental video packet telemetry overflowed",
                            )
                        })?;
                    self.telemetry.decoded_frames = self
                        .telemetry
                        .decoded_frames
                        .checked_add(1)
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_TELEMETRY",
                                "incremental decoded frame telemetry overflowed",
                            )
                        })?;
                    if late {
                        self.telemetry.dropped_video_packets = self
                            .telemetry
                            .dropped_video_packets
                            .checked_add(1)
                            .ok_or_else(|| {
                                playback_error(
                                    "ASTRA_MEDIA_INCREMENTAL_TELEMETRY",
                                    "incremental dropped-frame telemetry overflowed",
                                )
                            })?;
                        continue;
                    }
                    if frame.pts_us <= elapsed_us.saturating_add(self.config.max_video_lead_us) {
                        self.current = Some(frame);
                        continue;
                    }
                    self.pending = Some(frame);
                    break;
                }
                DecodedMediaPacket::Audio { packet, samples } => {
                    decoded_audio_this_tick = decoded_audio_this_tick
                        .checked_add(1)
                        .filter(|count| *count <= self.config.max_audio_packets)
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_AUDIO_BUDGET",
                                "incremental audio decode exceeded the profile-bound packet budget",
                            )
                        })?;
                    if !self.config.has_audio {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_TRACK",
                            "incremental decoder emitted audio for a video-only contract",
                        ));
                    }
                    if !self.accept_generation(packet.generation) {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_GENERATION",
                            "incremental audio packet belongs to a different decoder generation",
                        ));
                    }
                    if packet.generation == 0
                        || packet.sequence == 0
                        || packet.sequence <= self.last_audio_sequence
                        || packet.duration_us == 0
                        || packet.pts_us >= self.config.duration_us
                        || packet.sample_rate == 0
                        || packet.channels == 0
                        || packet.channels > 8
                        || packet.frame_count == 0
                        || !super::validation::valid_audio_packet_duration(&packet)
                        || !super::validation::safe_resource_id(&packet.resource_id)
                        || packet
                            .pts_us
                            .checked_add(packet.duration_us)
                            .is_none_or(|end| end > self.config.duration_us)
                        || samples.len() > self.limits.max_audio_samples
                        || self
                            .last_audio_pts_us
                            .is_some_and(|previous| packet.pts_us < previous)
                    {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_AUDIO",
                            "incremental audio packet violates timing or payload limits",
                        ));
                    }
                    let decoded = PlayerDecodedAudio::from_i16(
                        packet.sample_rate,
                        packet.channels,
                        samples,
                        self.limits.max_audio_samples,
                    )
                    .map_err(|error| playback_error(error.code, error.to_string()))?;
                    if self.audio.len() >= self.config.max_audio_packets {
                        return Err(playback_error(
                            "ASTRA_MEDIA_INCREMENTAL_AUDIO_BUDGET",
                            "pending incremental audio packet count exceeds its profile bound",
                        ));
                    }
                    self.pending_audio_samples = self
                        .pending_audio_samples
                        .checked_add(decoded.samples.len())
                        .filter(|samples| *samples <= self.limits.max_pending_audio_samples)
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_AUDIO_BUDGET",
                                "pending incremental audio exceeds its bound",
                            )
                        })?;
                    self.last_audio_pts_us = Some(packet.pts_us);
                    self.last_audio_sequence = packet.sequence;
                    self.telemetry.audio_packets =
                        self.telemetry.audio_packets.checked_add(1).ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_TELEMETRY",
                                "incremental audio packet telemetry overflowed",
                            )
                        })?;
                    self.telemetry.decoded_audio_samples = self
                        .telemetry
                        .decoded_audio_samples
                        .checked_add(u64::try_from(decoded.samples.len()).map_err(|_| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_TELEMETRY",
                                "incremental audio sample telemetry overflowed",
                            )
                        })?)
                        .ok_or_else(|| {
                            playback_error(
                                "ASTRA_MEDIA_INCREMENTAL_TELEMETRY",
                                "incremental decoded audio telemetry overflowed",
                            )
                        })?;
                    self.audio.push_back(IncrementalAudioChunk {
                        pts_us: packet.pts_us,
                        duration_us: packet.duration_us,
                        sample_rate: decoded.sample_rate,
                        channels: decoded.channels,
                        samples: decoded.samples,
                    });
                }
            }
        }
        Ok(previous_sequence != self.current.as_ref().map(|frame| frame.sequence))
    }

    pub fn seek(&mut self, position_us: u64) -> Result<u64, MediaError> {
        if position_us > self.config.duration_us {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_SEEK",
                "incremental seek target exceeds the playback duration",
            ));
        }
        let generation = self.decoder.seek(position_us)?;
        if generation == 0
            || self
                .active_generation
                .is_some_and(|previous| generation <= previous)
        {
            return Err(playback_error(
                "ASTRA_MEDIA_INCREMENTAL_GENERATION",
                "incremental decoder returned an invalid seek generation",
            ));
        }
        self.current = None;
        self.pending = None;
        self.audio.clear();
        self.pending_audio_samples = 0;
        self.last_video_pts_us = None;
        self.last_audio_pts_us = None;
        self.last_elapsed_us = position_us;
        self.last_frame_sequence = 0;
        self.last_audio_sequence = 0;
        self.active_generation = Some(generation);
        self.telemetry = IncrementalPlaybackTelemetry::default();
        self.ended = false;
        Ok(generation)
    }

    pub fn cancel(&mut self) -> Result<(), MediaError> {
        self.decoder.cancel()?;
        self.audio.clear();
        self.pending = None;
        self.current = None;
        self.ended = true;
        self.pending_audio_samples = 0;
        Ok(())
    }

    fn accept_generation(&mut self, generation: u64) -> bool {
        if generation == 0 {
            return false;
        }
        match self.active_generation {
            Some(expected) => expected == generation,
            None => {
                self.active_generation = Some(generation);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{
        DecodedMediaPacket, IncrementalMediaDecoder, IncrementalMediaPlayback,
        IncrementalPlaybackLimits, IncrementalPlaybackTelemetry,
    };
    use crate::{
        AudioFramePacket, LateVideoPolicy, MediaError, MediaPlaybackConfig, VideoFramePacket,
    };

    struct FixtureDecoder {
        config: MediaPlaybackConfig,
        packets: VecDeque<DecodedMediaPacket>,
        generation: u64,
        cancelled: bool,
    }

    impl IncrementalMediaDecoder for FixtureDecoder {
        fn provider_id(&self) -> &'static str {
            "astra.test.incremental"
        }

        fn playback_config(&self) -> MediaPlaybackConfig {
            self.config.clone()
        }

        fn read_next(&mut self) -> Result<Option<DecodedMediaPacket>, MediaError> {
            if self.cancelled {
                return Err(MediaError::message("ASTRA_TEST_DECODER_CANCELLED"));
            }
            Ok(self.packets.pop_front())
        }

        fn seek(&mut self, _position_us: u64) -> Result<u64, MediaError> {
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or_else(|| MediaError::message("ASTRA_TEST_DECODER_GENERATION"))?;
            Ok(self.generation)
        }

        fn cancel(&mut self) -> Result<(), MediaError> {
            self.cancelled = true;
            Ok(())
        }
    }

    fn config() -> MediaPlaybackConfig {
        MediaPlaybackConfig {
            has_audio: true,
            has_video: true,
            duration_us: 100,
            max_video_frames: 8,
            max_audio_packets: 8,
            max_tick_us: 100,
            max_audio_clock_jump_us: 100,
            max_video_lead_us: 0,
            max_video_lag_us: 100,
            late_video_policy: LateVideoPolicy::Block,
        }
    }

    fn video(sequence: u64, pts_us: u64) -> DecodedMediaPacket {
        DecodedMediaPacket::Video {
            packet: VideoFramePacket {
                generation: 1,
                sequence,
                resource_id: format!("fixture.video.{sequence}"),
                pts_us,
                duration_us: 50,
                width: 1,
                height: 1,
            },
            bgra8: vec![0, 1, 2, 3].into(),
        }
    }

    fn audio(sequence: u64, pts_us: u64) -> DecodedMediaPacket {
        DecodedMediaPacket::Audio {
            packet: AudioFramePacket {
                generation: 1,
                sequence,
                resource_id: format!("fixture.audio.{sequence}"),
                pts_us,
                duration_us: 50,
                sample_rate: 40_000,
                channels: 1,
                frame_count: 2,
            },
            samples: vec![-32_768, 32_767],
        }
    }

    fn open(packets: Vec<DecodedMediaPacket>) -> IncrementalMediaPlayback {
        open_with_config(config(), packets)
    }

    fn open_with_config(
        config: MediaPlaybackConfig,
        packets: Vec<DecodedMediaPacket>,
    ) -> IncrementalMediaPlayback {
        IncrementalMediaPlayback::open(
            Box::new(FixtureDecoder {
                config,
                packets: packets.into(),
                generation: 1,
                cancelled: false,
            }),
            IncrementalPlaybackLimits {
                max_video_frame_bytes: 16,
                max_audio_samples: 8,
                max_pending_audio_samples: 8,
            },
        )
        .expect("fixture decoder contract is valid")
    }

    #[test]
    fn cursor_keeps_future_frame_and_drains_audio_incrementally() {
        let mut playback = open(vec![video(1, 0), audio(1, 0), video(2, 50)]);

        assert!(playback.advance(0).expect("first advance"));
        assert_eq!(
            playback.current_frame().map(|frame| frame.sequence),
            Some(1)
        );
        assert_eq!(playback.drain_audio().len(), 1);
        assert_eq!(playback.telemetry().decoded_frames, 2);

        assert!(!playback.advance(25).expect("future frame remains pending"));
        assert_eq!(
            playback.current_frame().map(|frame| frame.sequence),
            Some(1)
        );
        assert!(playback.advance(50).expect("second frame becomes current"));
        assert_eq!(
            playback.current_frame().map(|frame| frame.sequence),
            Some(2)
        );
    }

    #[test]
    fn cursor_rejects_backwards_clock_and_clears_generation_state_on_seek() {
        let mut playback = open(vec![video(1, 0)]);
        playback.advance(10).expect("first advance");
        let error = playback.advance(9).expect_err("clock rewind must block");
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_CLOCK");
        assert_eq!(playback.seek(50).expect("seek generation"), 2);
        assert!(playback.current_frame().is_none());
        assert_eq!(playback.telemetry().decoded_frames, 0);
    }

    #[test]
    fn cursor_rejects_zero_limits() {
        let decoder = Box::new(FixtureDecoder {
            config: config(),
            packets: VecDeque::new(),
            generation: 1,
            cancelled: false,
        });
        let result = IncrementalMediaPlayback::open(
            decoder,
            IncrementalPlaybackLimits {
                max_video_frame_bytes: 0,
                max_audio_samples: 1,
                max_pending_audio_samples: 1,
            },
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("zero limit must block"),
        };
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_LIMITS");
    }

    #[test]
    fn cursor_enforces_tick_and_packet_budgets() {
        let mut playback = open(vec![audio(1, 0), audio(2, 50)]);
        let error = playback
            .advance(101)
            .expect_err("an oversized first tick must block");
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_TICK");

        let mut bounded_config = config();
        bounded_config.max_audio_packets = 1;
        let mut playback = open_with_config(bounded_config, vec![audio(1, 0), audio(2, 50)]);
        let error = playback
            .advance(0)
            .expect_err("one tick cannot decode more than the audio packet budget");
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_AUDIO_BUDGET");
    }

    #[test]
    fn cursor_rejects_track_mismatch_and_invalid_contract() {
        let mut audio_disabled = config();
        audio_disabled.has_audio = false;
        let mut playback = open_with_config(audio_disabled, vec![audio(1, 0)]);
        let error = playback
            .advance(0)
            .expect_err("audio emitted for a video-only contract must block");
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_TRACK");

        let mut invalid = config();
        invalid.max_video_lag_us = 0;
        let result = IncrementalMediaPlayback::open(
            Box::new(FixtureDecoder {
                config: invalid,
                packets: VecDeque::new(),
                generation: 1,
                cancelled: false,
            }),
            IncrementalPlaybackLimits::default(),
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("invalid playback contract must block at open"),
        };
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_CONFIG");
    }

    #[test]
    fn cursor_applies_explicit_late_video_policy() {
        let mut blocking = config();
        blocking.max_video_lag_us = 1;
        let mut playback = open_with_config(blocking, vec![video(1, 0)]);
        let error = playback
            .advance(100)
            .expect_err("late video must block under the block policy");
        assert_diagnostic(&error, "ASTRA_MEDIA_INCREMENTAL_AV_SYNC_LATE");
        assert_eq!(
            playback.telemetry(),
            IncrementalPlaybackTelemetry::default()
        );
        assert!(playback.current_frame().is_none());

        let mut dropping = config();
        dropping.max_video_lag_us = 1;
        dropping.late_video_policy = LateVideoPolicy::Drop;
        let mut playback = open_with_config(dropping, vec![video(1, 0)]);
        assert!(!playback.advance(100).expect("drop policy should continue"));
        assert_eq!(playback.telemetry().dropped_video_packets, 1);
        assert!(playback.current_frame().is_none());
    }

    fn assert_diagnostic(error: &MediaError, expected_code: &str) {
        let MediaError::Diagnostics(diagnostics) = error else {
            panic!("expected a structured diagnostic, got {error:?}");
        };
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == expected_code));
    }
}
