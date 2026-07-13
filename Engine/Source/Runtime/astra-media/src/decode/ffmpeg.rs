use std::io::Write;

use astra_core::Hash256;
use ffmpeg_next as ffmpeg;
use tempfile::Builder;

use super::{
    decode_error, DecodeKind, DecodeOutput, DecodeRequest, DecodeResult, MediaError,
    MAX_DECODED_AUDIO_BYTES, MAX_DECODED_VIDEO_FRAME_BYTES,
};

pub(super) fn probe() -> Result<(), MediaError> {
    ffmpeg::init().map_err(|error| {
        decode_error(
            "ASTRA_FFMPEG_PROBE",
            format!("FFmpeg initialization failed: {error}"),
        )
    })
}

pub(super) fn decode(
    provider_id: String,
    request: &DecodeRequest,
) -> Result<DecodeResult, MediaError> {
    let suffix = format!(".{}", request.codec);
    let mut source = Builder::new()
        .prefix("astra-media-")
        .suffix(&suffix)
        .tempfile()
        .map_err(|error| io_error("create bounded FFmpeg input", error))?;
    source
        .write_all(&request.bytes)
        .map_err(|error| io_error("write bounded FFmpeg input", error))?;
    source
        .flush()
        .map_err(|error| io_error("flush bounded FFmpeg input", error))?;

    let mut input = ffmpeg::format::input(source.path()).map_err(|error| {
        decode_error(
            "ASTRA_FFMPEG_DEMUX",
            format!("FFmpeg rejected the encoded input: {error}"),
        )
    })?;
    match request.kind {
        DecodeKind::Audio => decode_audio(provider_id, request, &mut input),
        DecodeKind::Video => decode_video(provider_id, request, &mut input),
        DecodeKind::Image => Err(decode_error(
            "ASTRA_FFMPEG_KIND_UNSUPPORTED",
            "FFmpeg provider does not own image decoding",
        )),
    }
}

fn decode_audio(
    provider_id: String,
    request: &DecodeRequest,
    input: &mut ffmpeg::format::context::Input,
) -> Result<DecodeResult, MediaError> {
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .ok_or_else(|| decode_error("ASTRA_FFMPEG_STREAM", "audio stream is missing"))?;
    let stream_index = stream.index();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_DECODER", "create audio decoder", error))?;
    let mut decoder = context
        .decoder()
        .audio()
        .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_DECODER", "open audio decoder", error))?;
    let sample_rate = decoder.rate();
    let channels = decoder.channels();
    if sample_rate == 0 || channels == 0 || channels > 8 {
        return Err(decode_error(
            "ASTRA_FFMPEG_AUDIO_FORMAT",
            "decoded audio has an invalid sample rate or channel count",
        ));
    }
    let source_layout = if decoder.channel_layout().is_empty() {
        ffmpeg::ChannelLayout::default(i32::from(channels))
    } else {
        decoder.channel_layout()
    };
    if source_layout.is_empty() {
        return Err(decode_error(
            "ASTRA_FFMPEG_AUDIO_FORMAT",
            "decoded audio channel layout is unavailable",
        ));
    }
    let target_format = ffmpeg::format::Sample::I16(ffmpeg::format::sample::Type::Packed);
    let mut resampler = ffmpeg::software::resampling::Context::get(
        decoder.format(),
        source_layout,
        sample_rate,
        target_format,
        source_layout,
        sample_rate,
    )
    .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_RESAMPLE", "create audio resampler", error))?;
    let mut pcm = Vec::new();

    for (packet_stream, packet) in input.packets() {
        if packet_stream.index() != stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_PACKET", "submit audio packet", error))?;
        drain_audio_frames(&mut decoder, &mut resampler, channels, &mut pcm, false)?;
    }
    decoder
        .send_eof()
        .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_EOS", "flush audio decoder", error))?;
    drain_audio_frames(&mut decoder, &mut resampler, channels, &mut pcm, true)?;

    while let Some(pending) = resampler.delay() {
        let samples = usize::try_from(pending.output)
            .ok()
            .filter(|samples| *samples > 0)
            .ok_or_else(|| {
                decode_error(
                    "ASTRA_FFMPEG_RESAMPLE",
                    "audio resampler reported an invalid pending sample count",
                )
            })?;
        let mut output = ffmpeg::frame::Audio::new(target_format, samples, source_layout);
        let delay = resampler.flush(&mut output).map_err(|error| {
            ffmpeg_error("ASTRA_FFMPEG_RESAMPLE", "flush audio resampler", error)
        })?;
        append_audio_frame(&output, channels, &mut pcm)?;
        if delay.is_none() {
            break;
        }
    }
    if pcm.is_empty() {
        return Err(decode_error(
            "ASTRA_FFMPEG_EMPTY_OUTPUT",
            "FFmpeg audio decode produced no PCM samples",
        ));
    }
    Ok(DecodeResult {
        provider_id,
        kind: DecodeKind::Audio,
        codec: request.codec.clone(),
        output: DecodeOutput::CpuBuffer {
            hash: Hash256::from_sha256(&pcm),
            bytes: pcm,
            format: format!("pcm_s16le:{sample_rate}:{channels}"),
        },
        diagnostics: Vec::new(),
    })
}

