use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{playback::playback_error, MediaError};

pub const DECODED_VIDEO_STREAM_SCHEMA: &str = "astra.decoded_video_stream.v1";
pub const DECODED_VIDEO_STREAM_DESCRIPTOR_SCHEMA: &str = "astra.decoded_video_stream_descriptor.v2";
pub const DECODED_VIDEO_FRAME_SCHEMA: &str = "astra.decoded_video_frame.v2";
/// CPU buffer format used by a streaming platform decoder when the frame
/// metadata can travel out-of-band.  The payload is the decoder-owned BGRA8
/// allocation; unlike the postcard frame format it is never nested in a
/// second serialization envelope.
pub const DECODED_VIDEO_FRAME_CPU_BUFFER_SCHEMA: &str = "astra.decoded_video_frame_cpu.v1";
pub const DECODED_VIDEO_STREAM_END_SCHEMA: &str = "astra.decoded_video_stream_end.v2";
/// Descriptor used by platform decoders whose output is produced lazily.
/// Unlike the complete-stream descriptor above it intentionally carries only
/// immutable source/shape identity; totals are authenticated by the end
/// marker after the cursor reaches EOS.
pub const DECODED_VIDEO_STREAM_CURSOR_SCHEMA: &str = "astra.decoded_video_stream_cursor.v1";
pub const DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA: &str = "astra.decoded_video_stream_cursor_end.v1";

pub fn is_decoded_video_cpu_buffer_format(format: &str) -> bool {
    format
        .strip_prefix(DECODED_VIDEO_FRAME_CPU_BUFFER_SCHEMA)
        .is_some_and(|suffix| suffix.starts_with(':'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedVideoStream {
    pub schema: String,
    pub duration_us: u64,
    pub frames: Vec<DecodedVideoFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedVideoFrame {
    pub sequence: u64,
    pub pts_us: u64,
    pub duration_us: u64,
    pub width: u32,
    pub height: u32,
    pub bgra8: Vec<u8>,
    pub content_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedVideoStreamDescriptor {
    pub schema: String,
    pub duration_us: u64,
    pub frame_count: u64,
    pub decoded_byte_count: u64,
    pub stream_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedVideoStreamEnd {
    pub schema: String,
    pub frame_count: u64,
    pub decoded_byte_count: u64,
    pub stream_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedVideoStreamCursor {
    pub schema: String,
    pub source_hash: Hash256,
    pub width: u32,
    pub height: u32,
    pub max_frames: u64,
    pub max_decoded_byte_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedVideoStreamCursorEnd {
    pub schema: String,
    pub source_hash: Hash256,
    pub frame_count: u64,
    pub decoded_byte_count: u64,
}

impl DecodedVideoStreamCursor {
    pub fn validate(&self) -> Result<(), MediaError> {
        if self.schema != DECODED_VIDEO_STREAM_CURSOR_SCHEMA
            || self.source_hash.as_bytes().iter().all(|byte| *byte == 0)
            || self.width == 0
            || self.height == 0
            || self.max_frames == 0
            || self.max_decoded_byte_count == 0
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_CURSOR",
                "lazy decoded video stream cursor is invalid",
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, MediaError> {
        self.validate()?;
        postcard::to_allocvec(self).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_CURSOR_ENCODE",
                format!("lazy decoded video cursor could not be encoded: {error}"),
            )
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, MediaError> {
        let cursor: Self = postcard::from_bytes(bytes).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_CURSOR_DECODE",
                format!("lazy decoded video cursor could not be decoded: {error}"),
            )
        })?;
        cursor.validate()?;
        Ok(cursor)
    }
}

impl DecodedVideoStreamCursorEnd {
    pub fn validate_against(&self, cursor: &DecodedVideoStreamCursor) -> Result<(), MediaError> {
        if self.schema != DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA
            || self.source_hash != cursor.source_hash
            || self.frame_count == 0
            || self.frame_count > cursor.max_frames
            || self.decoded_byte_count == 0
            || self.decoded_byte_count > cursor.max_decoded_byte_count
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_CURSOR_END",
                "lazy decoded video stream end marker is invalid",
            ));
        }
        Ok(())
    }

    pub fn encode(&self, cursor: &DecodedVideoStreamCursor) -> Result<Vec<u8>, MediaError> {
        self.validate_against(cursor)?;
        postcard::to_allocvec(self).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_CURSOR_END_ENCODE",
                format!("lazy decoded video end marker could not be encoded: {error}"),
            )
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, MediaError> {
        postcard::from_bytes(bytes).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_CURSOR_END_DECODE",
                format!("lazy decoded video end marker could not be decoded: {error}"),
            )
        })
    }
}

