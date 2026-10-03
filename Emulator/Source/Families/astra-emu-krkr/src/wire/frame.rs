use super::{
    codec::{invalid, read_header},
    Identity, MAX_FRAME_BYTES, VERSION,
};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

const HEADER_BYTES: usize = 48;
const MAGIC: &[u8; 4] = b"AKRF";

/// Opaque RGBA8 sRGB. Raster and logical stage may have different aspect
/// ratios under v10's centered content viewport rule. No GPU/native handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub stride: u32,
}

impl FrameInfo {
    pub fn required_bytes(self) -> Result<usize, &'static str> {
        if self.width == 0
            || self.height == 0
            || self.logical_width == 0
            || self.logical_height == 0
        {
            return Err("zero frame dimension");
        }
        let row = self.width.checked_mul(4).ok_or("frame row overflow")?;
        if self.stride < row {
            return Err("frame stride");
        }
        let len = u64::from(self.stride)
            .checked_mul(u64::from(self.height))
            .ok_or("frame size overflow")?;
        if len > MAX_FRAME_BYTES as u64 {
            return Err("frame byte bound exceeded");
        }
        usize::try_from(len).map_err(|_| "frame does not fit address space")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalFrame {
    pub identity: Identity,
    pub info: FrameInfo,
    pub pixels: Vec<u8>,
}

pub fn write_frame(
    writer: &mut impl Write,
    identity: Identity,
    info: FrameInfo,
    pixels: &[u8],
) -> io::Result<()> {
    identity.validate().map_err(invalid)?;
    let len = info.required_bytes().map_err(invalid)?;
    if pixels.len() != len {
        return Err(invalid("final frame byte length"));
    }
    let mut header = [0; HEADER_BYTES];
    header[..4].copy_from_slice(MAGIC);
    header[4..6].copy_from_slice(&VERSION.to_le_bytes());
    header[8..16].copy_from_slice(&identity.session.to_le_bytes());
    header[16..24].copy_from_slice(&identity.sequence.to_le_bytes());
    for (offset, value) in [
        (24, info.width),
        (28, info.height),
        (32, info.logical_width),
        (36, info.logical_height),
        (40, info.stride),
        (44, len as u32),
    ] {
        header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    writer.write_all(&header)?;
    writer.write_all(pixels)?;
    writer.flush()
}

pub fn read_frame(reader: &mut impl Read, expected: Identity) -> io::Result<Option<FinalFrame>> {
    let Some(header) = read_header::<HEADER_BYTES>(reader)? else {
        return Ok(None);
    };
    if &header[..4] != MAGIC
        || u16::from_le_bytes(header[4..6].try_into().unwrap()) != VERSION
        || header[6..8] != [0, 0]
    {
        return Err(invalid("frame magic, version or reserved flags"));
    }
    let identity = Identity {
        session: u64::from_le_bytes(header[8..16].try_into().unwrap()),
        sequence: u64::from_le_bytes(header[16..24].try_into().unwrap()),
    };
    identity.validate().map_err(invalid)?;
    if identity != expected {
        return Err(invalid("stale frame identity"));
    }
    let get = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
    let info = FrameInfo {
        width: get(24),
        height: get(28),
        logical_width: get(32),
        logical_height: get(36),
        stride: get(40),
    };
    let len = info.required_bytes().map_err(invalid)?;
    if get(44) as usize != len {
        return Err(invalid("frame length does not match stride"));
    }
    let mut pixels = vec![0; len];
    reader.read_exact(&mut pixels)?;
    Ok(Some(FinalFrame {
        identity,
        info,
        pixels,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_frame_roundtrip_and_partial_reads() {
        let identity = Identity {
            session: 5,
            sequence: 8,
        };
        let info = FrameInfo {
            width: 2,
            height: 1,
            logical_width: 4,
            logical_height: 2,
            stride: 8,
        };
        let pixels = vec![3; 8];
        let mut bytes = Vec::new();
        write_frame(&mut bytes, identity, info, &pixels).unwrap();
        let frame = read_frame(&mut bytes.as_slice(), identity)
            .unwrap()
            .unwrap();
        assert_eq!(
            frame,
            FinalFrame {
                identity,
                info,
                pixels
            }
        );
        assert!(read_frame(
            &mut bytes.as_slice(),
            Identity {
                session: 4,
                ..identity
            }
        )
        .is_err());
        for end in 1..bytes.len() {
            assert!(read_frame(&mut &bytes[..end], identity).is_err());
        }
        bytes[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut &bytes[..HEADER_BYTES], identity).is_err());
    }

    #[test]
    fn geometry_stride_and_bulk_bounds_are_checked() {
        let valid = FrameInfo {
            width: 1920,
            height: 1080,
            logical_width: 1280,
            logical_height: 720,
            stride: 7680,
        };
        assert_eq!(valid.required_bytes().unwrap(), 8294400);
        for invalid_info in [
            FrameInfo { width: 0, ..valid },
            FrameInfo { stride: 1, ..valid },
            FrameInfo {
                stride: u32::MAX,
                ..valid
            },
        ] {
            assert!(invalid_info.required_bytes().is_err());
        }
        assert!(FrameInfo {
            logical_width: 1281,
            ..valid
        }
        .required_bytes()
        .is_ok());
    }
}
