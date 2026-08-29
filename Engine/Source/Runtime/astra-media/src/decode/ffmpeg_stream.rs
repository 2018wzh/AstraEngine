use std::collections::VecDeque;
use std::io::{Cursor, Read, Seek};

use ffmpeg_next as ffmpeg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{decode_error, MediaError};
use crate::{
    DecodedMediaPacket, IncrementalDecodeBudget, IncrementalDecodeCapability,
    IncrementalDecodeProvider, IncrementalDecodeProviderRegistry, IncrementalDecodeRequest,
    IncrementalMediaDecoder, LateVideoPolicy, MediaPlaybackConfig,
};

mod backend;
mod custom_io;
use backend::*;
use custom_io::{open_seekable_input, SeekableInputOwner};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FfmpegStreamLimits {
    pub max_encoded_bytes: usize,
    pub max_audio_packet_bytes: usize,
    pub max_video_frame_bytes: usize,
    pub max_pending_packets: usize,
    pub max_video_frames: usize,
    pub max_audio_packets: usize,
    pub max_tick_us: u64,
    pub max_audio_clock_jump_us: u64,
    pub max_video_lead_us: u64,
    pub max_video_lag_us: u64,
    pub late_video_policy: LateVideoPolicy,
}

impl Default for FfmpegStreamLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 256 * 1024 * 1024,
            max_audio_packet_bytes: 4 * 1024 * 1024,
            max_video_frame_bytes: 64 * 1024 * 1024,
            max_pending_packets: 64,
            max_video_frames: 64,
            max_audio_packets: 64,
            max_tick_us: 100_000,
            max_audio_clock_jump_us: 250_000,
            max_video_lead_us: 20_000,
            max_video_lag_us: 100_000,
            late_video_policy: LateVideoPolicy::Block,
        }
    }
}

pub type FfmpegDecodedPacket = crate::DecodedMediaPacket;

pub const FFMPEG_INCREMENTAL_PROVIDER_ID: &str = "astra.decode.ffmpeg.incremental";

/// AstraMedia registry binding for the mature FFmpeg incremental backend.
/// The provider is intentionally feature-gated and must be registered by the
/// selected family/host composition; it is never an implicit fallback.
pub struct FfmpegIncrementalDecodeProvider;

impl FfmpegIncrementalDecodeProvider {
    pub fn probe() -> Result<Self, MediaError> {
        ffmpeg::init().map_err(|error| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_PROBE",
                format!("initialize FFmpeg provider: {error}"),
            )
        })?;
        Ok(Self)
    }
}

/// Open the explicitly selected FFmpeg incremental provider for a bounded
/// reader.  This is a convenience composition helper; provider selection is
/// still fixed to [`FFMPEG_INCREMENTAL_PROVIDER_ID`] and the registry keeps
/// rejecting duplicate, missing, or ineligible providers.  Callers that need
/// a different backend continue to use [`IncrementalDecodeProviderRegistry`]
/// directly.
pub fn open_ffmpeg_incremental_reader<R>(
    codec: impl Into<String>,
    reader: R,
    budget: IncrementalDecodeBudget,
) -> Result<Box<dyn IncrementalMediaDecoder>, MediaError>
where
    R: Read + Seek + Send + 'static,
{
    let provider = FfmpegIncrementalDecodeProvider::probe()?;
    let mut registry = IncrementalDecodeProviderRegistry::default();
    registry.register(Box::new(provider))?;
    registry.open(
        FFMPEG_INCREMENTAL_PROVIDER_ID,
        IncrementalDecodeRequest::new(codec, Box::new(reader)).with_budget(budget),
    )
}