impl DecodedVideoStreamDescriptor {
    pub fn validate(&self, max_frames: u64, max_bytes: u64) -> Result<(), MediaError> {
        if self.schema != DECODED_VIDEO_STREAM_DESCRIPTOR_SCHEMA
            || self.duration_us == 0
            || self.frame_count == 0
            || self.frame_count > max_frames
            || self.decoded_byte_count == 0
            || self.decoded_byte_count > max_bytes
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_DESCRIPTOR",
                "decoded video stream descriptor exceeds its profile contract",
            ));
        }
        Ok(())
    }

    pub fn encode(&self, max_frames: u64, max_bytes: u64) -> Result<Vec<u8>, MediaError> {
        self.validate(max_frames, max_bytes)?;
        postcard::to_allocvec(self).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_DESCRIPTOR_ENCODE",
                format!("decoded video stream descriptor could not be encoded: {error}"),
            )
        })
    }

    pub fn decode(bytes: &[u8], max_frames: u64, max_bytes: u64) -> Result<Self, MediaError> {
        let descriptor: Self = postcard::from_bytes(bytes).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_DESCRIPTOR_DECODE",
                format!("decoded video stream descriptor could not be decoded: {error}"),
            )
        })?;
        descriptor.validate(max_frames, max_bytes)?;
        Ok(descriptor)
    }
}

impl DecodedVideoFrame {
    /// Returns the stable, metadata-only format descriptor for a raw BGRA8
    /// frame.  The pixel allocation is deliberately not part of this string.
    pub fn cpu_buffer_format(&self) -> String {
        Self::cpu_buffer_format_from_parts(
            self.sequence,
            self.pts_us,
            self.duration_us,
            self.width,
            self.height,
        )
    }

    fn cpu_buffer_format_from_parts(
        sequence: u64,
        pts_us: u64,
        duration_us: u64,
        width: u32,
        height: u32,
    ) -> String {
        format!(
            "{DECODED_VIDEO_FRAME_CPU_BUFFER_SCHEMA}:{sequence}:{pts_us}:{duration_us}:{width}:{height}"
        )
    }

    /// Rebuilds a frame by consuming the platform-owned BGRA8 buffer.  Only
    /// the small metadata descriptor is parsed; `bytes` is moved directly
    /// into the resulting frame and is never cloned or postcard-decoded.
    pub fn from_cpu_buffer(
        format: &str,
        bytes: Vec<u8>,
        hash: &str,
        max_bytes: u64,
    ) -> Result<Self, MediaError> {
        if bytes.is_empty() || bytes.len() as u64 > max_bytes {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_BUDGET",
                "raw decoded video frame exceeds its profile-bound byte budget",
            ));
        }
        let metadata = parse_cpu_buffer_format(format)?;
        let content_hash: Hash256 = hash.parse().map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_HASH",
                format!("raw decoded video frame hash is invalid: {error}"),
            )
        })?;
        if Hash256::from_sha256(&bytes) != content_hash {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_HASH",
                "raw decoded video frame hash does not match its payload",
            ));
        }
        let frame = Self {
            sequence: metadata.sequence,
            pts_us: metadata.pts_us,
            duration_us: metadata.duration_us,
            width: metadata.width,
            height: metadata.height,
            bgra8: bytes,
            content_hash,
        };
        frame.validate()?;
        Ok(frame)
    }

    pub fn validate(&self) -> Result<(), MediaError> {
        let expected = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(4));
        if self.sequence == 0
            || self.duration_us == 0
            || self.width == 0
            || self.height == 0
            || expected != Some(self.bgra8.len() as u64)
            || Hash256::from_sha256(&self.bgra8) != self.content_hash
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_FRAME",
                "decoded video frame order, size, duration, or hash is invalid",
            ));
        }
        Ok(())
    }

    pub fn encode(&self, max_bytes: u64) -> Result<Vec<u8>, MediaError> {
        self.validate()?;
        let encoded = postcard::to_allocvec(self).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_ENCODE",
                format!("decoded video frame could not be encoded: {error}"),
            )
        })?;
        if encoded.len() as u64 > max_bytes {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_BUDGET",
                "decoded video frame exceeds its profile-bound byte budget",
            ));
        }
        Ok(encoded)
    }

    pub fn decode(bytes: &[u8], max_bytes: u64) -> Result<Self, MediaError> {
        if bytes.is_empty() || bytes.len() as u64 > max_bytes {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_BUDGET",
                "decoded video frame exceeds its profile-bound byte budget",
            ));
        }
        let frame: Self = postcard::from_bytes(bytes).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_FRAME_DECODE",
                format!("decoded video frame could not be decoded: {error}"),
            )
        })?;
        frame.validate()?;
        Ok(frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuBufferMetadata {
    sequence: u64,
    pts_us: u64,
    duration_us: u64,
    width: u32,
    height: u32,
}

fn parse_cpu_buffer_format(format: &str) -> Result<CpuBufferMetadata, MediaError> {
    let mut fields = format.split(':');
    if fields.next() != Some(DECODED_VIDEO_FRAME_CPU_BUFFER_SCHEMA) {
        return Err(playback_error(
            "ASTRA_DECODED_VIDEO_FRAME_FORMAT",
            "raw decoded video frame format schema is invalid",
        ));
    }
    let parse = |field: Option<&str>, name: &str| {
        field
            .ok_or_else(|| {
                playback_error(
                    "ASTRA_DECODED_VIDEO_FRAME_FORMAT",
                    format!("raw decoded video frame format is missing {name}"),
                )
            })?
            .parse::<u64>()
            .map_err(|error| {
                playback_error(
                    "ASTRA_DECODED_VIDEO_FRAME_FORMAT",
                    format!("raw decoded video frame {name} is invalid: {error}"),
                )
            })
    };
    let sequence = parse(fields.next(), "sequence")?;
    let pts_us = parse(fields.next(), "pts_us")?;
    let duration_us = parse(fields.next(), "duration_us")?;
    let width = u32::try_from(parse(fields.next(), "width")?).map_err(|_| {
        playback_error(
            "ASTRA_DECODED_VIDEO_FRAME_FORMAT",
            "raw decoded video frame width exceeds u32",
        )
    })?;
    let height = u32::try_from(parse(fields.next(), "height")?).map_err(|_| {
        playback_error(
            "ASTRA_DECODED_VIDEO_FRAME_FORMAT",
            "raw decoded video frame height exceeds u32",
        )
    })?;
    if fields.next().is_some() {
        return Err(playback_error(
            "ASTRA_DECODED_VIDEO_FRAME_FORMAT",
            "raw decoded video frame format has trailing fields",
        ));
    }
    Ok(CpuBufferMetadata {
        sequence,
        pts_us,
        duration_us,
        width,
        height,
    })
}

impl DecodedVideoStreamEnd {
    pub fn validate_against(
        &self,
        descriptor: &DecodedVideoStreamDescriptor,
    ) -> Result<(), MediaError> {
        if self.schema != DECODED_VIDEO_STREAM_END_SCHEMA
            || self.frame_count != descriptor.frame_count
            || self.decoded_byte_count != descriptor.decoded_byte_count
            || self.stream_hash != descriptor.stream_hash
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_STREAM_END",
                "decoded video stream end marker does not match its descriptor",
            ));
        }
        Ok(())
    }
}

