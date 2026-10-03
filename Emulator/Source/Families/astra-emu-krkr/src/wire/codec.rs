use super::{ChildPacket, Identity, ParentPacket, MAX_PACKET_BYTES, VERSION};
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

const HEADER_BYTES: usize = 28;
const MAGIC: &[u8; 4] = b"AKRC";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PacketKind {
    Control = 1,
    Pcm = 2,
    Progress = 3,
}

pub trait Packet: Serialize + DeserializeOwned {
    fn kind(&self) -> PacketKind;
    fn validate(&self) -> Result<(), &'static str>;
}

impl Packet for ParentPacket {
    fn kind(&self) -> PacketKind {
        match self {
            Self::Command(_) => PacketKind::Control,
            Self::PcmReply { .. } => PacketKind::Pcm,
            Self::Progress(_) => PacketKind::Progress,
        }
    }
    fn validate(&self) -> Result<(), &'static str> {
        self.validate()
    }
}

impl Packet for ChildPacket {
    fn kind(&self) -> PacketKind {
        match self {
            Self::Reply(_) => PacketKind::Control,
            Self::PcmWrite(_) => PacketKind::Pcm,
        }
    }
    fn validate(&self) -> Result<(), &'static str> {
        self.validate()
    }
}

pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// None means EOF at a packet boundary. Truncated headers/bodies are errors.
pub(crate) fn read_header<const N: usize>(reader: &mut impl Read) -> io::Result<Option<[u8; N]>> {
    let mut header = [0; N];
    loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut header[1..])?;
    Ok(Some(header))
}

