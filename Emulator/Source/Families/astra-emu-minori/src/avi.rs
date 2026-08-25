//! Bounded, pure-Rust Minori AVI playback adapter.
//!
//! Minori movies in the observed samples are RIFF/AVI containers with one
//! WMV3 video stream and, when present, one little-endian PCM audio stream.
//! The demuxer/WMV3 implementation is shared with the existing `na_wmv_player`
//! crate; this module owns only the Minori stream contract, timeline checks and
//! bounded telemetry.  Callers choose the backing `Read + Seek` source, so the
//! Headless path can use a revision-pinned range reader while a Manager worker
//! can use an already-owned bounded buffer.

use std::io::{Cursor, Read, Seek};

use astra_core::Diagnostic;
use astra_media::{
    DecodeCapability, DecodeKind, DecodeOutput, DecodeProvider, DecodeRequest, DecodeResult,
    MediaError, ProviderPriority,
};
use wmv_decoder::{AviDemuxer, AviPacketKind, AviStreamFormat, DecoderError, Wmv3Decoder};

const MAX_PREVIEW_EVENTS: usize = 4096;
const MAX_PREVIEW_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREVIEW_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_VIDEO_DIMENSION: u32 = 16_384;
const MAX_VIDEO_PACKET_BYTES: usize = 64 * 1024 * 1024;

pub const MINORI_AVI_DECODE_PROVIDER_ID: &str = "astra.decode.minori.avi";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MinoriAviTelemetry {
    pub encoded_bytes: u64,
    pub video_packets: u64,
    pub dropped_video_packets: u64,
    pub audio_packets: u64,
    pub decoded_frames: u64,
    pub decoded_audio_samples: u64,
}

pub enum MinoriAviEvent {
    Video {
        pts_us: u64,
        width: u32,
        height: u32,
        bgra8: Vec<u8>,
    },
    Audio {
        pts_ms: i64,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    },
    End,
}

pub struct MinoriAviDecoder<R> {
    demuxer: AviDemuxer<R>,
    video_stream: usize,
    audio_stream: Option<usize>,
    video: Wmv3Decoder,
    video_width: u32,
    video_height: u32,
    audio_sample_rate: Option<u32>,
    audio_channels: Option<u16>,
    ended: bool,
    duration_us: u64,
    telemetry: MinoriAviTelemetry,
    last_video_pts_us: Option<u64>,
    last_audio_pts_us: Option<u64>,
}

/// Explicit pure-Rust first-frame binding for Minori's RIFF/AVI movies.
///
/// The shared viewer contract is a bounded preview, so this provider returns
/// the first decoded video frame only.  Playback uses [`MinoriAviDecoder`]
/// directly and remains a separate, timestamped stream path.
#[derive(Debug, Clone, Default)]
pub struct MinoriAviDecodeProvider;

impl MinoriAviDecodeProvider {
    pub fn capability(&self) -> DecodeCapability {
        DecodeCapability {
            provider_id: MINORI_AVI_DECODE_PROVIDER_ID.to_owned(),
            priority: ProviderPriority::Platform,
            kinds: vec![DecodeKind::Video],
            codecs: vec!["avi".to_owned()],
            feature_gated: false,
            packaged_eligible: true,
            reference_only: false,
        }
    }
}

impl DecodeProvider for MinoriAviDecodeProvider {
    fn capability(&self) -> DecodeCapability {
        MinoriAviDecodeProvider::capability(self)
    }

