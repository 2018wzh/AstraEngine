use super::{
    host_error, invalid,
    pipeline::{MediaPipeline, MAX_FRAME_BYTES},
};
use astra_platform::{DecodeOutput, PlatformError};
use gstreamer::{prelude::*, ClockTime};
use gstreamer_video::prelude::*;

pub(super) struct VideoStream {
    pipeline: Option<MediaPipeline>,
    pending: Option<gstreamer::Sample>,
    duration_us: u64,
    frame_count: u64,
    decoded_bytes: u64,
    previous_pts: Option<u64>,
    ended: bool,
}

impl VideoStream {
    pub fn open(
        bytes: &[u8],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Self, DecodeOutput), PlatformError> {
        let pipeline = MediaPipeline::open(
            bytes,
            "videoconvert ! video/x-raw,format=BGRA,interlace-mode=progressive",
            cancelled,
        )?;
        let pending = pipeline
            .pull(cancelled)?
            .ok_or_else(|| host_error("decode.stream.start", "video stream produced no frames"))?;
        let duration_us = pipeline
            .pipeline
            .query_duration::<ClockTime>()
            .map(|duration| duration.useconds())
            .filter(|duration| *duration > 0)
            .ok_or_else(|| host_error("decode.stream.start", "video duration is unavailable"))?;
        // Validate format, dimensions and timestamps before accepting Start.
        inspect(&pending, duration_us)?;
        Ok((
            Self {
                pipeline: Some(pipeline),
                pending: Some(pending),
                duration_us,
                frame_count: 0,
                decoded_bytes: 0,
                previous_pts: None,
                ended: false,
            },
            DecodeOutput::VideoStreamStart {
                duration_us: Some(duration_us),
                frame_count: None,
                decoded_byte_count: None,
            },
        ))
    }

    pub fn close(mut self) -> Result<(), PlatformError> {
        self.close_pipeline()
    }

    fn close_pipeline(&mut self) -> Result<(), PlatformError> {
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.close()?;
        }
        Ok(())
    }

    pub fn next(&mut self, cancelled: &dyn Fn() -> bool) -> Result<DecodeOutput, PlatformError> {
        if self.ended {
            return Err(invalid("video stream end was already emitted"));
        }
        let Some(sample) = self.pending.take() else {
            self.close_pipeline()?;
            self.ended = true;
            return Ok(DecodeOutput::VideoStreamEnd {
                frame_count: self.frame_count,
                decoded_byte_count: self.decoded_bytes,
            });
        };
        let (info, pts_us) = inspect(&sample, self.duration_us)?;
        if self.previous_pts.is_some_and(|previous| pts_us <= previous) {
            return Err(host_error(
                "decode.stream.next",
                "video timestamps are not increasing",
            ));
        }
        let pipeline = self
            .pipeline
            .as_ref()
            .ok_or_else(|| invalid("video stream is closed"))?;
        // One lookahead sample derives variable frame durations from actual PTS.
        let following = pipeline.pull(cancelled)?;
        let end_us = if let Some(next) = following.as_ref() {
            inspect(next, self.duration_us)?.1
        } else {
            let buffer = sample
                .buffer()
                .ok_or_else(|| invalid("video sample has no buffer"))?;
            let duration = buffer
                .duration()
                .map(|duration| duration.useconds())
                .filter(|duration| *duration > 0)
                .ok_or_else(|| {
                    host_error("decode.stream.next", "final video frame has no duration")
                })?;
            let end = pts_us
                .checked_add(duration)
                .ok_or_else(|| invalid("video time overflowed"))?;
            // Microsecond conversion may truncate both independent timestamps.
            if end.abs_diff(self.duration_us) > 1 {
                return Err(host_error(
                    "decode.stream.next",
                    "video ended before or after its declared duration",
                ));
            }
            self.duration_us
        };
        let duration_us = end_us
            .checked_sub(pts_us)
            .filter(|duration| *duration > 0)
            .ok_or_else(|| host_error("decode.stream.next", "video frame duration is invalid"))?;
        let buffer = sample
            .buffer()
            .ok_or_else(|| invalid("video sample has no buffer"))?;
        let frame = gstreamer_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
            .map_err(|_| host_error("decode.stream.next", "video frame is not CPU-readable"))?;
        let row_bytes = info.width() as usize * 4;
        let stride = usize::try_from(frame.plane_stride()[0]).map_err(|_| {
            host_error("decode.stream.next", "negative video stride is unsupported")
        })?;
        let plane = frame
            .plane_data(0)
            .map_err(|_| invalid("video frame has no plane"))?;
        if stride < row_bytes {
            return Err(invalid("video stride is smaller than a row"));
        }
        let mut bytes = Vec::with_capacity(row_bytes * info.height() as usize);
        for row in 0..info.height() as usize {
            let offset = row
                .checked_mul(stride)
                .ok_or_else(|| invalid("video row offset overflowed"))?;
            let end = offset
                .checked_add(row_bytes)
                .ok_or_else(|| invalid("video row end overflowed"))?;
            let pixels = plane
                .get(offset..end)
                .ok_or_else(|| invalid("video plane is truncated"))?;
            bytes.extend_from_slice(pixels);
        }
        self.frame_count = self
            .frame_count
            .checked_add(1)
            .ok_or_else(|| invalid("video frame count overflowed"))?;
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("video byte count overflowed"))?;
        self.previous_pts = Some(pts_us);
        self.pending = following;
        if self.pending.is_none() {
            self.close_pipeline()?;
        }
        Ok(DecodeOutput::VideoFrame {
            sequence: self.frame_count,
            pts_us,
            duration_us,
            width: info.width(),
            height: info.height(),
            bgra8: bytes.into(),
        })
    }
}

fn inspect(
    sample: &gstreamer::Sample,
    duration_us: u64,
) -> Result<(gstreamer_video::VideoInfo, u64), PlatformError> {
    let caps = sample
        .caps()
        .ok_or_else(|| invalid("video sample has no caps"))?;
    let info = gstreamer_video::VideoInfo::from_caps(caps)
        .map_err(|_| invalid("video sample caps are invalid"))?;
    if info.format() != gstreamer_video::VideoFormat::Bgra
        || info.is_interlaced()
        || info.width() == 0
        || info.height() == 0
        || u64::from(info.width())
            .checked_mul(u64::from(info.height()))
            .and_then(|pixels| pixels.checked_mul(4))
            .is_none_or(|bytes| bytes > MAX_FRAME_BYTES as u64)
    {
        return Err(host_error(
            "decode.stream.next",
            "video frame exceeds the Linux format or frame budget",
        ));
    }
    let buffer = sample
        .buffer()
        .ok_or_else(|| invalid("video sample has no buffer"))?;
    let pts = buffer
        .pts()
        .ok_or_else(|| invalid("video sample has no PTS"))?;
    let segment = sample
        .segment()
        .and_then(|segment| segment.downcast_ref::<ClockTime>())
        .ok_or_else(|| invalid("video sample has no time segment"))?;
    let pts_us = segment
        .to_running_time(pts)
        .map(|time| time.useconds())
        .filter(|pts| *pts < duration_us)
        .ok_or_else(|| invalid("video PTS is outside stream duration"))?;
    Ok((info, pts_us))
}

#[cfg(test)]
impl VideoStream {
    pub(super) fn test_pipeline(&self) -> gstreamer::Pipeline {
        self.pipeline.as_ref().unwrap().pipeline.clone()
    }
    pub(super) fn test_input_path(&self) -> std::path::PathBuf {
        self.pipeline.as_ref().unwrap()._input.path().to_path_buf()
    }
}
