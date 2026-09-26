use std::time::{Duration, Instant};

use astra_media::{DecodeOutput as MediaDecodeOutput, DecodeProvider};
use astra_platform::{
    DecodeKind, DecodeOutput, PlatformDecodeRequest, PlatformError, PlatformErrorCode,
};

pub(crate) struct DecodeResource {
    kind: DecodeKind,
    next_sequence: u64,
}

impl DecodeResource {
    pub(crate) fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
        if kind != DecodeKind::Image {
            gstreamer::init()
                .map_err(|_| host_error("decode.open", "GStreamer initialization failed"))?;
        }
        Ok(Self {
            kind,
            next_sequence: 1,
        })
    }

    pub(crate) fn decode(
        &mut self,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        if request.sequence != self.next_sequence
            || request.kind != self.kind
            || request.bytes.is_empty()
            || !supported_codec(request.kind, &request.codec)
            || !request.description.is_empty()
            || request.sample_rate.is_some()
            || request.channels.is_some()
            || request.coded_width.is_some()
            || request.coded_height.is_some()
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence, kind, codec, payload, or metadata is invalid",
            ));
        }
        let next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request sequence overflowed",
            )
        })?;
        let output = match request.kind {
            DecodeKind::Image => decode_image(request)?,
            DecodeKind::Audio | DecodeKind::Video => decode_gstreamer(request)?,
        };
        self.next_sequence = next_sequence;
        Ok(output)
    }
}

fn supported_codec(kind: DecodeKind, codec: &str) -> bool {
    match kind {
        DecodeKind::Image => matches!(codec, "png" | "jpeg" | "jpg" | "webp"),
        DecodeKind::Audio => matches!(codec, "wav" | "mp3" | "aac" | "opus" | "mp4" | "webm"),
        DecodeKind::Video => matches!(codec, "h264" | "vp9" | "mp4" | "webm"),
    }
}

fn decode_image(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
    let result = astra_media::ImageDecodeProvider
        .decode(&astra_media::DecodeRequest {
            kind: astra_media::DecodeKind::Image,
            codec: request.codec,
            bytes: request.bytes,
            profile: "linux-steam-sniper-release".to_string(),
        })
        .map_err(media_decode_error)?;
    match result.output {
        MediaDecodeOutput::CpuBuffer { bytes, format } => {
            Ok(DecodeOutput::CpuBuffer { format, bytes })
        }
        MediaDecodeOutput::AudioPcmI16 { .. } | MediaDecodeOutput::AudioPcmF32 { .. } => Err(
            host_error("decode.submit", "image decoder returned audio PCM"),
        ),
        MediaDecodeOutput::MediaSurfaceToken(_) => Err(host_error(
            "decode.submit",
            "image decoder returned an unsupported external media surface",
        )),
    }
}