impl IncrementalDecodeProvider for FfmpegIncrementalDecodeProvider {
    fn provider_id(&self) -> &'static str {
        FFMPEG_INCREMENTAL_PROVIDER_ID
    }

    fn capability(&self) -> IncrementalDecodeCapability {
        IncrementalDecodeCapability {
            provider_id: FFMPEG_INCREMENTAL_PROVIDER_ID.to_owned(),
            codecs: vec![
                "avi".to_owned(),
                "mp4".to_owned(),
                "webm".to_owned(),
                "wav".to_owned(),
                "ogg".to_owned(),
                "flac".to_owned(),
                "mp3".to_owned(),
            ],
            feature_gated: false,
        }
    }

    fn open_reader(
        &self,
        request: IncrementalDecodeRequest,
    ) -> Result<Box<dyn IncrementalMediaDecoder>, MediaError> {
        let IncrementalDecodeRequest {
            codec,
            reader,
            budget,
        } = request;
        let limits = FfmpegStreamLimits {
            max_encoded_bytes: budget.max_encoded_bytes,
            max_video_frame_bytes: budget.max_video_frame_bytes,
            max_pending_packets: budget.max_pending_packets,
            max_video_frames: budget.max_video_frames,
            max_audio_packets: budget.max_audio_packets,
            ..FfmpegStreamLimits::default()
        };
        let decoder = FfmpegPlaybackDecoder::open_reader(&codec, reader, limits)?;
        Ok(Box::new(decoder))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FfmpegAudioOutputFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct FfmpegPlaybackDecoder {
    input: ffmpeg::format::context::Input,
    // Field order is intentional: the format context must close before the
    // custom AVIO owner releases its callbacks and reader.
    _source: SeekableInputOwner,
    audio: Option<AudioDecoder>,
    video: Option<VideoDecoder>,
    limits: FfmpegStreamLimits,
    duration_us: u64,
    generation: u64,
    next_audio_sequence: u64,
    next_video_sequence: u64,
    seek_floor_us: u64,
    pending: VecDeque<FfmpegDecodedPacket>,
    demux_eof: bool,
    cancelled: bool,
}

impl FfmpegPlaybackDecoder {
    pub fn open(codec: &str, bytes: &[u8], limits: FfmpegStreamLimits) -> Result<Self, MediaError> {
        Self::open_with_audio_output(codec, bytes, limits, None)
    }

    /// Open an encoded source through FFmpeg custom AVIO. The provider retains
    /// the seekable reader and never materialises a second plaintext copy.
    pub fn open_reader<R>(
        codec: &str,
        reader: R,
        limits: FfmpegStreamLimits,
    ) -> Result<Self, MediaError>
    where
        R: Read + Seek + Send + 'static,
    {
        Self::open_reader_with_audio_output(codec, reader, limits, None)
    }

    pub fn open_with_audio_output(
        codec: &str,
        bytes: &[u8],
        limits: FfmpegStreamLimits,
        audio_output: Option<FfmpegAudioOutputFormat>,
    ) -> Result<Self, MediaError> {
        validate_limits(&limits)?;
        if audio_output.is_some_and(|format| {
            format.sample_rate == 0 || format.channels == 0 || format.channels > 8
        }) {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_OUTPUT",
                "FFmpeg target audio format is invalid",
            ));
        }
        if !safe_codec(codec) || bytes.is_empty() || bytes.len() > limits.max_encoded_bytes {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_INPUT",
                "FFmpeg stream codec or encoded byte budget is invalid",
            ));
        }
        Self::open_reader_with_audio_output(
            codec,
            Cursor::new(bytes.to_vec()),
            limits,
            audio_output,
        )
    }

    pub fn open_reader_with_audio_output<R>(
        codec: &str,
        reader: R,
        limits: FfmpegStreamLimits,
        audio_output: Option<FfmpegAudioOutputFormat>,
    ) -> Result<Self, MediaError>
    where
        R: Read + Seek + Send + 'static,
    {
        validate_limits(&limits)?;
        if audio_output.is_some_and(|format| {
            format.sample_rate == 0 || format.channels == 0 || format.channels > 8
        }) {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_OUTPUT",
                "FFmpeg target audio format is invalid",
            ));
        }
        if !safe_codec(codec) {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_INPUT",
                "FFmpeg stream codec is not enabled by the profile",
            ));
        }
        let (input, source) = open_seekable_input(reader, limits.max_encoded_bytes)?;
        Self::open_source(input, source, limits, audio_output)
    }

    fn open_source(
        input: ffmpeg::format::context::Input,
        source: SeekableInputOwner,
        limits: FfmpegStreamLimits,
        audio_output: Option<FfmpegAudioOutputFormat>,
    ) -> Result<Self, MediaError> {
        let duration = input.duration();
        if duration <= 0 {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_DURATION",
                "encoded stream does not declare a positive duration",
            ));
        }
        let duration_us = u64::try_from(duration).map_err(|_| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_DURATION",
                "encoded stream duration exceeds the playback clock",
            )
        })?;
        let audio = create_audio_decoder(&input, audio_output)?;
        let video = create_video_decoder(&input)?;
        if audio.is_none() && video.is_none() {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_TRACKS",
                "encoded stream has no supported audio or video track",
            ));
        }
        Ok(Self {
            input,
            _source: source,
            audio,
            video,
            limits,
            duration_us,
            generation: 1,
            next_audio_sequence: 1,
            next_video_sequence: 1,
            seek_floor_us: 0,
            pending: VecDeque::new(),
            demux_eof: false,
            cancelled: false,
        })
    }

    pub fn playback_config(&self) -> MediaPlaybackConfig {
        MediaPlaybackConfig {
            has_audio: self.audio.is_some(),
            has_video: self.video.is_some(),
            duration_us: self.duration_us,
            max_video_frames: self.limits.max_video_frames,
            max_audio_packets: self.limits.max_audio_packets,
            max_tick_us: self.limits.max_tick_us,
            max_audio_clock_jump_us: self.limits.max_audio_clock_jump_us,
            max_video_lead_us: self.limits.max_video_lead_us,
            max_video_lag_us: self.limits.max_video_lag_us,
            late_video_policy: self.limits.late_video_policy,
        }
    }

    pub fn configure_audio_output(
        &mut self,
        format: FfmpegAudioOutputFormat,
    ) -> Result<(), MediaError> {
        if format.sample_rate == 0
            || format.channels == 0
            || format.channels > 8
            || self.generation != 1
            || self.next_audio_sequence != 1
            || self.next_video_sequence != 1
            || !self.pending.is_empty()
            || self.demux_eof
            || self.cancelled
        {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_OUTPUT",
                "audio output can only be configured before the first decode operation",
            ));
        }
        let audio = self.audio.as_mut().ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_OUTPUT",
                "audio output was configured for a stream without audio",
            )
        })?;
        reconfigure_audio_decoder(audio, format)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn read_next(&mut self) -> Result<Option<FfmpegDecodedPacket>, MediaError> {
        if self.cancelled {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_CANCELLED",
                "cancelled FFmpeg stream cannot decode more packets",
            ));
        }
        loop {
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
            if self.demux_eof {
                self.drain_eof()?;
                return Ok(self.pending.pop_front());
            }
            self.drain_ready()?;
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
            // Do not use `Input::packets()` here.  ffmpeg-next's iterator
            // intentionally discards every demux error except EOF, which
            // would turn a malformed or truncated source into a silent end
            // of stream.  Reading the packet directly preserves the native
            // error and lets the media boundary return a stable diagnostic.
            let mut packet = ffmpeg::Packet::empty();
            let stream_index = match packet.read(&mut self.input) {
                Ok(()) => packet.stream(),
                Err(ffmpeg::Error::Eof) => {
                    self.demux_eof = true;
                    continue;
                }
                Err(error) => {
                    return Err(ffmpeg_error(
                        "ASTRA_FFMPEG_STREAM_PACKET_READ",
                        "read encoded packet",
                        error,
                    ));
                }
            };
            if self
                .audio
                .as_ref()
                .is_some_and(|audio| audio.stream_index == stream_index)
            {
                let audio = self.audio.as_mut().ok_or_else(|| {
                    decode_error(
                        "ASTRA_FFMPEG_STREAM_STATE",
                        "audio decoder disappeared while processing its packet",
                    )
                })?;
                audio.decoder.send_packet(&packet).map_err(|error| {
                    ffmpeg_error("ASTRA_FFMPEG_STREAM_PACKET", "submit audio packet", error)
                })?;
                drain_audio(
                    audio,
                    self.generation,
                    &mut self.next_audio_sequence,
                    self.seek_floor_us,
                    self.duration_us,
                    &self.limits,
                    &mut self.pending,
                    false,
                )?;
            } else if self
                .video
                .as_ref()
                .is_some_and(|video| video.stream_index == stream_index)
            {
                let video = self.video.as_mut().ok_or_else(|| {
                    decode_error(
                        "ASTRA_FFMPEG_STREAM_STATE",
                        "video decoder disappeared while processing its packet",
                    )
                })?;
                video.decoder.send_packet(&packet).map_err(|error| {
                    ffmpeg_error("ASTRA_FFMPEG_STREAM_PACKET", "submit video packet", error)
                })?;
                drain_video(
                    video,
                    self.generation,
                    &mut self.next_video_sequence,
                    self.seek_floor_us,
                    self.duration_us,
                    &self.limits,
                    &mut self.pending,
                    false,
                )?;
            }
        }
    }

    pub fn seek(&mut self, position_us: u64) -> Result<u64, MediaError> {
        if self.cancelled || position_us > self.duration_us {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_SEEK",
                "FFmpeg seek target or decoder state is invalid",
            ));
        }
        let timestamp = i64::try_from(position_us).map_err(|_| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_SEEK",
                "FFmpeg seek target exceeds the native clock",
            )
        })?;
        self.input.seek(timestamp, ..timestamp).map_err(|error| {
            ffmpeg_error("ASTRA_FFMPEG_STREAM_SEEK", "seek encoded stream", error)
        })?;
        let generation = self.generation.checked_add(1).ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_GENERATION",
                "FFmpeg stream generation overflowed",
            )
        })?;
        if let Some(audio) = &mut self.audio {
            audio.decoder.flush();
            audio.resampler = create_resampler(
                audio.source_format,
                audio.source_layout,
                audio.source_sample_rate,
                audio.format,
                audio.layout,
                audio.sample_rate,
            )?;
            audio.eof_sent = false;
            audio.decoder_drained = false;
            audio.resampler_flushed = false;
            audio.next_output_pts_us = None;
        }
        if let Some(video) = &mut self.video {
            video.decoder.flush();
            video.eof_sent = false;
            video.decoder_drained = false;
        }
        self.generation = generation;
        self.next_audio_sequence = 1;
        self.next_video_sequence = 1;
        self.seek_floor_us = position_us;
        self.pending.clear();
        self.demux_eof = false;
        Ok(generation)
    }

    pub fn cancel(&mut self) -> Result<(), MediaError> {
        if self.cancelled {
            return Err(decode_error(
                "ASTRA_FFMPEG_STREAM_CANCELLED",
                "FFmpeg stream was already cancelled",
            ));
        }
        self.cancelled = true;
        self.pending.clear();
        Ok(())
    }

    fn drain_eof(&mut self) -> Result<(), MediaError> {
        if let Some(audio) = &mut self.audio {
            if !audio.eof_sent {
                audio.decoder.send_eof().map_err(|error| {
                    ffmpeg_error("ASTRA_FFMPEG_STREAM_EOS", "flush audio decoder", error)
                })?;
                audio.eof_sent = true;
            }
            drain_audio(
                audio,
                self.generation,
                &mut self.next_audio_sequence,
                self.seek_floor_us,
                self.duration_us,
                &self.limits,
                &mut self.pending,
                true,
            )?;
            if audio.decoder_drained {
                flush_audio_resampler(
                    audio,
                    self.generation,
                    &mut self.next_audio_sequence,
                    self.seek_floor_us,
                    self.duration_us,
                    &self.limits,
                    &mut self.pending,
                )?;
            }
        }
        if let Some(video) = &mut self.video {
            if !video.eof_sent {
                video.decoder.send_eof().map_err(|error| {
                    ffmpeg_error("ASTRA_FFMPEG_STREAM_EOS", "flush video decoder", error)
                })?;
                video.eof_sent = true;
            }
            drain_video(
                video,
                self.generation,
                &mut self.next_video_sequence,
                self.seek_floor_us,
                self.duration_us,
                &self.limits,
                &mut self.pending,
                true,
            )?;
        }
        Ok(())
    }

    fn drain_ready(&mut self) -> Result<(), MediaError> {
        if let Some(audio) = &mut self.audio {
            drain_audio(
                audio,
                self.generation,
                &mut self.next_audio_sequence,
                self.seek_floor_us,
                self.duration_us,
                &self.limits,
                &mut self.pending,
                false,
            )?;
        }
        if let Some(video) = &mut self.video {
            drain_video(
                video,
                self.generation,
                &mut self.next_video_sequence,
                self.seek_floor_us,
                self.duration_us,
                &self.limits,
                &mut self.pending,
                false,
            )?;
        }
        Ok(())
    }
}

impl crate::IncrementalMediaDecoder for FfmpegPlaybackDecoder {
    fn provider_id(&self) -> &'static str {
        FFMPEG_INCREMENTAL_PROVIDER_ID
    }

    fn playback_config(&self) -> MediaPlaybackConfig {
        Self::playback_config(self)
    }

    fn read_next(&mut self) -> Result<Option<DecodedMediaPacket>, MediaError> {
        Self::read_next(self)
    }

    fn seek(&mut self, position_us: u64) -> Result<u64, MediaError> {
        Self::seek(self, position_us)
    }

    fn cancel(&mut self) -> Result<(), MediaError> {
        Self::cancel(self)
    }
}
