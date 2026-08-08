use crate::{playback::playback_error, MediaError};

pub const DECODED_VIDEO_STREAM_SCHEMA: &str = "astra.decoded_video_stream.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedVideoStream {
    pub schema: String,
    pub duration_us: u64,
    pub frames: Vec<DecodedVideoFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedVideoFrame {
    pub sequence: u64,
    pub pts_us: u64,
    pub duration_us: u64,
    pub width: u32,
    pub height: u32,
    pub bgra8: astra_byte_source::OwnedByteBuffer,
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
        {
            return Err(playback_error(
                "ASTRA_DECODED_VIDEO_FRAME",
                "decoded video frame order, size, or duration is invalid",
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
            frame.validate()?;
            total_bytes = total_bytes
                .checked_add(frame.bgra8.len() as u64)
                .ok_or_else(|| {
                    playback_error(
                        "ASTRA_DECODED_VIDEO_BUDGET",
                        "decoded video byte accounting overflowed",
                    )
                })?;
            if frame.sequence != previous_sequence + 1
                || frame.pts_us >= self.duration_us
                || frame
                    .pts_us
                    .checked_add(frame.duration_us)
                    .is_none_or(|end| end > self.duration_us)
                || previous_pts.is_some_and(|pts| frame.pts_us < pts)
            {
                return Err(playback_error(
                    "ASTRA_DECODED_VIDEO_FRAME",
                    "decoded video frame order or timing is invalid",
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
}
