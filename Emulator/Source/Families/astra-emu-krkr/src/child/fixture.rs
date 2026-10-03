//! Deliberate transport faults. Never a native core, Family or VM/GPU fixture.
use super::*;
use std::io::Write;

pub fn transport_fixture(mode: &str, session: u64, port: u16) -> io::Result<()> {
    match mode {
        "ignore-stop" => {
            let _streams = Sockets::connect(port, session)?;
            loop {
                thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        "stale-control" | "partial-control" | "unknown-control" => {
            let mut streams = Sockets::connect(port, session)?;
            let identity = Identity {
                session: if mode == "stale-control" {
                    session + 1
                } else {
                    session
                },
                sequence: 1,
            };
            let mut bytes = Vec::new();
            wire::write_packet(&mut bytes, identity, &ChildPacket::Reply(Reply::Stopped))?;
            if mode == "partial-control" {
                bytes.truncate(3);
            }
            if mode == "unknown-control" {
                bytes[28] = 99;
            }
            streams.control.write_all(&bytes)?;
            streams.shutdown()
        }
        "blocked-frame" => {
            let mut streams = ChildSockets::open(session, port)?;
            let identity = Identity {
                session,
                sequence: 1,
            };
            let info = FrameInfo {
                width: 4096,
                height: 4096,
                logical_width: 4096,
                logical_height: 4096,
                stride: 4096 * 4,
            };
            let mut pixels = vec![0; wire::MAX_FRAME_BYTES];
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel[3] = 255;
            }
            streams
                .frames
                .try_send(FinalFrame {
                    identity,
                    info,
                    pixels,
                })
                .map_err(io::Error::other)?;
            let (identity, command) = streams.next()?;
            if command != Command::Stop {
                return Err(io::Error::other("fixture expects stop"));
            }
            streams.finish(identity, Reply::Stopped)
        }
        _ => Err(io::Error::other("unknown transport fixture")),
    }
}
