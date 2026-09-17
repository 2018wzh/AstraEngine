use std::collections::VecDeque;

use ffmpeg_next as ffmpeg;

use super::{FfmpegDecodedPacket, FfmpegStreamLimits};
use crate::decode::{decode_error, MediaError};
use crate::VideoFramePacket;

pub(super) struct VideoDecoder {
    pub(super) stream_index: usize,
    pub(super) time_base: ffmpeg::Rational,
    pub(super) frame_duration_us: u64,
    pub(super) decoder: ffmpeg::decoder::Video,
    pub(super) scaler: ffmpeg::software::scaling::Context,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) eof_sent: bool,
    pub(super) decoder_drained: bool,
}

pub(super) fn create_video_decoder(
    input: &ffmpeg::format::context::Input,
) -> Result<Option<VideoDecoder>, MediaError> {
    let Some(stream) = input.streams().best(ffmpeg::media::Type::Video) else {
        return Ok(None);
    };
    let stream_index = stream.index();
    let time_base = stream.time_base();
    let rate = stream.avg_frame_rate();
    if rate.numerator() <= 0 || rate.denominator() <= 0 {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_VIDEO_RATE",
            "FFmpeg video track has no stable average frame rate",
        ));
    }
    let frame_duration_us = (1_000_000_u64)
        .checked_mul(rate.denominator() as u64)
        .map(|value| value / rate.numerator() as u64)
        .filter(|duration| *duration > 0)
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_RATE",
                "FFmpeg video frame duration overflowed",
            )
        })?;
    let context =
        ffmpeg::codec::context::Context::from_parameters(stream.parameters()).map_err(|error| {
            ffmpeg_error("ASTRA_FFMPEG_STREAM_DECODER", "create video decoder", error)
        })?;
    let decoder = context.decoder().video().map_err(|error| {
        ffmpeg_error("ASTRA_FFMPEG_STREAM_DECODER", "open video decoder", error)
    })?;
    let width = decoder.width();
    let height = decoder.height();
    if width == 0 || height == 0 {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_VIDEO_FORMAT",
            "FFmpeg video track has empty dimensions",
        ));
    }
    let scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        width,
        height,
        ffmpeg::format::Pixel::BGRA,
        width,
        height,
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_STREAM_SCALE", "create video scaler", error))?;
    Ok(Some(VideoDecoder {
        stream_index,
        time_base,
        frame_duration_us,
        decoder,
        scaler,
        width,
        height,
        eof_sent: false,
        decoder_drained: false,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn drain_video(
    video: &mut VideoDecoder,
    generation: u64,
    next_sequence: &mut u64,
    seek_floor_us: u64,
    stream_duration_us: u64,
    limits: &FfmpegStreamLimits,
    pending: &mut VecDeque<FfmpegDecodedPacket>,
    eof: bool,
) -> Result<(), MediaError> {
    loop {
        if pending.len() >= limits.max_pending_packets {
            break;
        }
        let mut decoded = ffmpeg::frame::Video::empty();
        match video.decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let pts_us = timestamp_us(decoded.timestamp(), video.time_base)?;
                pts_us.checked_add(video.frame_duration_us).ok_or_else(|| {
                    decode_error(
                        "ASTRA_FFMPEG_STREAM_TIMESTAMP",
                        "FFmpeg video frame end timestamp overflowed",
                    )
                })?;
                if pts_us < seek_floor_us || pts_us >= stream_duration_us {
                    continue;
                }
                let frame_duration_us = video.frame_duration_us.min(stream_duration_us - pts_us);
                let mut converted = ffmpeg::frame::Video::empty();
                video
                    .scaler
                    .run(&decoded, &mut converted)
                    .map_err(|error| {
                        ffmpeg_error("ASTRA_FFMPEG_STREAM_SCALE", "convert video frame", error)
                    })?;
                let bgra = copy_bgra(&converted, video.width, video.height, limits)?;
                ensure_pending_budget(pending, limits)?;
                let sequence = *next_sequence;
                *next_sequence = sequence.checked_add(1).ok_or_else(|| {
                    decode_error(
                        "ASTRA_FFMPEG_STREAM_SEQUENCE",
                        "FFmpeg video sequence overflowed",
                    )
                })?;
                pending.push_back(FfmpegDecodedPacket::Video {
                    packet: VideoFramePacket {
                        generation,
                        sequence,
                        resource_id: format!("ffmpeg.video.{generation}.{sequence}"),
                        pts_us,
                        duration_us: frame_duration_us,
                        width: video.width,
                        height: video.height,
                    },
                    bgra8: bgra.into(),
                });
            }
            Err(ffmpeg::Error::Other {
                errno: ffmpeg::error::EAGAIN,
            }) if !eof => break,
            Err(ffmpeg::Error::Eof) if eof => {
                video.decoder_drained = true;
                break;
            }
            Err(error) => {
                return Err(ffmpeg_error(
                    "ASTRA_FFMPEG_STREAM_FRAME",
                    "receive video frame",
                    error,
                ))
            }
        }
    }
    Ok(())
}

pub(super) fn copy_bgra(
    frame: &ffmpeg::frame::Video,
    width: u32,
    height: u32,
    limits: &FfmpegStreamLimits,
) -> Result<Vec<u8>, MediaError> {
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_BUDGET",
                "FFmpeg video row size overflowed",
            )
        })?;
    let frame_bytes = row_bytes
        .checked_mul(height as usize)
        .filter(|bytes| *bytes <= limits.max_video_frame_bytes)
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_BUDGET",
                "FFmpeg video frame exceeds its byte budget",
            )
        })?;
    if frame.stride(0) < row_bytes {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_VIDEO_FORMAT",
            "FFmpeg video frame stride is truncated",
        ));
    }
    let data = frame.data(0);
    let mut bgra = Vec::with_capacity(frame_bytes);
    for row in 0..height as usize {
        let start = row.checked_mul(frame.stride(0)).ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_BUDGET",
                "FFmpeg video row offset overflowed",
            )
        })?;
        let end = start.checked_add(row_bytes).ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_BUDGET",
                "FFmpeg video row end overflowed",
            )
        })?;
        bgra.extend_from_slice(data.get(start..end).ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_FORMAT",
                "FFmpeg video frame is truncated",
            )
        })?);
    }
    Ok(bgra)
}

