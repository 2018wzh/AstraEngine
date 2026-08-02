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

pub struct MpegRangeDecoder {
    reader: BoundedByteSourceReader,
    pipeline: MpegAvPipeline,
    pending: VecDeque<MpegAvEvent>,
    input: [u8; MPEG_RANGE_CHUNK_BYTES],
    eof: bool,
    flushed: bool,
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
        })
    }

    pub fn next_event(&mut self) -> Result<MpegRangeEvent, String> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(match event {
                    MpegAvEvent::Video(frame) => MpegRangeEvent::Video(frame),
                    MpegAvEvent::Audio(chunk) => MpegRangeEvent::Audio(chunk),
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

    use super::MpegRangeDecoder;

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
}
