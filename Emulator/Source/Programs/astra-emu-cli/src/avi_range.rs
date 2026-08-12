//! Bounded Minori AVI playback adapter.

use astra_byte_source::BoundedByteSourceReader;
use na_mpeg2_decoder::MpegAudioF32;
use wmv_decoder::{AviDemuxer, AviPacketKind, AviStreamFormat, DecoderError, Wmv3Decoder};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AviRangeTelemetry {
    pub encoded_bytes: u64,
    pub video_packets: u64,
    pub dropped_video_packets: u64,
    pub audio_packets: u64,
    pub decoded_frames: u64,
    pub decoded_audio_samples: u64,
}

pub enum AviRangeEvent {
    Video {
        pts_us: u64,
        width: u32,
        height: u32,
        bgra8: Vec<u8>,
    },
    Audio(MpegAudioF32),
    End,
}

pub struct AviRangeDecoder {
    demuxer: AviDemuxer<BoundedByteSourceReader>,
    video_stream: usize,
    audio_stream: Option<usize>,
    video: Wmv3Decoder,
    audio_sample_rate: Option<u32>,
    audio_channels: Option<u16>,
    ended: bool,
    duration_us: u64,
    telemetry: AviRangeTelemetry,
}

impl AviRangeDecoder {
    pub fn new(reader: BoundedByteSourceReader) -> Result<Self, String> {
        let demuxer =
            AviDemuxer::open(reader).map_err(|_| "ASTRA_EMU_MINORI_AVI_HEADER".to_owned())?;
        let mut video_stream = None;
        let mut audio_stream = None;
        let mut video = None;
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
                    video_stream = Some(stream.index);
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
            audio_sample_rate,
            audio_channels,
            ended: false,
            duration_us,
            telemetry: AviRangeTelemetry::default(),
        })
    }

    pub fn telemetry(&self) -> AviRangeTelemetry {
        self.telemetry
    }

    pub fn duration_us(&self) -> u64 {
        self.duration_us
    }

    pub fn next_event(&mut self) -> Result<AviRangeEvent, String> {
        if self.ended {
            return Ok(AviRangeEvent::End);
        }
        loop {
            let Some(packet) = self
                .demuxer
                .next_packet()
                .map_err(|_| "ASTRA_EMU_MINORI_AVI_DEMUX".to_owned())?
            else {
                self.ended = true;
                return Ok(AviRangeEvent::End);
            };
            self.telemetry.encoded_bytes = self
                .telemetry
                .encoded_bytes
                .checked_add(packet.data.len() as u64)
                .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
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
                                error = %error,
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
                    self.telemetry.decoded_frames = self
                        .telemetry
                        .decoded_frames
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    return Ok(AviRangeEvent::Video {
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
                        .chunks_exact(2)
                        .map(|bytes| f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0)
                        .collect::<Vec<_>>();
                    self.telemetry.audio_packets = self
                        .telemetry
                        .audio_packets
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    self.telemetry.decoded_audio_samples = self
                        .telemetry
                        .decoded_audio_samples
                        .checked_add(samples.len() as u64)
                        .ok_or_else(|| "ASTRA_EMU_MINORI_AVI_TELEMETRY_OVERFLOW".to_owned())?;
                    return Ok(AviRangeEvent::Audio(MpegAudioF32 {
                        pts_ms: i64::try_from(packet.pts_us / 1_000)
                            .map_err(|_| "ASTRA_EMU_MINORI_AVI_AUDIO_TIMELINE".to_owned())?,
                        sample_rate,
                        channels,
                        samples,
                    }));
                }
                _ => {
                    return Err("ASTRA_EMU_MINORI_AVI_PACKET_STREAM".into());
                }
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