impl DecodedVideoStream {
    pub fn validate(&self, max_frames: u64, max_bytes: u64) -> Result<(), MediaError> {
        if self.schema != DECODED_VIDEO_STREAM_SCHEMA
            || self.duration_us == 0
            || self.frames.is_empty()
            || self.frames.len() as u64 > max_frames
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_STREAM",
                "decoded video stream schema, duration, or frame count is invalid",
            ));
        }
        let mut total_bytes = 0_u64;
        let mut previous_sequence = 0_u64;
        let mut previous_pts = None;
        for frame in &self.frames {
            let expected = u64::from(frame.width)
                .checked_mul(u64::from(frame.height))
                .and_then(|pixels| pixels.checked_mul(4));
            total_bytes = total_bytes
                .checked_add(frame.bgra8.len() as u64)
                .ok_or_else(|| {
                    playback_error(
                        "ASTRA_DECODED_VIDEO_BUDGET",
                        "decoded video byte accounting overflowed",
                    )
                })?;
            if frame.sequence != previous_sequence + 1
                || frame.duration_us == 0
                || frame.width == 0
                || frame.height == 0
                || expected != Some(frame.bgra8.len() as u64)
                || frame.pts_us >= self.duration_us
                || frame
                    .pts_us
                    .checked_add(frame.duration_us)
                    .is_none_or(|end| end > self.duration_us)
                || previous_pts.is_some_and(|pts| frame.pts_us < pts)
                || Hash256::from_sha256(&frame.bgra8) != frame.content_hash
            {
                return Err(playback_error(
                    "ASTRA_DECODED_VIDEO_FRAME",
                    "decoded video frame order, bounds, size, or hash is invalid",
                ));
            }
            previous_sequence = frame.sequence;
            previous_pts = Some(frame.pts_us);
        }
        if total_bytes > max_bytes {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_BUDGET",
                "decoded video stream exceeds its profile-bound byte budget",
            ));
        }
        Ok(())
    }

    pub fn encode(&self, max_frames: u64, max_bytes: u64) -> Result<Vec<u8>, MediaError> {
        self.validate(max_frames, max_bytes)?;
        let encoded = postcard::to_allocvec(self).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_ENCODE",
                format!("decoded video stream could not be encoded: {error}"),
            )
        })?;
        if encoded.len() as u64 > max_bytes {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_BUDGET",
                "encoded decoded-video payload exceeds its profile-bound byte budget",
            ));
        }
        Ok(encoded)
    }

    pub fn decode(bytes: &[u8], max_frames: u64, max_bytes: u64) -> Result<Self, MediaError> {
        if bytes.is_empty() || bytes.len() as u64 > max_bytes {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_BUDGET",
                "decoded video payload exceeds its profile-bound byte budget",
            ));
        }
        let stream: Self = postcard::from_bytes(bytes).map_err(|error| {
            playback_error(
                "ASTRA_DECODED_VIDEO_DECODE",
                format!("decoded video stream could not be decoded: {error}"),
            )
        })?;
        stream.validate(max_frames, max_bytes)?;
        Ok(stream)
    }
}
