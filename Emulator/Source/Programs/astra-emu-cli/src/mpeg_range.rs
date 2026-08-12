//! Bounded, revision-pinned MPEG packet decoder for legacy VFS media.
//!
//! The decoder owns one VFS range buffer and a bounded packet queue. It never
//! materializes an encoded movie or decoded frame history.

use std::{
    collections::VecDeque,
    io::{Read, Seek, SeekFrom},
};

use astra_byte_source::BoundedByteSourceReader;
use na_mpeg2_decoder::{MpegAudioF32, MpegAvEvent, MpegAvPipeline, MpegRgbaFrame};

pub const MPEG_RANGE_CHUNK_BYTES: usize = 64 * 1024;
const MAX_PENDING_EVENTS: usize = 256;

pub enum MpegRangeEvent {
    Video(MpegRgbaFrame),
    Audio(MpegAudioF32),
    End,
}

/// Aggregate codec evidence captured while reading a bounded movie stream.
/// It deliberately contains only counters; no encoded bytes, timestamps, or
/// frame payloads are retained in diagnostics or reports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MpegRangeTelemetry {
    pub input_bytes: u64,
    pub video_pes_start_codes: u64,
    pub sequence_headers: u64,
    pub picture_headers: u64,
    pub slice_start_codes: u64,
    pub mpeg4_visual_sequence_headers: u64,
    pub vc1_sequence_headers: u64,
    pub other_start_codes: u64,
    pub video_events: u64,
    pub audio_events: u64,
}

#[derive(Default)]
struct StartCodeObserver {
    tail: [u8; 3],
    tail_len: usize,
    telemetry: MpegRangeTelemetry,
}

impl StartCodeObserver {
    fn observe(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.telemetry.input_bytes = self
            .telemetry
            .input_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| "ASTRA_EMU_MINORI_MPEG_TELEMETRY_OVERFLOW".to_owned())?;
        if bytes.is_empty() {
            return Ok(());
        }
        let mut combined = Vec::with_capacity(self.tail_len + bytes.len());
        combined.extend_from_slice(&self.tail[..self.tail_len]);
        combined.extend_from_slice(bytes);
        if combined.len() >= 4 {
            let first_new_start = self.tail_len.saturating_sub(2);
            for index in first_new_start..=combined.len() - 4 {
                if combined[index..index + 3] != [0, 0, 1] {
                    continue;
                }
                self.record_start_code(combined[index + 3])?;
            }
        }
        self.tail_len = combined.len().min(self.tail.len());
        let start = combined.len() - self.tail_len;
        self.tail[..self.tail_len].copy_from_slice(&combined[start..]);
        Ok(())
    }

    fn record_start_code(&mut self, code: u8) -> Result<(), String> {
        let counter = if (0xE0..=0xEF).contains(&code) {
            &mut self.telemetry.video_pes_start_codes
        } else if code == 0xB3 {
            &mut self.telemetry.sequence_headers
        } else if code == 0x00 {
            &mut self.telemetry.picture_headers
        } else if code == 0x0F {
            &mut self.telemetry.vc1_sequence_headers
        } else if (0x01..=0xAF).contains(&code) {
            &mut self.telemetry.slice_start_codes
        } else if code == 0xB0 {
            &mut self.telemetry.mpeg4_visual_sequence_headers
        } else {
            &mut self.telemetry.other_start_codes
        };
        *counter = counter
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_MINORI_MPEG_TELEMETRY_OVERFLOW".to_owned())?;
        Ok(())
    }
}

pub struct MpegRangeDecoder {
    reader: BoundedByteSourceReader,
    pipeline: MpegAvPipeline,
    pending: VecDeque<MpegAvEvent>,
    input: [u8; MPEG_RANGE_CHUNK_BYTES],
    eof: bool,
    flushed: bool,
    observer: StartCodeObserver,
}

impl MpegRangeDecoder {
    pub fn new(mut reader: BoundedByteSourceReader, container_offset: u64) -> Result<Self, String> {
        let length = reader.stat().len;
        if container_offset >= length {
            return Err("ASTRA_EMU_MINORI_MPEG_CONTAINER_OFFSET".into());
        }
        reader
            .seek(SeekFrom::Start(container_offset))
            .map_err(|_| "ASTRA_EMU_MINORI_MPEG_CONTAINER_SEEK".to_owned())?;
        Ok(Self {
            reader,
            pipeline: MpegAvPipeline::new(),
            pending: VecDeque::new(),
            input: [0; MPEG_RANGE_CHUNK_BYTES],
            eof: false,
            flushed: false,
            observer: StartCodeObserver::default(),
        })
    }

