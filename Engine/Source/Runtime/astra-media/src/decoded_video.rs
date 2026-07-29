use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{playback::playback_error, MediaError};

pub const DECODED_VIDEO_STREAM_SCHEMA: &str = "astra.decoded_video_stream.v1";
pub const DECODED_VIDEO_STREAM_DESCRIPTOR_SCHEMA: &str = "astra.decoded_video_stream_descriptor.v2";
pub const DECODED_VIDEO_FRAME_SCHEMA: &str = "astra.decoded_video_frame.v2";
pub const DECODED_VIDEO_STREAM_END_SCHEMA: &str = "astra.decoded_video_stream_end.v2";

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
