//! Bounded RIFF/AVI demuxer used by legacy engine media adapters.
//!
//! The demuxer keeps the source seekable and reads one `movi` packet at a
//! time. It does not retain `idx1`, encoded frame history, or the whole file.

use std::io::{Read, Seek, SeekFrom};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{DecoderError, Result};

const MAX_AVI_HEADER_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const MAX_AVI_PACKET_BYTES: u64 = 64 * 1024 * 1024;
const MAX_AVI_STREAMS: usize = 16;
const MAX_AVI_LIST_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AviVideoInfo {
    pub width: u32,
    pub height: u32,
    pub compression: [u8; 4],
    pub bits_per_pixel: u16,
    pub extra_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AviAudioInfo {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub average_bytes_per_second: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub extra_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AviStreamFormat {
    Video(AviVideoInfo),
    Audio(AviAudioInfo),
    Other { stream_type: [u8; 4] },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AviStreamInfo {
    pub index: usize,
    pub handler: [u8; 4],
    pub scale: u32,
    pub rate: u32,
    pub start: u32,
    pub length: u32,
    pub sample_size: u32,
    pub format: AviStreamFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AviPacketKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AviPacket {
    pub stream_index: usize,
    pub kind: AviPacketKind,
    pub pts_us: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct ChunkHeader {
    id: [u8; 4],
    data_start: u64,
    data_end: u64,
    padded_end: u64,
}

#[derive(Debug, Clone, Copy)]
struct ListCursor {
    data_end: u64,
    padded_end: u64,
}

#[derive(Debug, Clone)]
struct RawStreamHeader {
    stream_type: [u8; 4],
    handler: [u8; 4],
    scale: u32,
    rate: u32,
    start: u32,
    length: u32,
    sample_size: u32,
}

/// Streaming RIFF/AVI demuxer with strict parent and packet bounds.
pub struct AviDemuxer<R> {
    reader: R,
    streams: Vec<AviStreamInfo>,
    stream_units: Vec<u64>,
    lists: Vec<ListCursor>,
}

impl<R: Read + Seek> AviDemuxer<R> {
    pub fn open(mut reader: R) -> Result<Self> {
        let file_length = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; 12];
        reader.read_exact(&mut header)?;
        if &header[..4] != b"RIFF" || &header[8..] != b"AVI " {
            return Err(DecoderError::InvalidData("not a RIFF/AVI stream".into()));
        }
        let riff_size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64;
        let riff_end = 8u64
            .checked_add(riff_size)
            .ok_or_else(|| DecoderError::InvalidData("AVI RIFF size overflow".into()))?;
        if riff_end > file_length || riff_end < 12 {
            return Err(DecoderError::InvalidData(
                "AVI RIFF size exceeds the pinned source".into(),
            ));
        }

        let mut streams = None;
        let mut movi = None;
        reader.seek(SeekFrom::Start(12))?;
        while let Some(chunk) = read_chunk_header(&mut reader, riff_end)? {
            if chunk.id == *b"LIST" {
                let list_type = read_fourcc(&mut reader, chunk.data_start, chunk.data_end)?;
                match &list_type {
                    b"hdrl" => {
                        if streams.is_some() {
                            return Err(DecoderError::InvalidData(
                                "AVI contains duplicate hdrl lists".into(),
                            ));
                        }
                        streams = Some(parse_hdrl(
                            &mut reader,
                            chunk.data_start + 4,
                            chunk.data_end,
                        )?);
                    }
                    b"movi" => {
                        if movi.is_some() {
                            return Err(DecoderError::InvalidData(
                                "AVI contains duplicate movi lists".into(),
                            ));
                        }
                        movi = Some(ListCursor {
                            data_end: chunk.data_end,
                            padded_end: chunk.padded_end,
                        });
                    }
                    _ => {}
                }
            }
            reader.seek(SeekFrom::Start(chunk.padded_end))?;
        }

        let streams = streams
            .ok_or_else(|| DecoderError::InvalidData("AVI is missing its hdrl list".into()))?;
        let movi =
            movi.ok_or_else(|| DecoderError::InvalidData("AVI is missing its movi list".into()))?;
        if streams.is_empty() || streams.len() > MAX_AVI_STREAMS {
            return Err(DecoderError::InvalidData(
                "AVI stream count violates the configured bound".into(),
            ));
        }
        let movi_start = find_movi_start(&mut reader, riff_end)?;
        reader.seek(SeekFrom::Start(movi_start))?;
        Ok(Self {
            stream_units: vec![0; streams.len()],
            streams,
            lists: vec![movi],
            reader,
        })
    }

    pub fn streams(&self) -> &[AviStreamInfo] {
        &self.streams
    }

    pub fn next_packet(&mut self) -> Result<Option<AviPacket>> {
        loop {
            let Some(parent) = self.lists.last().copied() else {
                return Ok(None);
            };
            let position = self.reader.stream_position()?;
            if position >= parent.data_end {
                self.reader.seek(SeekFrom::Start(parent.padded_end))?;
                self.lists.pop();
                continue;
            }
            let Some(chunk) = read_chunk_header(&mut self.reader, parent.data_end)? else {
                self.reader.seek(SeekFrom::Start(parent.padded_end))?;
                self.lists.pop();
                continue;
            };
            if chunk.id == *b"LIST" {
                if self.lists.len() >= MAX_AVI_LIST_DEPTH {
                    return Err(DecoderError::InvalidData(
                        "AVI nested list depth exceeds the configured bound".into(),
                    ));
                }
                let list_type = read_fourcc(&mut self.reader, chunk.data_start, chunk.data_end)?;
                if matches!(&list_type, b"rec " | b"movi") {
                    self.reader.seek(SeekFrom::Start(chunk.data_start + 4))?;
                    self.lists.push(ListCursor {
                        data_end: chunk.data_end,
                        padded_end: chunk.padded_end,
                    });
                } else {
                    self.reader.seek(SeekFrom::Start(chunk.padded_end))?;
                }
                continue;
            }
            let Some((stream_index, kind)) = packet_identity(chunk.id) else {
                self.reader.seek(SeekFrom::Start(chunk.padded_end))?;
                continue;
            };
            let stream = self.streams.get(stream_index).ok_or_else(|| {
                DecoderError::InvalidData("AVI packet references an unknown stream".into())
            })?;
            let expected_kind = match stream.format {
                AviStreamFormat::Video(_) => AviPacketKind::Video,
                AviStreamFormat::Audio(_) => AviPacketKind::Audio,
                AviStreamFormat::Other { .. } => {
                    self.reader.seek(SeekFrom::Start(chunk.padded_end))?;
                    continue;
                }
            };
            if kind != expected_kind {
                return Err(DecoderError::InvalidData(
                    "AVI packet kind conflicts with its stream header".into(),
                ));
            }
            let size = chunk.data_end - chunk.data_start;
            if size > MAX_AVI_PACKET_BYTES {
                return Err(DecoderError::InvalidData(
                    "AVI packet exceeds the configured bound".into(),
                ));
            }
            let mut data = vec![
                0u8;
                usize::try_from(size).map_err(|_| {
                    DecoderError::InvalidData("AVI packet size exceeds the platform bound".into())
                })?
            ];
            self.reader.seek(SeekFrom::Start(chunk.data_start))?;
            self.reader.read_exact(&mut data)?;
            self.reader.seek(SeekFrom::Start(chunk.padded_end))?;

            let units = self.stream_units[stream_index];
            let pts_units = u64::from(stream.start)
                .checked_add(units)
                .ok_or_else(|| DecoderError::InvalidData("AVI timestamp overflow".into()))?;
            let pts_us = pts_units
                .checked_mul(u64::from(stream.scale))
                .and_then(|value| value.checked_mul(1_000_000))
                .map(|value| value / u64::from(stream.rate))
                .ok_or_else(|| DecoderError::InvalidData("AVI timestamp overflow".into()))?;
            let consumed_units = match kind {
                AviPacketKind::Video => 1,
                AviPacketKind::Audio => {
                    if stream.sample_size == 0 || size % u64::from(stream.sample_size) != 0 {
                        return Err(DecoderError::InvalidData(
                            "AVI audio packet is not sample aligned".into(),
                        ));
                    }
                    size / u64::from(stream.sample_size)
                }
            };
            self.stream_units[stream_index] = units
                .checked_add(consumed_units)
                .ok_or_else(|| DecoderError::InvalidData("AVI stream length overflow".into()))?;
            return Ok(Some(AviPacket {
                stream_index,
                kind,
                pts_us,
                data,
            }));
        }
    }
}

fn find_movi_start<R: Read + Seek>(reader: &mut R, riff_end: u64) -> Result<u64> {
    reader.seek(SeekFrom::Start(12))?;
    while let Some(chunk) = read_chunk_header(reader, riff_end)? {
        if chunk.id == *b"LIST"
            && read_fourcc(reader, chunk.data_start, chunk.data_end)? == *b"movi"
        {
            return chunk
                .data_start
                .checked_add(4)
                .ok_or_else(|| DecoderError::InvalidData("AVI movi offset overflow".into()));
        }
        reader.seek(SeekFrom::Start(chunk.padded_end))?;
    }
    Err(DecoderError::InvalidData(
        "AVI is missing its movi list".into(),
    ))
}

fn parse_hdrl<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> Result<Vec<AviStreamInfo>> {
    reader.seek(SeekFrom::Start(start))?;
    let mut streams = Vec::new();
    while let Some(chunk) = read_chunk_header(reader, end)? {
        if chunk.id == *b"LIST"
            && read_fourcc(reader, chunk.data_start, chunk.data_end)? == *b"strl"
        {
            if streams.len() >= MAX_AVI_STREAMS {
                return Err(DecoderError::InvalidData(
                    "AVI stream count exceeds the configured bound".into(),
                ));
            }
            streams.push(parse_strl(
                reader,
                chunk.data_start + 4,
                chunk.data_end,
                streams.len(),
            )?);
        }
        reader.seek(SeekFrom::Start(chunk.padded_end))?;
    }
    Ok(streams)
}

fn parse_strl<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    end: u64,
    index: usize,
) -> Result<AviStreamInfo> {
    reader.seek(SeekFrom::Start(start))?;
    let mut stream_header = None;
    let mut stream_format = None;
    while let Some(chunk) = read_chunk_header(reader, end)? {
        let size = chunk.data_end - chunk.data_start;
        if size > MAX_AVI_HEADER_CHUNK_BYTES {
            return Err(DecoderError::InvalidData(
                "AVI stream header chunk exceeds the configured bound".into(),
            ));
        }
        if chunk.id == *b"strh" {
            if stream_header.is_some() {
                return Err(DecoderError::InvalidData(
                    "AVI stream contains duplicate strh chunks".into(),
                ));
            }
            let bytes = read_chunk_bytes(reader, chunk)?;
            stream_header = Some(parse_stream_header(&bytes)?);
        } else if chunk.id == *b"strf" {
            if stream_format.is_some() {
                return Err(DecoderError::InvalidData(
                    "AVI stream contains duplicate strf chunks".into(),
                ));
            }
            stream_format = Some(read_chunk_bytes(reader, chunk)?);
        }
        reader.seek(SeekFrom::Start(chunk.padded_end))?;
    }
    let header = stream_header
        .ok_or_else(|| DecoderError::InvalidData("AVI stream is missing its strh chunk".into()))?;
    if header.scale == 0 || header.rate == 0 {
        return Err(DecoderError::InvalidData(
            "AVI stream has an invalid time base".into(),
        ));
    }
    let bytes = stream_format
        .ok_or_else(|| DecoderError::InvalidData("AVI stream is missing its strf chunk".into()))?;
    let format = match &header.stream_type {
        b"vids" => AviStreamFormat::Video(parse_video_format(&bytes)?),
        b"auds" => AviStreamFormat::Audio(parse_audio_format(&bytes)?),
        stream_type => AviStreamFormat::Other {
            stream_type: *stream_type,
        },
    };
    Ok(AviStreamInfo {
        index,
        handler: header.handler,
        scale: header.scale,
        rate: header.rate,
        start: header.start,
        length: header.length,
        sample_size: header.sample_size,
        format,
    })
}

fn parse_stream_header(bytes: &[u8]) -> Result<RawStreamHeader> {
    if bytes.len() < 48 {
        return Err(DecoderError::InvalidData(
            "AVI strh chunk is truncated".into(),
        ));
    }
    Ok(RawStreamHeader {
        stream_type: bytes[0..4].try_into().unwrap(),
        handler: bytes[4..8].try_into().unwrap(),
        scale: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
        rate: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        start: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        length: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
        sample_size: u32::from_le_bytes(bytes[44..48].try_into().unwrap()),
    })
}

fn parse_video_format(bytes: &[u8]) -> Result<AviVideoInfo> {
    if bytes.len() < 40 {
        return Err(DecoderError::InvalidData(
            "AVI video strf chunk is truncated".into(),
        ));
    }
    let header_size = usize::try_from(u32::from_le_bytes(bytes[0..4].try_into().unwrap()))
        .map_err(|_| DecoderError::InvalidData("AVI bitmap header is oversized".into()))?;
    if !(40..=bytes.len()).contains(&header_size) {
        return Err(DecoderError::InvalidData(
            "AVI bitmap header size is invalid".into(),
        ));
    }
    let width = i32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let height = i32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if width <= 0 || height == 0 || height == i32::MIN {
        return Err(DecoderError::InvalidData(
            "AVI video dimensions are invalid".into(),
        ));
    }
    Ok(AviVideoInfo {
        width: u32::try_from(width).unwrap(),
        height: height.unsigned_abs(),
        bits_per_pixel: u16::from_le_bytes(bytes[14..16].try_into().unwrap()),
        compression: bytes[16..20].try_into().unwrap(),
        extra_data: bytes[40..header_size].to_vec(),
    })
}

fn parse_audio_format(bytes: &[u8]) -> Result<AviAudioInfo> {
    if bytes.len() < 16 {
        return Err(DecoderError::InvalidData(
            "AVI audio strf chunk is truncated".into(),
        ));
    }
    let extra_size = if bytes.len() >= 18 {
        usize::from(u16::from_le_bytes(bytes[16..18].try_into().unwrap()))
    } else {
        0
    };
    let extra_end = 18usize
        .checked_add(extra_size)
        .ok_or_else(|| DecoderError::InvalidData("AVI audio format size overflow".into()))?;
    if extra_size > 0 && extra_end > bytes.len() {
        return Err(DecoderError::InvalidData(
            "AVI audio format extra data is truncated".into(),
        ));
    }
    Ok(AviAudioInfo {
        format_tag: u16::from_le_bytes(bytes[0..2].try_into().unwrap()),
        channels: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
        sample_rate: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        average_bytes_per_second: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        block_align: u16::from_le_bytes(bytes[12..14].try_into().unwrap()),
        bits_per_sample: u16::from_le_bytes(bytes[14..16].try_into().unwrap()),
        extra_data: if extra_size == 0 {
            Vec::new()
        } else {
            bytes[18..extra_end].to_vec()
        },
    })
}

fn read_chunk_header<R: Read + Seek>(
    reader: &mut R,
    parent_end: u64,
) -> Result<Option<ChunkHeader>> {
    let start = reader.stream_position()?;
    if start == parent_end {
        return Ok(None);
    }
    let header_end = start
        .checked_add(8)
        .ok_or_else(|| DecoderError::InvalidData("AVI chunk offset overflow".into()))?;
    if header_end > parent_end {
        return Err(DecoderError::InvalidData(
            "AVI chunk header crosses its parent bound".into(),
        ));
    }
    let mut id = [0u8; 4];
    reader.read_exact(&mut id)?;
    let size = u64::from(reader.read_u32::<LittleEndian>()?);
    let data_start = header_end;
    let data_end = data_start
        .checked_add(size)
        .ok_or_else(|| DecoderError::InvalidData("AVI chunk size overflow".into()))?;
    let padded_end = data_end
        .checked_add(size & 1)
        .ok_or_else(|| DecoderError::InvalidData("AVI chunk padding overflow".into()))?;
    if padded_end > parent_end {
        return Err(DecoderError::InvalidData(
            "AVI chunk crosses its parent bound".into(),
        ));
    }
    Ok(Some(ChunkHeader {
        id,
        data_start,
        data_end,
        padded_end,
    }))
}

fn read_fourcc<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> Result<[u8; 4]> {
    if start
        .checked_add(4)
        .is_none_or(|fourcc_end| fourcc_end > end)
    {
        return Err(DecoderError::InvalidData(
            "AVI LIST type is truncated".into(),
        ));
    }
    reader.seek(SeekFrom::Start(start))?;
    let mut fourcc = [0u8; 4];
    reader.read_exact(&mut fourcc)?;
    Ok(fourcc)
}

fn read_chunk_bytes<R: Read + Seek>(reader: &mut R, chunk: ChunkHeader) -> Result<Vec<u8>> {
    let size = usize::try_from(chunk.data_end - chunk.data_start)
        .map_err(|_| DecoderError::InvalidData("AVI header exceeds the platform bound".into()))?;
    let mut bytes = vec![0u8; size];
    reader.seek(SeekFrom::Start(chunk.data_start))?;
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn packet_identity(id: [u8; 4]) -> Option<(usize, AviPacketKind)> {
    if !id[0].is_ascii_digit() || !id[1].is_ascii_digit() {
        return None;
    }
    let index = usize::from(id[0] - b'0') * 10 + usize::from(id[1] - b'0');
    let kind = match &id[2..] {
        b"dc" | b"db" => AviPacketKind::Video,
        b"wb" => AviPacketKind::Audio,
        _ => return None,
    };
    Some((index, kind))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{AviDemuxer, AviPacketKind, AviStreamFormat};

    fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn list(kind: &[u8; 4], children: &[u8]) -> Vec<u8> {
        let mut payload = kind.to_vec();
        payload.extend_from_slice(children);
        chunk(b"LIST", &payload)
    }

    fn stream_header(
        kind: &[u8; 4],
        handler: &[u8; 4],
        scale: u32,
        rate: u32,
        sample_size: u32,
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; 56];
        bytes[..4].copy_from_slice(kind);
        bytes[4..8].copy_from_slice(handler);
        bytes[20..24].copy_from_slice(&scale.to_le_bytes());
        bytes[24..28].copy_from_slice(&rate.to_le_bytes());
        bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&sample_size.to_le_bytes());
        bytes
    }

    fn fixture() -> Vec<u8> {
        let mut video_format = vec![0u8; 44];
        video_format[..4].copy_from_slice(&44u32.to_le_bytes());
        video_format[4..8].copy_from_slice(&16i32.to_le_bytes());
        video_format[8..12].copy_from_slice(&16i32.to_le_bytes());
        video_format[12..14].copy_from_slice(&1u16.to_le_bytes());
        video_format[14..16].copy_from_slice(&24u16.to_le_bytes());
        video_format[16..20].copy_from_slice(b"WMV3");
        video_format[40..44].copy_from_slice(&[1, 2, 3, 4]);
        let video = list(
            b"strl",
            &[
                chunk(b"strh", &stream_header(b"vids", b"wmv3", 1, 24, 0)),
                chunk(b"strf", &video_format),
            ]
            .concat(),
        );

        let mut audio_format = Vec::new();
        audio_format.extend_from_slice(&1u16.to_le_bytes());
        audio_format.extend_from_slice(&2u16.to_le_bytes());
        audio_format.extend_from_slice(&48_000u32.to_le_bytes());
        audio_format.extend_from_slice(&192_000u32.to_le_bytes());
        audio_format.extend_from_slice(&4u16.to_le_bytes());
        audio_format.extend_from_slice(&16u16.to_le_bytes());
        let audio = list(
            b"strl",
            &[
                chunk(
                    b"strh",
                    &stream_header(b"auds", &[1, 0, 0, 0], 4, 192_000, 4),
                ),
                chunk(b"strf", &audio_format),
            ]
            .concat(),
        );
        let hdrl = list(b"hdrl", &[video, audio].concat());
        let movi = list(
            b"movi",
            &[
                chunk(b"00dc", &[9, 8, 7]),
                chunk(b"00dc", &[]),
                chunk(b"01wb", &[0, 0, 1, 0, 2, 0, 3, 0]),
            ]
            .concat(),
        );
        let payload = [b"AVI ".as_slice(), hdrl.as_slice(), movi.as_slice()].concat();
        let mut riff = b"RIFF".to_vec();
        riff.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        riff.extend_from_slice(&payload);
        riff
    }

    #[test]
    fn parses_wmv3_pcm_headers_and_streams_bounded_packets() {
        let mut demuxer = AviDemuxer::open(Cursor::new(fixture())).unwrap();
        assert_eq!(demuxer.streams().len(), 2);
        let AviStreamFormat::Video(video) = &demuxer.streams()[0].format else {
            panic!("expected video stream");
        };
        assert_eq!(video.compression, *b"WMV3");
        assert_eq!(video.extra_data, [1, 2, 3, 4]);
        let video_packet = demuxer.next_packet().unwrap().unwrap();
        assert_eq!(video_packet.kind, AviPacketKind::Video);
        assert_eq!(video_packet.pts_us, 0);
        assert_eq!(video_packet.data, [9, 8, 7]);
        let dropped_video_packet = demuxer.next_packet().unwrap().unwrap();
        assert_eq!(dropped_video_packet.kind, AviPacketKind::Video);
        assert_eq!(dropped_video_packet.pts_us, 41_666);
        assert!(dropped_video_packet.data.is_empty());
        let audio_packet = demuxer.next_packet().unwrap().unwrap();
        assert_eq!(audio_packet.kind, AviPacketKind::Audio);
        assert_eq!(audio_packet.pts_us, 0);
        assert_eq!(audio_packet.data.len(), 8);
        assert!(demuxer.next_packet().unwrap().is_none());
    }

    #[test]
    fn rejects_packets_that_cross_the_movi_bound() {
        let mut bytes = fixture();
        bytes.pop();
        assert!(AviDemuxer::open(Cursor::new(bytes)).is_err());
    }
}
