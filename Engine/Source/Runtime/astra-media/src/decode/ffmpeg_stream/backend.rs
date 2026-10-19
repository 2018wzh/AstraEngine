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
    configured_color: Option<(crate::VideoColorMetadata, crate::VideoColorMetadata)>,
}

pub(super) fn create_video_decoder(
    input: &ffmpeg::format::context::Input,
    limits: &FfmpegStreamLimits,
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
    let parameters = stream.parameters();
    // ffmpeg-next exposes no dimension getters on Parameters.
    // SAFETY: the borrowed codec parameters remain owned by `input` and are
    // read only here, before the input or parameters can be dropped.
    let (declared_width, declared_height) = unsafe {
        let parameters = &*parameters.as_ptr();
        (parameters.width, parameters.height)
    };
    checked_frame_bytes(
        u32::try_from(declared_width).unwrap_or(0),
        u32::try_from(declared_height).unwrap_or(0),
        limits,
    )?;
    let context =
        ffmpeg::codec::context::Context::from_parameters(parameters).map_err(|error| {
            ffmpeg_error("ASTRA_FFMPEG_STREAM_DECODER", "create video decoder", error)
        })?;
    let decoder = context.decoder();
    let codec = decoder
        .codec()
        .or_else(|| ffmpeg::decoder::find(decoder.id()))
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_DECODER",
                "FFmpeg video decoder is unavailable",
            )
        })?;
    let mut options = ffmpeg::Dictionary::new();
    options.set(
        "max_pixels",
        &(limits.max_video_frame_bytes / 4)
            .min(i32::MAX as usize)
            .to_string(),
    );
    let decoder = decoder
        .open_as_with(codec, options)
        .and_then(|opened| opened.video())
        .map_err(|error| {
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
    checked_frame_bytes(width, height, limits)?;
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
        configured_color: None,
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
                // Validate before swscale can allocate an output frame. A stream
                // cannot silently change the dimensions bound at open time.
                checked_frame_bytes(decoded.width(), decoded.height(), limits)?;
                if decoded.width() != video.width || decoded.height() != video.height {
                    return Err(decode_error(
                        "ASTRA_FFMPEG_STREAM_VIDEO_FORMAT",
                        "FFmpeg video dimensions changed during playback",
                    ));
                }
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
                let source_color = decoded_video_color(&decoded);
                let color = match video.configured_color {
                    Some((source, presentation)) if source == source_color => presentation,
                    _ => {
                        configure_video_colors(&mut video.scaler, source_color)?;
                        let presentation =
                            presentation_video_color(&mut video.scaler, source_color)?;
                        video.configured_color = Some((source_color, presentation));
                        presentation
                    }
                };
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
                        color,
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

fn decoded_video_color(frame: &ffmpeg::frame::Video) -> crate::VideoColorMetadata {
    // AVFrame owns these fields for the lifetime of the borrowed decoded frame.
    let raw = unsafe { &*frame.as_ptr() };
    let mut color = video_color_codes(
        raw.colorspace as i32,
        raw.color_range as i32,
        raw.color_trc as i32,
    );
    // Native RGB is a pixel-format property, not the RGB texture produced by swscale.
    let descriptor = unsafe { ffmpeg::ffi::av_pix_fmt_desc_get(frame.format().into()) };
    if !descriptor.is_null()
        && unsafe { (*descriptor).flags & ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB as u64 } != 0
    {
        color.matrix = crate::VideoColorMatrix::Rgb;
    }
    color
}

fn video_color_codes(matrix: i32, range: i32, transfer: i32) -> crate::VideoColorMetadata {
    use crate::{VideoColorMatrix as M, VideoColorRange as R, VideoColorTransfer as T};
    let matrix = match matrix {
        0 => M::Rgb,
        1 => M::Bt709,
        4 => M::Fcc,
        5 | 6 => M::Bt601,
        7 => M::Smpte240m,
        2 => M::Unknown,
        _ => M::Unsupported,
    };
    let range = match range {
        1 => R::Limited,
        2 => R::Full,
        _ => R::Unknown,
    };
    let transfer = match transfer {
        1 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 17 => T::Sdr,
        16 | 18 => T::Hdr,
        2 => T::Unknown,
        _ => T::Unsupported,
    };
    crate::VideoColorMetadata {
        matrix,
        range,
        transfer,
    }
}

fn configure_video_colors(
    scaler: &mut ffmpeg::software::scaling::Context,
    color: crate::VideoColorMetadata,
) -> Result<(), MediaError> {
    use crate::{VideoColorMatrix as M, VideoColorRange as R};
    let coefficients = match color.matrix {
        M::Bt709 => 1,
        M::Fcc => 4,
        M::Bt601 => 5,
        M::Smpte240m => 7,
        _ => return Ok(()),
    };
    let full = match color.range {
        R::Limited => 0,
        R::Full => 1,
        R::Unknown => return Ok(()),
    };
    // Explicit coefficients make decoded RGB agree with reported source matrix/range.
    let result = unsafe {
        let table = ffmpeg::ffi::sws_getCoefficients(coefficients);
        ffmpeg::ffi::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            table,
            full,
            table,
            1,
            0,
            1 << 16,
            1 << 16,
        )
    };
    if result < 0 {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_COLOR",
            "FFmpeg scaler rejected video color metadata",
        ));
    }
    Ok(())
}