fn drain_audio_frames(
    decoder: &mut ffmpeg::decoder::Audio,
    resampler: &mut ffmpeg::software::resampling::Context,
    channels: u16,
    pcm: &mut Vec<u8>,
    eos: bool,
) -> Result<(), MediaError> {
    loop {
        let mut decoded = ffmpeg::frame::Audio::empty();
        match decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let mut output = ffmpeg::frame::Audio::empty();
                resampler.run(&decoded, &mut output).map_err(|error| {
                    ffmpeg_error("ASTRA_FFMPEG_RESAMPLE", "convert audio frame", error)
                })?;
                append_audio_frame(&output, channels, pcm)?;
            }
            Err(ffmpeg::Error::Other {
                errno: ffmpeg::error::EAGAIN,
            }) if !eos => break,
            Err(ffmpeg::Error::Eof) if eos => break,
            Err(error) => {
                return Err(ffmpeg_error(
                    "ASTRA_FFMPEG_FRAME",
                    "receive audio frame",
                    error,
                ))
            }
        }
    }
    Ok(())
}

fn append_audio_frame(
    frame: &ffmpeg::frame::Audio,
    channels: u16,
    pcm: &mut Vec<u8>,
) -> Result<(), MediaError> {
    if frame.samples() == 0 {
        return Ok(());
    }
    if !frame.is_packed() || frame.channels() != channels {
        return Err(decode_error(
            "ASTRA_FFMPEG_AUDIO_FORMAT",
            "resampled audio frame does not match packed PCM contract",
        ));
    }
    let byte_count = frame
        .samples()
        .checked_mul(usize::from(channels))
        .and_then(|samples| samples.checked_mul(std::mem::size_of::<i16>()))
        .ok_or_else(|| {
            decode_error(
                "ASTRA_FFMPEG_AUDIO_BUDGET",
                "decoded audio byte count overflowed",
            )
        })?;
    let data = frame.data(0);
    if byte_count > data.len() {
        return Err(decode_error(
            "ASTRA_FFMPEG_AUDIO_FORMAT",
            "resampled audio frame is truncated",
        ));
    }
    if pcm
        .len()
        .checked_add(byte_count)
        .is_none_or(|total| total > MAX_DECODED_AUDIO_BYTES)
    {
        return Err(decode_error(
            "ASTRA_FFMPEG_AUDIO_BUDGET",
            "decoded audio exceeds the bounded CPU buffer budget",
        ));
    }
    pcm.extend_from_slice(&data[..byte_count]);
    Ok(())
}