    fn decode(&self, request: &DecodeRequest) -> Result<DecodeResult, MediaError> {
        if request.kind != DecodeKind::Video {
            return Err(avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_KIND"));
        }
        if !request.codec.eq_ignore_ascii_case("avi") {
            return Err(avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_CODEC"));
        }
        validate_preview_input_len(request.bytes.len())?;
        let mut decoder = MinoriAviDecoder::new(Cursor::new(request.bytes.clone()))
            .map_err(|_| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_HEADER"))?;
        for _ in 0..MAX_PREVIEW_EVENTS {
            match decoder
                .next_event()
                .map_err(|_| avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_DECODE"))?
            {
                MinoriAviEvent::Video {
                    width,
                    height,
                    mut bgra8,
                    ..
                } => {
                    if bgra8.len() > MAX_PREVIEW_FRAME_BYTES || !bgra8.len().is_multiple_of(4) {
                        return Err(avi_preview_diagnostic("ASTRA_EMU_MINORI_AVI_PREVIEW_FRAME"));
                    }
                    for pixel in bgra8.as_chunks_mut::<4>().0.iter_mut() {
                        pixel.swap(0, 2);
                    }
                    return Ok(DecodeResult {
                        provider_id: MINORI_AVI_DECODE_PROVIDER_ID.to_owned(),
                        kind: request.kind,
                        codec: request.codec.to_ascii_lowercase(),
                        output: DecodeOutput::CpuBuffer {
                            bytes: bgra8.into(),
                            format: format!("rgba8:first_frame:{width}x{height}"),
                        },
                        diagnostics: Vec::new(),
                    });
                }
                MinoriAviEvent::Audio { .. } => {}
                MinoriAviEvent::End => {
                    return Err(avi_preview_diagnostic(
                        "ASTRA_EMU_MINORI_AVI_PREVIEW_NO_FRAME",
                    ));
                }
            }
        }
        Err(avi_preview_diagnostic(
            "ASTRA_EMU_MINORI_AVI_PREVIEW_EVENT_LIMIT",
        ))
    }
}

fn avi_preview_diagnostic(code: &'static str) -> MediaError {
    MediaError::Diagnostics(vec![Diagnostic::blocking(
        code,
        "Minori AVI first-frame preview was rejected",
    )])
}

fn validate_preview_input_len(len: usize) -> Result<(), MediaError> {
    if len == 0 || len > MAX_PREVIEW_INPUT_BYTES {
        return Err(avi_preview_diagnostic(
            "ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT",
        ));
    }
    Ok(())
}

fn validate_video_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_VIDEO_DIMENSION || height > MAX_VIDEO_DIMENSION {
        return Err("ASTRA_EMU_MINORI_AVI_VIDEO_DIMENSIONS".to_owned());
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS".to_owned())?;
    if pixels > MAX_PREVIEW_FRAME_BYTES as u64 {
        return Err("ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS".to_owned());
    }
    Ok(())
}

impl<R: Read + Seek> MinoriAviDecoder<R> {
    pub fn new(reader: R) -> Result<Self, String> {
        let demuxer =
            AviDemuxer::open(reader).map_err(|_| "ASTRA_EMU_MINORI_AVI_HEADER".to_owned())?;
        let mut video_stream = None;
        let mut audio_stream = None;
        let mut video = None;
        let mut video_dimensions = None;
        let mut audio_sample_rate = None;
        let mut audio_channels = None;
        for stream in demuxer.streams() {
            match &stream.format {
                AviStreamFormat::Video(info) => {
                    if video_stream.is_some() {
                        return Err("ASTRA_EMU_MINORI_AVI_VIDEO_STREAM_CONFLICT".into());
                    }
                    if info.compression != *b"WMV3"
                        || stream.handler.to_ascii_uppercase() != *b"WMV3"
                    {
                        return Err("ASTRA_EMU_MINORI_AVI_VIDEO_CODEC".into());
                    }
                    validate_video_dimensions(info.width, info.height)?;
                    video_stream = Some(stream.index);
                    video_dimensions = Some((info.width, info.height));
                    video = Some(
                        Wmv3Decoder::new(info.width, info.height, &info.extra_data)
                            .map_err(|_| "ASTRA_EMU_MINORI_WMV3_SEQUENCE".to_owned())?,
                    );
                }
                AviStreamFormat::Audio(info) => {
                    if audio_stream.is_some() {
                        return Err("ASTRA_EMU_MINORI_AVI_AUDIO_STREAM_CONFLICT".into());
                    }
                    if info.format_tag != 1
                        || info.bits_per_sample != 16
                        || !(1..=2).contains(&info.channels)
                        || info.sample_rate == 0
                        || info.block_align != info.channels.saturating_mul(2)
                        || stream.sample_size != u32::from(info.block_align)
                    {
                        return Err("ASTRA_EMU_MINORI_AVI_AUDIO_FORMAT".into());
                    }
                    audio_stream = Some(stream.index);
                    audio_sample_rate = Some(info.sample_rate);
                    audio_channels = Some(info.channels);
                }
                AviStreamFormat::Other { .. } => {}
            }
        }
        let video_stream_index =
            video_stream.ok_or_else(|| "ASTRA_EMU_MINORI_AVI_VIDEO_STREAM_MISSING".to_owned())?;
        let video_info = demuxer
            .streams()
            .get(video_stream_index)
            .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_VIDEO_STREAM_MISSING".to_owned())?;
        if video_info.rate == 0 || video_info.scale == 0 || video_info.length == 0 {
            return Err("ASTRA_EMU_MINORI_AVI_TIMELINE".into());
        }
        let (video_width, video_height) = video_dimensions
            .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_VIDEO_STREAM_MISSING".to_owned())?;
        let duration_us = u64::from(video_info.length)
            .checked_mul(u64::from(video_info.scale))
            .and_then(|value| value.checked_mul(1_000_000))
            .map(|value| value / u64::from(video_info.rate))
            .filter(|value| *value > 0)
            .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TIMELINE".to_owned())?;
        Ok(Self {
            demuxer,
            video_stream: video_stream_index,
            audio_stream,
            video: video.ok_or_else(|| "ASTRA_EMU_MINORI_AVI_VIDEO_STREAM_MISSING".to_owned())?,
            video_width,
            video_height,
            audio_sample_rate,
            audio_channels,
            ended: false,
            duration_us,
            telemetry: MinoriAviTelemetry::default(),
            last_video_pts_us: None,
            last_audio_pts_us: None,
        })
    }

    pub fn telemetry(&self) -> MinoriAviTelemetry {
        self.telemetry
    }

    pub fn duration_us(&self) -> u64 {
        self.duration_us
    }

    pub fn next_event(&mut self) -> Result<MinoriAviEvent, String> {
        if self.ended {
            return Ok(MinoriAviEvent::End);
        }
        loop {
            let Some(packet) = self
                .demuxer
                .next_packet()
                .map_err(|_| "ASTRA_EMU_MINORI_AVI_DEMUX".to_owned())?
            else {
                self.ended = true;
                return Ok(MinoriAviEvent::End);
            };
            self.telemetry.encoded_bytes = self
                .telemetry
                .encoded_bytes
                .checked_add(
                    u64::try_from(packet.data.len())
                        .map_err(|_| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?,
                )
                .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
            if packet.data.len() > MAX_VIDEO_PACKET_BYTES {
                return Err("ASTRA_EMU_MINORI_AVI_PACKET_BOUNDS".into());
            }
            match packet.kind {
                AviPacketKind::Video if packet.stream_index == self.video_stream => {
                    self.telemetry.video_packets = self
                        .telemetry
                        .video_packets
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    if packet.data.is_empty() {
                        self.telemetry.dropped_video_packets = self
                            .telemetry
                            .dropped_video_packets
                            .checked_add(1)
                            .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                        continue;
                    }
                    let pts_ms = u32::try_from(packet.pts_us / 1_000)
                        .map_err(|_| "ASTRA_EMU_MINORI_AVI_TIMELINE".to_owned())?;
                    let decoded = self
                        .video
                        .decode_frame_owned(&packet.data, pts_ms)
                        .map_err(|error| {
                            tracing::error!(
                                event = "astra_emu_minori_wmv3_decode_failed",
                                error_class = decoder_error_class(&error),
                                packet_index = self.telemetry.video_packets,
                                encoded_bytes = packet.data.len(),
                                pts_us = packet.pts_us,
                                "WMV3 packet decode failed"
                            );
                            "ASTRA_EMU_MINORI_WMV3_DECODE".to_owned()
                        })?;
                    let bgra8 = decoded
                        .frame
                        .to_bgra8()
                        .map_err(|_| "ASTRA_EMU_MINORI_WMV3_FRAME".to_owned())?;
                    let expected = usize::try_from(decoded.frame.width)
                        .ok()
                        .and_then(|width| {
                            usize::try_from(decoded.frame.height)
                                .ok()
                                .and_then(|height| width.checked_mul(height))
                        })
                        .and_then(|pixels| pixels.checked_mul(4))
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS".to_owned())?;
                    if expected > MAX_PREVIEW_FRAME_BYTES {
                        return Err("ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS".into());
                    }
                    if bgra8.len() != expected
                        || decoded.frame.width != self.video_width
                        || decoded.frame.height != self.video_height
                        || self
                            .last_video_pts_us
                            .is_some_and(|previous| packet.pts_us < previous)
                    {
                        return Err("ASTRA_EMU_MINORI_AVI_TIMELINE".into());
                    }
                    self.last_video_pts_us = Some(packet.pts_us);
                    self.telemetry.decoded_frames = self
                        .telemetry
                        .decoded_frames
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    return Ok(MinoriAviEvent::Video {
                        pts_us: packet.pts_us,
                        width: decoded.frame.width,
                        height: decoded.frame.height,
                        bgra8,
                    });
                }
                AviPacketKind::Audio if Some(packet.stream_index) == self.audio_stream => {
                    let channels = self
                        .audio_channels
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_AUDIO_FORMAT".to_owned())?;
                    let sample_rate = self
                        .audio_sample_rate
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_AUDIO_FORMAT".to_owned())?;
                    if packet.data.len() % 2 != 0 {
                        return Err("ASTRA_EMU_MINORI_AVI_AUDIO_FORMAT".into());
                    }
                    let samples = packet
                        .data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|bytes| f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0)
                        .collect::<Vec<_>>();
                    if samples.is_empty()
                        || !samples.len().is_multiple_of(usize::from(channels))
                        || self
                            .last_audio_pts_us
                            .is_some_and(|previous| packet.pts_us < previous)
                    {
                        return Err("ASTRA_EMU_MINORI_AVI_AUDIO_FORMAT".into());
                    }
                    self.last_audio_pts_us = Some(packet.pts_us);
                    self.telemetry.audio_packets = self
                        .telemetry
                        .audio_packets
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    self.telemetry.decoded_audio_samples =
                        self.telemetry
                            .decoded_audio_samples
                            .checked_add(u64::try_from(samples.len()).map_err(|_| {
                                "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned()
                            })?)
                            .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    return Ok(MinoriAviEvent::Audio {
                        pts_ms: i64::try_from(packet.pts_us / 1_000)
                            .map_err(|_| "ASTRA_EMU_MINORI_AVI_AUDIO_TIMELINE".to_owned())?,
                        sample_rate,
                        channels,
                        samples,
                    });
                }
                _ => return Err("ASTRA_EMU_MINORI_AVI_PACKET_STREAM".into()),
            }
        }
    }
}

fn decoder_error_class(error: &DecoderError) -> &'static str {
    match error {
        DecoderError::Io(_) => "io",
        DecoderError::InvalidData(_) => "invalid_data",
        DecoderError::Unsupported(_) => "unsupported",
        DecoderError::EndOfStream => "end_of_stream",
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use astra_media::{DecodeKind, DecodeProvider, DecodeRequest, MediaError};

    use super::{
        validate_preview_input_len, validate_video_dimensions, MinoriAviDecodeProvider,
        MinoriAviDecoder, MAX_PREVIEW_INPUT_BYTES, MINORI_AVI_DECODE_PROVIDER_ID,
    };

    #[test]
    fn truncated_container_is_rejected_before_stream_selection() {
        let Err(error) = MinoriAviDecoder::new(Cursor::new(Vec::<u8>::new())) else {
            panic!("empty AVI unexpectedly opened");
        };
        assert_eq!(error, "ASTRA_EMU_MINORI_AVI_HEADER");
    }

    #[test]
    fn preview_provider_is_explicit_and_bounded() {
        let provider = MinoriAviDecodeProvider;
        let capability = provider.capability();
        assert_eq!(capability.provider_id, MINORI_AVI_DECODE_PROVIDER_ID);
        assert_eq!(capability.kinds, vec![DecodeKind::Video]);
        assert_eq!(capability.codecs, vec!["avi"]);
        let error = provider
            .decode(&DecodeRequest {
                kind: DecodeKind::Video,
                codec: "avi".into(),
                bytes: Vec::<u8>::new().into(),
                profile: "astra.manager.preview.v1".into(),
            })
            .expect_err("empty movie must not produce a preview");
        let MediaError::Diagnostics(diagnostics) = error else {
            panic!("preview failure must remain a blocking diagnostic");
        };
        assert_eq!(
            diagnostics[0].code,
            "ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT"
        );
    }

    #[test]
    fn preview_input_budget_is_checked_before_container_parse() {
        assert!(validate_preview_input_len(1).is_ok());
        let error = validate_preview_input_len(MAX_PREVIEW_INPUT_BYTES + 1).unwrap_err();
        let MediaError::Diagnostics(diagnostics) = error else {
            panic!("preview input failure must remain a blocking diagnostic");
        };
        assert_eq!(
            diagnostics[0].code,
            "ASTRA_EMU_MINORI_AVI_PREVIEW_INPUT_LIMIT"
        );
    }

    #[test]
    fn video_dimension_budget_rejects_zero_oversized_and_overflowing_frames() {
        assert!(validate_video_dimensions(320, 180).is_ok());
        assert_eq!(
            validate_video_dimensions(0, 180).unwrap_err(),
            "ASTRA_EMU_MINORI_AVI_VIDEO_DIMENSIONS"
        );
        assert_eq!(
            validate_video_dimensions(16_385, 720).unwrap_err(),
            "ASTRA_EMU_MINORI_AVI_VIDEO_DIMENSIONS"
        );
        assert_eq!(
            validate_video_dimensions(16_384, 16_384).unwrap_err(),
            "ASTRA_EMU_MINORI_AVI_FRAME_BOUNDS"
        );
    }
}