    pub fn telemetry(&self) -> MpegRangeTelemetry {
        self.observer.telemetry
    }

    pub fn next_event(&mut self) -> Result<MpegRangeEvent, String> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(match event {
                    MpegAvEvent::Video(frame) => {
                        self.observer.telemetry.video_events = self
                            .observer
                            .telemetry
                            .video_events
                            .checked_add(1)
                            .ok_or_else(|| "ASTRA_EMU_MINORI_MPEG_TELEMETRY_OVERFLOW".to_owned())?;
                        MpegRangeEvent::Video(frame)
                    }
                    MpegAvEvent::Audio(chunk) => {
                        self.observer.telemetry.audio_events = self
                            .observer
                            .telemetry
                            .audio_events
                            .checked_add(1)
                            .ok_or_else(|| "ASTRA_EMU_MINORI_MPEG_TELEMETRY_OVERFLOW".to_owned())?;
                        MpegRangeEvent::Audio(chunk)
                    }
                });
            }
            if !self.eof {
                let read = self
                    .reader
                    .read(&mut self.input)
                    .map_err(|_| "ASTRA_EMU_MINORI_MPEG_SOURCE_READ".to_owned())?;
                if read == 0 {
                    self.eof = true;
                    continue;
                }
                self.observer.observe(&self.input[..read])?;
                self.pipeline
                    .push_with(&self.input[..read], None, |event| {
                        self.pending.push_back(event)
                    })
                    .map_err(|_| "ASTRA_EMU_MINORI_MPEG_DECODE".to_owned())?;
                self.check_pending()?;
                continue;
            }
            if !self.flushed {
                self.pipeline
                    .flush_with(|event| self.pending.push_back(event))
                    .map_err(|_| "ASTRA_EMU_MINORI_MPEG_DECODE".to_owned())?;
                self.flushed = true;
                self.check_pending()?;
                continue;
            }
            return Ok(MpegRangeEvent::End);
        }
    }

    fn check_pending(&self) -> Result<(), String> {
        if self.pending.len() > MAX_PENDING_EVENTS {
            return Err("ASTRA_EMU_MINORI_MPEG_EVENT_BUDGET".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use astra_byte_source::{BoundedByteSourceReader, FileByteSource};

    use super::{MpegRangeDecoder, StartCodeObserver};

    #[test]
    fn rejects_a_container_offset_at_or_after_the_pinned_source_end() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temporary.path(), b"mpeg").unwrap();
        let source = Arc::new(FileByteSource::open(temporary.path()).unwrap());
        let reader = BoundedByteSourceReader::new(source, 2).unwrap();

        let error = match MpegRangeDecoder::new(reader, 4) {
            Ok(_) => panic!("container offset at source end was accepted"),
            Err(error) => error,
        };
        assert_eq!(error, "ASTRA_EMU_MINORI_MPEG_CONTAINER_OFFSET");
    }

    #[test]
    fn telemetry_classifies_start_codes_across_read_boundaries() {
        let mut observer = StartCodeObserver::default();
        observer.observe(&[0, 0]).unwrap();
        observer
            .observe(&[1, 0xE0, 0, 0, 1, 0xB3, 0, 0, 1, 0x00, 0, 0, 1, 0x01])
            .unwrap();
        observer
            .observe(&[0, 0, 1, 0xB0, 0, 0, 1, 0x0F, 0, 0, 1, 0xBA])
            .unwrap();
        assert_eq!(
            observer.telemetry,
            super::MpegRangeTelemetry {
                input_bytes: 2 + 14 + 12,
                video_pes_start_codes: 1,
                sequence_headers: 1,
                picture_headers: 1,
                slice_start_codes: 1,
                mpeg4_visual_sequence_headers: 1,
                vc1_sequence_headers: 1,
                other_start_codes: 1,
                ..super::MpegRangeTelemetry::default()
            }
        );
    }
}