fn decode_video(
    provider_id: String,
    request: &DecodeRequest,
    input: &mut ffmpeg::format::context::Input,
) -> Result<DecodeResult, MediaError> {
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| decode_error("ASTRA_FFMPEG_STREAM", "video stream is missing"))?;
    let stream_index = stream.index();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_DECODER", "create video decoder", error))?;
    let mut decoder = context
        .decoder()
        .video()
        .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_DECODER", "open video decoder", error))?;
    let width = decoder.width();
    let height = decoder.height();
    let expected_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| decode_error("ASTRA_FFMPEG_VIDEO_BUDGET", "video frame size overflowed"))?;
    if expected_bytes == 0 || expected_bytes > MAX_DECODED_VIDEO_FRAME_BYTES {
        return Err(decode_error(
            "ASTRA_FFMPEG_VIDEO_BUDGET",
            "video frame exceeds the bounded CPU buffer budget",
        ));
    }
    let mut scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        width,
        height,
        ffmpeg::format::Pixel::BGRA,
        width,
        height,
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_SCALE", "create video scaler", error))?;
    let mut frame = None;

    for (packet_stream, packet) in input.packets() {
        if packet_stream.index() != stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_PACKET", "submit video packet", error))?;
        frame = receive_video_frame(&mut decoder, &mut scaler, width, height, false)?;
        if frame.is_some() {
            break;
        }
    }
    if frame.is_none() {
        decoder
            .send_eof()
            .map_err(|error| ffmpeg_error("ASTRA_FFMPEG_EOS", "flush video decoder", error))?;
        frame = receive_video_frame(&mut decoder, &mut scaler, width, height, true)?;
    }
    let bgra = frame.ok_or_else(|| {
        decode_error(
            "ASTRA_FFMPEG_EMPTY_OUTPUT",
            "FFmpeg video decode produced no frame",
        )
    })?;
    Ok(DecodeResult {
        provider_id,
        kind: DecodeKind::Video,
        codec: request.codec.clone(),
        output: DecodeOutput::CpuBuffer {
            hash: Hash256::from_sha256(&bgra),
            bytes: bgra,
            format: format!("bgra8:first_frame:{width}x{height}"),
        },
        diagnostics: Vec::new(),
    })
}

fn receive_video_frame(
    decoder: &mut ffmpeg::decoder::Video,
    scaler: &mut ffmpeg::software::scaling::Context,
    width: u32,
    height: u32,
    eos: bool,
) -> Result<Option<Vec<u8>>, MediaError> {
    let mut decoded = ffmpeg::frame::Video::empty();
    match decoder.receive_frame(&mut decoded) {
        Ok(()) => {
            let mut converted = ffmpeg::frame::Video::empty();
            scaler.run(&decoded, &mut converted).map_err(|error| {
                ffmpeg_error("ASTRA_FFMPEG_SCALE", "convert video frame", error)
            })?;
            let row_bytes = usize::try_from(width)
                .ok()
                .and_then(|width| width.checked_mul(4))
                .ok_or_else(|| {
                    decode_error("ASTRA_FFMPEG_VIDEO_BUDGET", "video row size overflowed")
                })?;
            if converted.stride(0) < row_bytes {
                return Err(decode_error(
                    "ASTRA_FFMPEG_VIDEO_FORMAT",
                    "converted video frame stride is truncated",
                ));
            }
            let mut bgra =
                Vec::with_capacity(row_bytes.checked_mul(height as usize).ok_or_else(|| {
                    decode_error("ASTRA_FFMPEG_VIDEO_BUDGET", "video frame size overflowed")
                })?);
            let data = converted.data(0);
            for row in 0..height as usize {
                let start = row.checked_mul(converted.stride(0)).ok_or_else(|| {
                    decode_error("ASTRA_FFMPEG_VIDEO_BUDGET", "video row offset overflowed")
                })?;
                let end = start.checked_add(row_bytes).ok_or_else(|| {
                    decode_error("ASTRA_FFMPEG_VIDEO_BUDGET", "video row end overflowed")
                })?;
                let bytes = data.get(start..end).ok_or_else(|| {
                    decode_error(
                        "ASTRA_FFMPEG_VIDEO_FORMAT",
                        "converted video frame is truncated",
                    )
                })?;
                bgra.extend_from_slice(bytes);
            }
            Ok(Some(bgra))
        }
        Err(ffmpeg::Error::Other {
            errno: ffmpeg::error::EAGAIN,
        }) if !eos => Ok(None),
        Err(ffmpeg::Error::Eof) if eos => Ok(None),
        Err(error) => Err(ffmpeg_error(
            "ASTRA_FFMPEG_FRAME",
            "receive video frame",
            error,
        )),
    }
}

fn ffmpeg_error(code: &'static str, operation: &'static str, error: ffmpeg::Error) -> MediaError {
    decode_error(code, format!("FFmpeg failed to {operation}: {error}"))
}

fn io_error(operation: &'static str, error: std::io::Error) -> MediaError {
    decode_error(
        "ASTRA_FFMPEG_TEMP_IO",
        format!("failed to {operation}: {}", error.kind()),
    )
}