fn decode_gstreamer(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
    use std::io::Write;

    use gstreamer::{prelude::*, ClockTime};

    let mut input = tempfile::NamedTempFile::new().map_err(|_| {
        host_error(
            "decode.submit",
            "temporary media input could not be created",
        )
    })?;
    input.write_all(&request.bytes).map_err(|_| {
        host_error(
            "decode.submit",
            "temporary media input could not be written",
        )
    })?;
    let location = input.path().to_string_lossy();
    if location.contains(['"', '\\']) {
        return Err(host_error(
            "decode.submit",
            "temporary media input path is unsupported",
        ));
    }
    let conversion = match request.kind {
        DecodeKind::Audio => {
            "audioconvert ! audioresample ! audio/x-raw,format=F32LE,layout=interleaved,rate=48000,channels=2"
        }
        DecodeKind::Video => "videoconvert ! video/x-raw,format=RGBA",
        DecodeKind::Image => unreachable!(),
    };
    let element = gstreamer::parse::launch(&format!(
        "filesrc location=\"{location}\" ! decodebin ! {conversion} ! appsink name=astra_sink sync=false"
    ))
    .map_err(|_| host_error("decode.submit", "GStreamer pipeline could not be built"))?;
    let pipeline = element
        .downcast::<gstreamer::Pipeline>()
        .map_err(|_| host_error("decode.submit", "GStreamer pipeline has an invalid type"))?;
    let sink = pipeline
        .by_name("astra_sink")
        .and_then(|element| element.downcast::<gstreamer_app::AppSink>().ok())
        .ok_or_else(|| host_error("decode.submit", "GStreamer appsink is unavailable"))?;
    let decode_result = (|| {
        pipeline
            .set_state(gstreamer::State::Playing)
            .map_err(|_| host_error("decode.submit", "GStreamer pipeline could not start"))?;
        let bus = pipeline
            .bus()
            .ok_or_else(|| host_error("decode.submit", "GStreamer pipeline bus is unavailable"))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut bytes = Vec::new();
        let mut audio_samples = Vec::new();
        loop {
            check_pipeline_error(&bus)?;
            let sample = sink.try_pull_sample(ClockTime::from_mseconds(250));
            if let Some(sample) = sample {
                let buffer = sample
                    .buffer()
                    .ok_or_else(|| host_error("decode.submit", "GStreamer sample has no buffer"))?;
                let map = buffer.map_readable().map_err(|_| {
                    host_error("decode.submit", "GStreamer sample is not CPU-readable")
                })?;
                if request.kind == DecodeKind::Audio {
                    let chunks = map.as_slice().chunks_exact(std::mem::size_of::<f32>());
                    if !chunks.remainder().is_empty() {
                        return Err(host_error(
                            "decode.submit",
                            "GStreamer produced a partial f32 audio sample",
                        ));
                    }
                    audio_samples.extend(chunks.map(|sample| {
                        f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]])
                    }));
                } else {
                    bytes.extend_from_slice(map.as_slice());
                    break;
                }
            } else if sink.is_eos() {
                break;
            } else if Instant::now() >= deadline {
                return Err(host_error("decode.submit", "GStreamer decode timed out"));
            }
        }
        check_pipeline_error(&bus)?;
        if bytes.is_empty() && audio_samples.is_empty() {
            return Err(host_error(
                "decode.submit",
                "GStreamer produced no decoded samples",
            ));
        }
        if request.kind == DecodeKind::Audio {
            Ok(DecodeOutput::AudioPcmF32 {
                sample_rate: 48_000,
                channels: 2,
                samples: audio_samples,
            })
        } else {
            Ok(DecodeOutput::CpuBuffer {
                format: "rgba8".to_string(),
                bytes: bytes.into(),
            })
        }
    })();
    let _ = pipeline.set_state(gstreamer::State::Null);
    decode_result
}

// Do not forward GStreamer debug strings: they can contain the private input path.
fn check_pipeline_error(bus: &gstreamer::Bus) -> Result<(), PlatformError> {
    if bus
        .timed_pop_filtered(gstreamer::ClockTime::ZERO, &[gstreamer::MessageType::Error])
        .is_some()
    {
        return Err(host_error("decode.submit", "GStreamer decoding failed")
            .with_field("diagnostic_code", "ASTRA_LINUX_GSTREAMER_DECODE_FAILED"));
    }
    Ok(())
}

fn media_decode_error(error: astra_media::MediaError) -> PlatformError {
    match error {
        astra_media::MediaError::Diagnostics(diagnostics) => {
            let diagnostic = diagnostics.into_iter().next();
            let mut error = PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "decode.submit",
                diagnostic
                    .as_ref()
                    .map_or("image decode failed", |value| value.message.as_str()),
            );
            if let Some(diagnostic) = diagnostic {
                error = error.with_field("diagnostic_code", diagnostic.code);
            }
            error
        }
        astra_media::MediaError::Message(message) => PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "decode.submit",
            message,
        ),
    }
}

fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}

#[cfg(test)]
mod tests;