pub(super) fn timestamp_us(
    timestamp: Option<i64>,
    time_base: ffmpeg::Rational,
) -> Result<u64, MediaError> {
    let timestamp = timestamp.ok_or_else(|| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_TIMESTAMP",
            "FFmpeg frame does not carry a presentation timestamp",
        )
    })?;
    if timestamp < 0 || time_base.numerator() <= 0 || time_base.denominator() <= 0 {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_TIMESTAMP",
            "FFmpeg frame presentation timestamp is invalid",
        ));
    }
    let value = i128::from(timestamp)
        .checked_mul(i128::from(time_base.numerator()))
        .and_then(|value| value.checked_mul(1_000_000))
        .map(|value| value / i128::from(time_base.denominator()))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_TIMESTAMP",
                "FFmpeg frame presentation timestamp overflowed",
            )
        })?;
    Ok(value)
}

pub(super) fn ensure_pending_budget(
    pending: &VecDeque<FfmpegDecodedPacket>,
    limits: &FfmpegStreamLimits,
) -> Result<(), MediaError> {
    if pending.len() >= limits.max_pending_packets {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_BACKPRESSURE",
            "FFmpeg decoded packet queue reached its profile-bound limit",
        ));
    }
    Ok(())
}

pub(super) fn validate_limits(limits: &FfmpegStreamLimits) -> Result<(), MediaError> {
    if limits.max_encoded_bytes == 0
        || limits.max_audio_packet_bytes == 0
        || limits.max_video_frame_bytes == 0
        || limits.max_pending_packets == 0
        || limits.max_video_frames == 0
        || limits.max_audio_packets == 0
        || limits.max_tick_us == 0
        || limits.max_audio_clock_jump_us == 0
        || limits.max_video_lag_us == 0
    {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_LIMITS",
            "every FFmpeg stream resource and clock limit must be non-zero",
        ));
    }
    Ok(())
}

pub(super) fn safe_codec(codec: &str) -> bool {
    matches!(codec, "mp4" | "webm" | "wav" | "ogg" | "flac" | "mp3")
}

pub(super) fn ffmpeg_error(
    code: &'static str,
    operation: &'static str,
    error: ffmpeg::Error,
) -> MediaError {
    decode_error(code, format!("FFmpeg failed to {operation}: {error}"))
}

pub(super) fn io_error(operation: &'static str, error: std::io::Error) -> MediaError {
    decode_error(
        "ASTRA_FFMPEG_STREAM_IO",
        format!("failed to {operation}: {}", error.kind()),
    )
}
