use super::{
    codec::{invalid, read_header},
    VERSION,
};
use std::io::{self, Read, Write};

const MAGIC: &[u8; 4] = b"AKRH";
const BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Lane {
    Control = 1,
    Pcm = 2,
    Frame = 3,
}

pub fn write_handshake(writer: &mut impl Write, session: u64, lane: Lane) -> io::Result<()> {
    if session == 0 {
        return Err(invalid("zero session"));
    }
    let mut bytes = [0; BYTES];
    bytes[..4].copy_from_slice(MAGIC);
    bytes[4..6].copy_from_slice(&VERSION.to_le_bytes());
    bytes[6..8].copy_from_slice(&(lane as u16).to_le_bytes());
    bytes[8..16].copy_from_slice(&session.to_le_bytes());
    writer.write_all(&bytes)?;
    writer.flush()
}

pub fn read_handshake(reader: &mut impl Read, session: u64) -> io::Result<Lane> {
    let bytes = read_header::<BYTES>(reader)?.ok_or_else(|| invalid("handshake EOF"))?;
    if session == 0
        || &bytes[..4] != MAGIC
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != VERSION
        || u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != session
    {
        return Err(invalid("handshake identity or version"));
    }
    match u16::from_le_bytes(bytes[6..8].try_into().unwrap()) {
        1 => Ok(Lane::Control),
        2 => Ok(Lane::Pcm),
        3 => Ok(Lane::Frame),
        _ => Err(invalid("unknown lane")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handshakes_reject_partial_unknown_and_other_session() {
        let mut bytes = Vec::new();
        write_handshake(&mut bytes, 8, Lane::Pcm).unwrap();
        assert_eq!(read_handshake(&mut bytes.as_slice(), 8).unwrap(), Lane::Pcm);
        assert!(read_handshake(&mut bytes.as_slice(), 9).is_err());
        for end in 0..BYTES {
            assert!(read_handshake(&mut &bytes[..end], 8).is_err());
        }
        bytes[6] = 4;
        assert!(read_handshake(&mut bytes.as_slice(), 8).is_err());
    }
}
