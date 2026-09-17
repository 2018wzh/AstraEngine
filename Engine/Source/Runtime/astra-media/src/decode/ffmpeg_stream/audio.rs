use super::backend::{ensure_pending_budget, ffmpeg_error, timestamp_us};
use super::{FfmpegAudioOutputFormat, FfmpegDecodedPacket, FfmpegStreamLimits};
use crate::decode::{decode_error, MediaError};
use crate::AudioFramePacket;
use ffmpeg_next as ffmpeg;
use std::collections::VecDeque;

pub(super) struct AudioDecoder {
    pub(super) stream_index: usize,
    pub(super) time_base: ffmpeg::Rational,
    pub(super) decoder: ffmpeg::decoder::Audio,
    pub(super) resampler: ffmpeg::software::resampling::Context,
    pub(super) format: ffmpeg::format::Sample,
    pub(super) source_layout: ffmpeg::ChannelLayout,
    pub(super) source_sample_rate: u32,
    pub(super) layout: ffmpeg::ChannelLayout,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
    pub(super) eof_sent: bool,
    pub(super) decoder_drained: bool,
    pub(super) resampler_flushed: bool,
    pub(super) next_output_pts_us: Option<u64>,
}

pub(super) fn create_audio_decoder(
    input: &ffmpeg::format::context::Input,
    output: Option<FfmpegAudioOutputFormat>,
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
    let source_sample_rate = decoder.rate();
    let source_channels = decoder.channels();
    if source_sample_rate == 0 || source_channels == 0 || source_channels > 8 {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_FORMAT",
            "FFmpeg audio track has an invalid rate or channel count",
        ));
    }
    let source_layout = if decoder.channel_layout().is_empty() {
        ffmpeg::ChannelLayout::default(i32::from(source_channels))
    } else {
        decoder.channel_layout()
    };
    if source_layout.is_empty() {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_FORMAT",
            "FFmpeg audio track has no channel layout",
        ));
    }
    let sample_rate = output.map_or(source_sample_rate, |format| format.sample_rate);
    let channels = output.map_or(source_channels, |format| format.channels);
    let layout = ffmpeg::ChannelLayout::default(i32::from(channels));
    if layout.is_empty() {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_OUTPUT",
            "FFmpeg target audio channel layout is unavailable",
        ));
    }
    let format = ffmpeg::format::Sample::I16(ffmpeg::format::sample::Type::Packed);
    let resampler = create_resampler(
        decoder.format(),
        source_layout,
        source_sample_rate,
        format,
        layout,
        sample_rate,
    )?;
    Ok(Some(AudioDecoder {
        stream_index,
        time_base,
        decoder,
        resampler,
        format,
        source_layout,
        source_sample_rate,
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
    source_layout: ffmpeg::ChannelLayout,
    source_sample_rate: u32,
    target_format: ffmpeg::format::Sample,
    target_layout: ffmpeg::ChannelLayout,
    target_sample_rate: u32,
) -> Result<ffmpeg::software::resampling::Context, MediaError> {
    ffmpeg::software::resampling::Context::get(
        source_format,
        source_layout,
        source_sample_rate,
        target_format,
        target_layout,
        target_sample_rate,
    )
    .map_err(|error| {
        ffmpeg_error(
            "ASTRA_FFMPEG_STREAM_RESAMPLE",
            "create audio resampler",
            error,
        )
    })
}

pub(super) fn reconfigure_audio_decoder(
    audio: &mut AudioDecoder,
    output: FfmpegAudioOutputFormat,
) -> Result<(), MediaError> {
    let layout = ffmpeg::ChannelLayout::default(i32::from(output.channels));
    if layout.is_empty() {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_OUTPUT",
            "FFmpeg target audio channel layout is unavailable",
        ));
    }
    let resampler = create_resampler(
        audio.decoder.format(),
        audio.source_layout,
        audio.source_sample_rate,
        audio.format,
        layout,
        output.sample_rate,
    )?;
    audio.resampler = resampler;
    audio.layout = layout;
    audio.sample_rate = output.sample_rate;
    audio.channels = output.channels;
    audio.eof_sent = false;
    audio.decoder_drained = false;
    audio.resampler_flushed = false;
    audio.next_output_pts_us = None;
    Ok(())
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
                // Formats such as PCM WAV can omit channel order in each frame.
                // Apply the same standard layout selected when opening this stream.
                if decoded.channel_layout().is_empty()
                    && i32::from(decoded.channels()) == audio.source_layout.channels()
                {
                    decoded.set_channel_layout(audio.source_layout);
                }
                let input_pts_us = timestamp_us(decoded.timestamp(), audio.time_base)?;
                // Query at microsecond precision: the wrapper's delay() checks
                // whole seconds and reports None for the usual subsecond delay.
                // SAFETY: this worker owns a live, initialized resampling context.
                let delay_us =
                    unsafe { ffmpeg::ffi::swr_get_delay(audio.resampler.as_mut_ptr(), 1_000_000) };
                let delay_us = u64::try_from(delay_us).map_err(|_| {
                    decode_error("ASTRA_FFMPEG_STREAM_TIMESTAMP", "negative resampler delay")
                })?;
                let pts_us = input_pts_us.checked_sub(delay_us).ok_or_else(|| {
                    decode_error(
                        "ASTRA_FFMPEG_STREAM_TIMESTAMP",
                        "resampler delay precedes audio timeline",
                    )
                })?;
                let mut converted = resample_buffer(audio, decoded.samples(), limits)?;
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
    let sample_count = byte_count / std::mem::size_of::<i16>();
    let mut samples = Vec::<i16>::with_capacity(sample_count);
    // SAFETY: destination capacity is reserved for the complete packed frame;
    // byte-wise copy does not require the FFmpeg source pointer to be aligned.
    unsafe {
        std::ptr::copy_nonoverlapping(pcm.as_ptr(), samples.as_mut_ptr().cast::<u8>(), byte_count);
        samples.set_len(sample_count);
    }
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
        },
        samples,
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
    let mut converted = resample_buffer(audio, 0, limits)?;
    audio.resampler.flush(&mut converted).map_err(|error| {
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
    audio.resampler_flushed = converted.samples() == 0;
    Ok(())
}

fn resample_buffer(
    audio: &mut AudioDecoder,
    input_samples: usize,
    limits: &FfmpegStreamLimits,
) -> Result<ffmpeg::frame::Audio, MediaError> {
    let budget_error = || {
        decode_error(
            "ASTRA_FFMPEG_STREAM_AUDIO_BUDGET",
            "resampled audio exceeds its packet budget",
        )
    };
    let input_samples = i32::try_from(input_samples).map_err(|_| budget_error())?;
    // SAFETY: context is live and exclusively owned; input_samples is nonnegative.
    let capacity =
        unsafe { ffmpeg::ffi::swr_get_out_samples(audio.resampler.as_mut_ptr(), input_samples) };
    let capacity = usize::try_from(capacity)
        .map_err(|_| budget_error())?
        .max(1);
    capacity
        .checked_mul(usize::from(audio.channels))
        .and_then(|n| n.checked_mul(2))
        .filter(|bytes| *bytes <= limits.max_audio_packet_bytes)
        .ok_or_else(budget_error)?;
    Ok(ffmpeg::frame::Audio::new(
        audio.format,
        capacity,
        audio.layout,
    ))
}
