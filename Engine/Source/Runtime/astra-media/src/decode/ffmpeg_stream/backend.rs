use std::collections::VecDeque;

use astra_core::Hash256;
use ffmpeg_next as ffmpeg;

use super::{FfmpegDecodedPacket, FfmpegStreamLimits};
use crate::decode::{decode_error, MediaError};
use crate::{AudioFramePacket, VideoFramePacket};

pub(super) struct AudioDecoder {
    pub(super) stream_index: usize,
    pub(super) time_base: ffmpeg::Rational,
    pub(super) decoder: ffmpeg::decoder::Audio,
    pub(super) resampler: ffmpeg::software::resampling::Context,
    pub(super) format: ffmpeg::format::Sample,
    pub(super) layout: ffmpeg::ChannelLayout,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) eof_sent: bool,
    pub(super) decoder_drained: bool,
    pub(super) resampler_flushed: bool,
    pub(super) next_output_pts_us: Option<u64>,
}

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

pub(super) fn create_audio_decoder(
    input: &ffmpeg::format::context::Input,
) -> Result<Option<AudioDecoder>, MediaError> {
    let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) else {
        return Ok(None);
    };
    let stream_index = stream.index();
    let time_base = stream.time_base();
    let context =
        ffmpeg::codec::context::Context::from_parameters(stream.parameters()).map_err(|error| {
            ffmpeg_error("ASTRA_FFMPEG_STREAM_DECODER", "create audio decoder", error)
        })?;
    let decoder = context.decoder().audio().map_err(|error| {
        ffmpeg_error("ASTRA_FFMPEG_STREAM_DECODER", "open audio decoder", error)
    })?;
    let sample_rate = decoder.rate();
    let channels = decoder.channels();
    if sample_rate == 0 || channels == 0 || channels > 8 {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_FORMAT",
            "FFmpeg audio track has an invalid rate or channel count",
        ));
    }
    let layout = if decoder.channel_layout().is_empty() {
        ffmpeg::ChannelLayout::default(i32::from(channels))
    } else {
        decoder.channel_layout()
    };
    if layout.is_empty() {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_FORMAT",
            "FFmpeg audio track has no channel layout",
        ));
    }
    let format = ffmpeg::format::Sample::I16(ffmpeg::format::sample::Type::Packed);
    let resampler = create_resampler(decoder.format(), layout, sample_rate, format)?;
    Ok(Some(AudioDecoder {
        stream_index,
        time_base,
        decoder,
        resampler,
        format,
        layout,
        sample_rate,
        channels,
        eof_sent: false,
        decoder_drained: false,
        resampler_flushed: false,
        next_output_pts_us: None,
    }))
}