fn presentation_video_color(
    scaler: &mut ffmpeg::software::scaling::Context,
    source: crate::VideoColorMetadata,
) -> Result<crate::VideoColorMetadata, MediaError> {
    use crate::{VideoColorMatrix as M, VideoColorRange as R, VideoColorTransfer as T};
    // This backend presents unmanaged BGRA8 as SDR. Unlabelled sources retain
    // that existing interpretation; explicit HDR/unsupported tags stay visible.
    let transfer = match source.transfer {
        T::Unknown => T::Sdr,
        other => other,
    };
    if source.matrix == M::Rgb {
        return Ok(crate::VideoColorMetadata {
            matrix: M::Rgb,
            range: R::Full,
            transfer,
        });
    }
    if source.matrix == M::Unsupported {
        return Ok(crate::VideoColorMetadata { transfer, ..source });
    }
    let mut input = std::ptr::null_mut();
    let mut output = std::ptr::null_mut();
    let (mut input_range, mut output_range, mut brightness, mut contrast, mut saturation) =
        (0, 0, 0, 0, 0);
    // swscale owns both four-coefficient tables; inspect them while its context
    // is borrowed. These are the parameters actually used, not raster guesses.
    let status = unsafe {
        ffmpeg::ffi::sws_getColorspaceDetails(
            scaler.as_mut_ptr(),
            &mut input,
            &mut input_range,
            &mut output,
            &mut output_range,
            &mut brightness,
            &mut contrast,
            &mut saturation,
        )
    };
    if status < 0
        || input.is_null()
        || brightness != 0
        || contrast != 1 << 16
        || saturation != 1 << 16
    {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_COLOR",
            "FFmpeg scaler color conversion cannot be described",
        ));
    }
    let actual = unsafe { std::slice::from_raw_parts(input, 4) };
    let matrix = [(1, M::Bt709), (4, M::Fcc), (5, M::Bt601), (7, M::Smpte240m)]
        .into_iter()
        .find_map(|(code, matrix)| {
            let coefficients = unsafe { ffmpeg::ffi::sws_getCoefficients(code) };
            (!coefficients.is_null()
                && actual == unsafe { std::slice::from_raw_parts(coefficients, 4) })
            .then_some(matrix)
        })
        .unwrap_or(M::Unsupported);
    let range = match input_range {
        0 => R::Limited,
        1 => R::Full,
        _ => R::Unknown,
    };
    Ok(crate::VideoColorMetadata {
        matrix,
        range,
        transfer,
    })
}

pub(super) fn copy_bgra(
    frame: &ffmpeg::frame::Video,
    width: u32,
    height: u32,
    limits: &FfmpegStreamLimits,
) -> Result<Vec<u8>, MediaError> {
    let frame_bytes = checked_frame_bytes(width, height, limits)?;
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_BUDGET",
                "FFmpeg video row size overflowed",
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

fn checked_frame_bytes(
    width: u32,
    height: u32,
    limits: &FfmpegStreamLimits,
) -> Result<usize, MediaError> {
    usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes > 0 && *bytes <= limits.max_video_frame_bytes)
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_STREAM_VIDEO_BUDGET",
                "FFmpeg video frame exceeds its byte budget",
            )
        })
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

#[cfg(test)]
mod color_tests {
    use super::*;
    #[test]
    fn decoded_metadata_distinguishes_unknown_rgb_sdr_and_hdr_without_size_guesses() {
        use crate::{VideoColorMatrix as M, VideoColorRange as R, VideoColorTransfer as T};
        assert_eq!(
            video_color_codes(1, 1, 1),
            crate::VideoColorMetadata {
                matrix: M::Bt709,
                range: R::Limited,
                transfer: T::Sdr
            }
        );
        assert_eq!(
            video_color_codes(6, 2, 6),
            crate::VideoColorMetadata {
                matrix: M::Bt601,
                range: R::Full,
                transfer: T::Sdr
            }
        );
        assert_eq!(video_color_codes(0, 2, 13).matrix, M::Rgb);
        assert_eq!(
            video_color_codes(2, 0, 2),
            crate::VideoColorMetadata::default()
        );
        assert_eq!(video_color_codes(9, 1, 16).transfer, T::Hdr);
        assert_eq!(video_color_codes(9, 1, 18).matrix, M::Unsupported);
    }

    #[test]
    fn unlabelled_frames_report_actual_scaler_conversion_and_keep_explicit_hdr() {
        use crate::{
            VideoColorMatrix as M, VideoColorMetadata, VideoColorRange as R,
            VideoColorTransfer as T,
        };
        for (width, height) in [(320, 240), (1920, 1080)] {
            let mut scaler = ffmpeg::software::scaling::Context::get(
                ffmpeg::format::Pixel::YUV420P,
                width,
                height,
                ffmpeg::format::Pixel::BGRA,
                width,
                height,
                ffmpeg::software::scaling::flag::Flags::BILINEAR,
            )
            .unwrap();
            let native =
                presentation_video_color(&mut scaler, VideoColorMetadata::default()).unwrap();
            assert_eq!(
                native,
                VideoColorMetadata {
                    matrix: M::Bt601,
                    range: R::Limited,
                    transfer: T::Sdr,
                }
            );
            let explicit = VideoColorMetadata {
                matrix: M::Bt709,
                range: R::Full,
                transfer: T::Sdr,
            };
            configure_video_colors(&mut scaler, explicit).unwrap();
            assert_eq!(
                presentation_video_color(&mut scaler, explicit).unwrap(),
                explicit
            );
            assert_eq!(
                presentation_video_color(
                    &mut scaler,
                    VideoColorMetadata {
                        transfer: T::Hdr,
                        ..explicit
                    }
                )
                .unwrap()
                .transfer,
                T::Hdr
            );
        }
    }
}
