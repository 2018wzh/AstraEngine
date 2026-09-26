use astra_media::{DecodeOutput as MediaDecodeOutput, DecodeProvider};
use astra_platform::{
    DecodeKind, DecodeOutput, DecodeStreamAction, PlatformDecodeRequest, PlatformError,
    PlatformErrorCode,
};
mod pipeline;
mod video;
mod worker;
use pipeline::MediaPipeline;
use video::VideoStream;
pub(crate) use worker::DecodeSession;

pub(super) struct DecodeResource {
    kind: DecodeKind,
    next_sequence: u64,
    video: Option<VideoStream>,
    failed: bool,
}

impl DecodeResource {
    pub(super) fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
        if kind != DecodeKind::Image {
            gstreamer::init()
                .map_err(|_| host_error("decode.open", "GStreamer initialization failed"))?;
        }
        Ok(Self {
            kind,
            next_sequence: 1,
            video: None,
            failed: false,
        })
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        if let Some(video) = self.video.take() {
            video.close()?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn decode(&mut self, request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
        self.decode_cancellable(request, &|| false)
    }

    fn decode_cancellable(
        &mut self,
        request: PlatformDecodeRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<DecodeOutput, PlatformError> {
        if self.failed {
            return Err(invalid("failed decoder must be closed"));
        }
        if request.sequence != self.next_sequence
            || request.kind != self.kind
            || !request.description.is_empty()
            || request.sample_rate.is_some()
            || request.channels.is_some()
            || request.coded_width.is_some()
            || request.coded_height.is_some()
        {
            return Err(invalid(
                "decode request sequence, kind, or metadata is invalid",
            ));
        }
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| invalid("decode sequence overflowed"))?;
        match request.stream_action {
            DecodeStreamAction::OneShot | DecodeStreamAction::Start => {
                if request.bytes.is_empty()
                    || !supported_codec(self.kind, &request.codec)
                    || self.video.is_some()
                {
                    return Err(invalid("decode input, codec, or stream state is invalid"));
                }
                if (request.stream_action == DecodeStreamAction::Start)
                    != (self.kind == DecodeKind::Video)
                {
                    return Err(invalid(
                        "video requires Start/Next; image and audio require OneShot",
                    ));
                }
            }
            DecodeStreamAction::Next => {
                if self.video.is_none() || !request.bytes.is_empty() || !request.codec.is_empty() {
                    return Err(invalid(
                        "Next requires an active stream and no encoded input",
                    ));
                }
            }
        }
        let output = (|| {
            if cancelled() {
                return Err(cancelled_error());
            }
            match request.stream_action {
                DecodeStreamAction::Start => {
                    let (stream, output) = VideoStream::open(&request.bytes, cancelled)?;
                    self.video = Some(stream);
                    Ok(output)
                }
                DecodeStreamAction::Next => self
                    .video
                    .as_mut()
                    .expect("validated stream")
                    .next(cancelled),
                DecodeStreamAction::OneShot => match self.kind {
                    DecodeKind::Image => decode_image(request),
                    DecodeKind::Audio => decode_audio(&request.bytes, cancelled),
                    DecodeKind::Video => unreachable!("validated stream action"),
                },
            }
        })();
        match output {
            Ok(output) if !cancelled() => {
                self.next_sequence = next_sequence;
                Ok(output)
            }
            result => {
                self.video.take();
                self.failed = true;
                match result {
                    Err(error) => Err(error),
                    Ok(_) => Err(cancelled_error()),
                }
            }
        }
    }
}

fn invalid(message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, "decode.submit", message)
}

fn cancelled_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Cancelled,
        "decode.submit",
        "decode request was cancelled",
    )
}

fn decode_audio(bytes: &[u8], cancelled: &dyn Fn() -> bool) -> Result<DecodeOutput, PlatformError> {
    let pipeline = MediaPipeline::open(bytes,
        "audioconvert ! audioresample ! audio/x-raw,format=F32LE,layout=interleaved,rate=48000,channels=2", cancelled)?;
    let mut samples = Vec::new();
    while let Some(sample) = pipeline.pull(cancelled)? {
        let buffer = sample
            .buffer()
            .ok_or_else(|| invalid("audio sample has no buffer"))?;
        let map = buffer
            .map_readable()
            .map_err(|_| invalid("audio sample is not CPU-readable"))?;
        let chunks = map.as_slice().chunks_exact(4);
        if !chunks.remainder().is_empty()
            || (samples.len() + chunks.len()) > pipeline::MAX_FRAME_BYTES / 4
        {
            return Err(host_error(
                "decode.submit",
                "audio output is unaligned or exceeds the Linux decode budget",
            ));
        }
        samples.extend(
            chunks.map(|sample| f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]])),
        );
    }
    if samples.is_empty() {
        return Err(host_error(
            "decode.submit",
            "GStreamer produced no decoded samples",
        ));
    }
    pipeline.close()?;
    Ok(DecodeOutput::AudioPcmF32 {
        sample_rate: 48_000,
        channels: 2,
        samples,
    })
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