pub(super) fn create_resampler(
    source_format: ffmpeg::format::Sample,
    layout: ffmpeg::ChannelLayout,
    sample_rate: u32,
    target_format: ffmpeg::format::Sample,
) -> Result<ffmpeg::software::resampling::Context, MediaError> {
    ffmpeg::software::resampling::Context::get(
        source_format,
        layout,
        sample_rate,
        target_format,
        layout,
        sample_rate,
    )
    .map_err(|error| {
        ffmpeg_error(
            "ASTRA_FFMPEG_STREAM_RESAMPLE",
            "create audio resampler",
            error,
        )
    })
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
pub(super) fn drain_audio(
    audio: &mut AudioDecoder,
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
        let mut decoded = ffmpeg::frame::Audio::empty();
        match audio.decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let pts_us = timestamp_us(decoded.timestamp(), audio.time_base)?;
                let mut converted = ffmpeg::frame::Audio::empty();
                audio
                    .resampler
                    .run(&decoded, &mut converted)
                    .map_err(|error| {
                        ffmpeg_error("ASTRA_FFMPEG_STREAM_RESAMPLE", "convert audio frame", error)
                    })?;
                push_audio_frame(
                    audio,
                    &converted,
                    pts_us,
                    generation,
                    next_sequence,
                    seek_floor_us,
                    stream_duration_us,
                    limits,
                    pending,
                )?;
            }
            Err(ffmpeg::Error::Other {
                errno: ffmpeg::error::EAGAIN,
            }) if !eof => break,
            Err(ffmpeg::Error::Eof) if eof => {
                audio.decoder_drained = true;
                break;
            }
            Err(error) => {
                return Err(ffmpeg_error(
                    "ASTRA_FFMPEG_STREAM_FRAME",
                    "receive audio frame",
                    error,
                ))
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_audio_frame(
    audio: &mut AudioDecoder,
    frame: &ffmpeg::frame::Audio,
    pts_us: u64,
    generation: u64,
    next_sequence: &mut u64,
    seek_floor_us: u64,
    stream_duration_us: u64,
    limits: &FfmpegStreamLimits,
    pending: &mut VecDeque<FfmpegDecodedPacket>,
) -> Result<(), MediaError> {
    if frame.samples() == 0 {
        return Ok(());
    }
    let decoded_frame_count = u32::try_from(frame.samples()).map_err(|_| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
            "FFmpeg audio packet has too many frames",
        )
    })?;
    if pts_us < seek_floor_us || pts_us >= stream_duration_us {
        return Ok(());
    }
    let remaining_us = stream_duration_us - pts_us;
    let max_frame_count = remaining_us
        .checked_add(1)
        .and_then(|value| value.checked_mul(u64::from(audio.sample_rate)))
        .and_then(|value| value.checked_sub(1))
        .map(|value| value / 1_000_000)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
                "FFmpeg terminal audio packet trim overflowed",
            )
        })?;
    let frame_count = decoded_frame_count.min(max_frame_count);
    if frame_count == 0 {
        return Ok(());
    }
    let duration_us = u64::from(frame_count)
        .checked_mul(1_000_000)
        .map(|value| value / u64::from(audio.sample_rate))
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
                "FFmpeg audio packet duration overflowed",
            )
        })?;
    pts_us.checked_add(duration_us).ok_or_else(|| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_TIMESTAMP",
            "FFmpeg audio packet end timestamp overflowed",
        )
    })?;
    if !frame.is_packed() || frame.channels() != audio.channels {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_FORMAT",
            "FFmpeg resampler did not produce packed PCM",
        ));
    }
    let byte_count = usize::try_from(frame_count)
        .map_err(|_| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
                "FFmpeg audio packet frame count exceeds the host address space",
            )
        })?
        .checked_mul(usize::from(audio.channels))
        .and_then(|samples| samples.checked_mul(2))
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
                "FFmpeg audio packet byte count overflowed",
            )
        })?;
    if byte_count == 0 || byte_count > limits.max_audio_packet_bytes {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
            "FFmpeg audio packet exceeds its byte budget",
        ));
    }
    let pcm = frame.data(0).get(..byte_count).ok_or_else(|| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_FORMAT",
            "FFmpeg audio packet is truncated",
        )
    })?;
    ensure_pending_budget(pending, limits)?;
    let sequence = *next_sequence;
    *next_sequence = sequence.checked_add(1).ok_or_else(|| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_SEQUENCE",
            "FFmpeg audio sequence overflowed",
        )
    })?;
    let bytes = pcm.to_vec();
    let content_hash = Hash256::from_sha256(&bytes);
    pending.push_back(FfmpegDecodedPacket::Audio {
        packet: AudioFramePacket {
            generation,
            sequence,
            resource_id: format!("ffmpeg.audio.{generation}.{sequence}"),
            pts_us,
            duration_us,
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            frame_count,
            content_hash,
        },
        pcm_s16le: bytes,
    });
    audio.next_output_pts_us = Some(pts_us.checked_add(duration_us).ok_or_else(|| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_TIMESTAMP",
            "FFmpeg audio output clock overflowed",
        )
    })?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn flush_audio_resampler(
    audio: &mut AudioDecoder,
    generation: u64,
    next_sequence: &mut u64,
    seek_floor_us: u64,
    stream_duration_us: u64,
    limits: &FfmpegStreamLimits,
    pending: &mut VecDeque<FfmpegDecodedPacket>,
) -> Result<(), MediaError> {
    if audio.resampler_flushed || pending.len() >= limits.max_pending_packets {
        return Ok(());
    }
    let capacity = audio
        .resampler
        .delay()
        .and_then(|delay| usize::try_from(delay.output).ok())
        .filter(|samples| *samples > 0)
        .unwrap_or(1);
    let mut converted = ffmpeg::frame::Audio::new(audio.format, capacity, audio.layout);
    let remaining = audio.resampler.flush(&mut converted).map_err(|error| {
        ffmpeg_error(
            "ASTRA_FFMPEG_STREAM_RESAMPLE",
            "flush audio resampler",
            error,
        )
    })?;
    if converted.samples() > 0 {
        let pts_us = audio.next_output_pts_us.unwrap_or(seek_floor_us);
        push_audio_frame(
            audio,
            &converted,
            pts_us,
            generation,
            next_sequence,
            seek_floor_us,
            stream_duration_us,
            limits,
            pending,
        )?;
    }
    audio.resampler_flushed = remaining.is_none();
    Ok(())
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
                let content_hash = Hash256::from_sha256(&bgra);
                pending.push_back(FfmpegDecodedPacket::Video {
                    packet: VideoFramePacket {
                        generation,
                        sequence,
                        resource_id: format!("ffmpeg.video.{generation}.{sequence}"),
                        pts_us,
                        duration_us: frame_duration_us,
                        width: video.width,
                        height: video.height,
                        content_hash,
                    },
                    bgra8: bgra,
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
