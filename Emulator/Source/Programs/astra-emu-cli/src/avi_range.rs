//! Range-backed adapter for the shared Minori AVI decoder.

use astra_byte_source::BoundedByteSourceReader;
use astra_emu_minori::{MinoriAviDecoder, MinoriAviEvent, MinoriAviTelemetry};
use na_mpeg2_decoder::MpegAudioF32;

pub type AviRangeTelemetry = MinoriAviTelemetry;

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
    inner: MinoriAviDecoder<BoundedByteSourceReader>,
}

impl AviRangeDecoder {
    pub fn new(reader: BoundedByteSourceReader) -> Result<Self, String> {
        Ok(Self {
            inner: MinoriAviDecoder::new(reader)?,
        })
    }

    pub fn telemetry(&self) -> AviRangeTelemetry {
        self.inner.telemetry()
    }

    pub fn duration_us(&self) -> u64 {
        self.inner.duration_us()
    }

    pub fn next_event(&mut self) -> Result<AviRangeEvent, String> {
        match self.inner.next_event()? {
            MinoriAviEvent::Video {
                pts_us,
                width,
                height,
                bgra8,
            } => Ok(AviRangeEvent::Video {
                pts_us,
                width,
                height,
                bgra8,
            }),
            MinoriAviEvent::Audio {
                pts_ms,
                sample_rate,
                channels,
                samples,
            } => Ok(AviRangeEvent::Audio(MpegAudioF32 {
                pts_ms,
                sample_rate,
                channels,
                samples,
            })),
            MinoriAviEvent::End => Ok(AviRangeEvent::End),
        }
    }
}