pub fn write_packet<T: Packet>(
    writer: &mut impl Write,
    identity: Identity,
    packet: &T,
) -> io::Result<()> {
    identity.validate().map_err(invalid)?;
    packet.validate().map_err(invalid)?;
    // Postcard's measurement flavor needs no payload allocation. Allocate
    // only a validated size, instead of clearing 512 KiB for every PCM ack.
    let size = postcard::serialize_with_flavor(packet, postcard::ser_flavors::Size::default())
        .map_err(|_| invalid("invalid packet size"))?;
    if size == 0 || size > MAX_PACKET_BYTES {
        return Err(invalid("packet length"));
    }
    let mut body = vec![0; size];
    let encoded = postcard::to_slice(packet, &mut body)
        .map_err(|_| invalid("invalid or oversized packet"))?;
    let mut header = [0; HEADER_BYTES];
    header[..4].copy_from_slice(MAGIC);
    header[4..6].copy_from_slice(&VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&(packet.kind() as u16).to_le_bytes());
    header[8..16].copy_from_slice(&identity.session.to_le_bytes());
    header[16..24].copy_from_slice(&identity.sequence.to_le_bytes());
    header[24..28].copy_from_slice(&(encoded.len() as u32).to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(encoded)?;
    writer.flush()
}

pub fn read_packet<T: Packet>(
    reader: &mut impl Read,
    expected_session: u64,
) -> io::Result<Option<(Identity, T)>> {
    let Some(header) = read_header::<HEADER_BYTES>(reader)? else {
        return Ok(None);
    };
    if &header[..4] != MAGIC || u16::from_le_bytes(header[4..6].try_into().unwrap()) != VERSION {
        return Err(invalid("packet magic or version"));
    }
    let kind = u16::from_le_bytes(header[6..8].try_into().unwrap());
    if !(1..=3).contains(&kind) {
        return Err(invalid("unknown packet kind"));
    }
    let identity = Identity {
        session: u64::from_le_bytes(header[8..16].try_into().unwrap()),
        sequence: u64::from_le_bytes(header[16..24].try_into().unwrap()),
    };
    identity.validate().map_err(invalid)?;
    if identity.session != expected_session {
        return Err(invalid("stale session"));
    }
    let len = u32::from_le_bytes(header[24..28].try_into().unwrap()) as usize;
    if len == 0 || len > MAX_PACKET_BYTES {
        return Err(invalid("packet length"));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    let (packet, trailing): (T, _) =
        postcard::take_from_bytes(&body).map_err(|_| invalid("invalid packet encoding"))?;
    if !trailing.is_empty() || packet.kind() as u16 != kind {
        return Err(invalid("packet trailing bytes or kind mismatch"));
    }
    packet.validate().map_err(invalid)?;
    Ok(Some((identity, packet)))
}

/// Used separately for control, PCM and progress. Responses echo their
/// request sequence; the receiver must match its outstanding request.
#[derive(Debug)]
pub struct Sequence {
    session: u64,
    next: u64,
}

impl Sequence {
    pub fn new(session: u64) -> io::Result<Self> {
        Identity {
            session,
            sequence: 1,
        }
        .validate()
        .map_err(invalid)?;
        Ok(Self { session, next: 1 })
    }
    pub fn issue(&mut self) -> io::Result<Identity> {
        let sequence = self.next;
        self.next = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("sequence exhausted"))?;
        Ok(Identity {
            session: self.session,
            sequence,
        })
    }
    pub fn accept(&mut self, identity: Identity) -> io::Result<()> {
        if identity.session != self.session || identity.sequence != self.next {
            return Err(invalid("stale or out-of-order sequence"));
        }
        self.issue().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Command, Items, KeyCode};

    #[test]
    fn packet_roundtrip_and_boundary_eof() {
        let id = Identity {
            session: 7,
            sequence: 1,
        };
        let packet = ParentPacket::Command(Command::Stop);
        let mut data = Vec::new();
        write_packet(&mut data, id, &packet).unwrap();
        let mut input = data.as_slice();
        assert_eq!(
            read_packet::<ParentPacket>(&mut input, 7).unwrap(),
            Some((id, packet))
        );
        assert!(read_packet::<ParentPacket>(&mut input, 7)
            .unwrap()
            .is_none());
        for end in 1..data.len() {
            assert!(read_packet::<ParentPacket>(&mut &data[..end], 7).is_err());
        }
    }

    #[test]
    fn malformed_headers_and_stale_identity_fail_before_body_allocation() {
        let mut data = Vec::new();
        write_packet(
            &mut data,
            Identity {
                session: 3,
                sequence: 2,
            },
            &ParentPacket::Command(Command::Stop),
        )
        .unwrap();
        assert!(read_packet::<ParentPacket>(&mut data.as_slice(), 4).is_err());
        for (offset, value) in [(0, 0), (4, 2), (6, 255), (8, 0), (16, 0)] {
            let mut bad = data.clone();
            bad[offset] = value;
            assert!(read_packet::<ParentPacket>(&mut bad.as_slice(), 3).is_err());
        }
        data[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_packet::<ParentPacket>(&mut &data[..HEADER_BYTES], 3).is_err());
    }

    #[test]
    fn huge_collection_prefix_unknown_enum_and_trailing_bytes_are_rejected() {
        let mut huge = Vec::new();
        // postcard uses unsigned LEB128 lengths. No payload or huge reserve is needed.
        huge.extend_from_slice(&[255, 255, 255, 255, 15]);
        assert!(postcard::from_bytes::<Items<u8, 960>>(&huge).is_err());
        assert!(postcard::from_bytes::<KeyCode>(&[255, 1]).is_err());
        let mut bytes = Vec::new();
        write_packet(
            &mut bytes,
            Identity {
                session: 1,
                sequence: 1,
            },
            &ParentPacket::Command(Command::Stop),
        )
        .unwrap();
        let n = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        bytes[24..28].copy_from_slice(&(n + 1).to_le_bytes());
        bytes.push(0);
        assert!(read_packet::<ParentPacket>(&mut bytes.as_slice(), 1).is_err());
    }

    #[test]
    fn sequence_rejects_repeat_skip_and_other_session() {
        let mut sequence = Sequence::new(9).unwrap();
        for id in [
            Identity {
                session: 8,
                sequence: 1,
            },
            Identity {
                session: 9,
                sequence: 2,
            },
        ] {
            assert!(sequence.accept(id).is_err());
        }
        let first = Identity {
            session: 9,
            sequence: 1,
        };
        sequence.accept(first).unwrap();
        assert!(sequence.accept(first).is_err());
        sequence.next = u64::MAX;
        assert!(sequence.issue().is_err());
    }
}
