//! These three loopback streams are private to one child, not a service.
//! No protocol data uses stdio, which remains available to native diagnostics.
use crate::wire::{self, Lane};
use std::{
    io,
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    process::Child,
    thread,
    time::{Duration, Instant},
};

pub(crate) const POLL: Duration = Duration::from_millis(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct Sockets {
    pub control: TcpStream,
    pub pcm: TcpStream,
    pub frame: TcpStream,
}

impl Sockets {
    pub fn connect(port: u16, session: u64) -> io::Result<Self> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let connect = |lane| -> io::Result<TcpStream> {
            let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
            stream.set_nodelay(true)?;
            stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
            stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
            wire::write_handshake(&mut stream, session, lane)?;
            if wire::read_handshake(&mut stream, session)? != lane {
                return Err(io::Error::other("KrKr lane handshake mismatch"));
            }
            stream.set_read_timeout(None)?;
            stream.set_write_timeout(None)?;
            Ok(stream)
        };
        Ok(Self {
            control: connect(Lane::Control)?,
            pcm: connect(Lane::Pcm)?,
            frame: connect(Lane::Frame)?,
        })
    }

    /// try_clone handles refer to the same underlying stream. shutdown also
    /// releases pending I/O in workers using those other handles.
    pub fn shutdown(&self) -> io::Result<()> {
        let mut first = None;
        for stream in [&self.control, &self.pcm, &self.frame] {
            if let Err(error) = stream.shutdown(Shutdown::Both) {
                // A peer already exiting, or an earlier PCM cancel, may have
                // disconnected this stream. In particular macOS rejects a
                // second shutdown, unlike Linux.
                if error.kind() != io::ErrorKind::NotConnected && first.is_none() {
                    first = Some(error);
                }
            }
        }
        first.map_or(Ok(()), Err)
    }
}

pub(crate) struct Rendezvous {
    listener: TcpListener,
    lanes: [Option<TcpStream>; 3],
}

impl Rendezvous {
    pub fn bind() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            lanes: [None, None, None],
        })
    }
    pub fn port(&self) -> io::Result<u16> {
        self.listener.local_addr().map(|address| address.port())
    }
    pub fn accept(
        mut self,
        child: &mut Child,
        session: u64,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<Sockets> {
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        while self.lanes.iter().any(Option::is_none) {
            if cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "KrKr open cancelled",
                ));
            }
            if child.try_wait()?.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "KrKr child exited before connecting",
                ));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "KrKr child connection timed out",
                ));
            }
            match self.listener.accept() {
                Ok((mut stream, peer)) => {
                    if !peer.ip().is_loopback() {
                        return Err(io::Error::other("non-loopback KrKr peer"));
                    }
                    stream.set_nodelay(true)?;
                    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
                    let lane = wire::read_handshake(&mut stream, session)?;
                    stream.set_read_timeout(None)?;
                    let slot = &mut self.lanes[lane as usize - 1];
                    if slot.is_some() {
                        let _ = stream.shutdown(Shutdown::Both);
                        return Err(io::Error::other("duplicate KrKr lane"));
                    }
                    wire::write_handshake(&mut stream, session, lane)?;
                    *slot = Some(stream);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Sockets {
            control: self.lanes[0].take().unwrap(),
            pcm: self.lanes[1].take().unwrap(),
            frame: self.lanes[2].take().unwrap(),
        })
    }
}
impl Drop for Rendezvous {
    fn drop(&mut self) {
        for stream in self.lanes.iter().flatten() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

pub(crate) fn arguments() -> io::Result<(u64, u16)> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 || args[0] != "--session" || args[2] != "--transport-port" {
        return Err(io::Error::other("invalid private KrKr child arguments"));
    }
    let session = args[1]
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| io::Error::other("invalid session"))?;
    let port = args[3]
        .parse::<u16>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| io::Error::other("invalid transport port"))?;
    Ok((session, port))
}

#[cfg(test)]
pub(crate) fn pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    (client, listener.accept().unwrap().0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn shutdown_releases_a_pending_read_on_a_cloned_stream() {
        let (stream, _peer) = pair();
        let mut reader = stream.try_clone().unwrap();
        let (ready, waiting) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            ready.send(()).unwrap();
            reader.read(&mut [0; 1])
        });
        waiting.recv().unwrap();
        stream.shutdown(Shutdown::Both).unwrap();
        let result = worker.join().unwrap();
        assert!(matches!(result, Ok(0)) || result.is_err());
    }

    #[test]
    fn shutdown_releases_a_blocked_large_write() {
        let (stream, _peer) = pair();
        let mut writer = stream.try_clone().unwrap();
        let (ready, waiting) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            ready.send(()).unwrap();
            let block = vec![0; wire::MAX_FRAME_BYTES];
            loop {
                writer.write_all(&block)?;
            }
            #[allow(unreachable_code)]
            Ok::<(), io::Error>(())
        });
        waiting.recv().unwrap();
        thread::sleep(Duration::from_millis(20));
        stream.shutdown(Shutdown::Both).unwrap();
        assert!(worker.join().unwrap().is_err());
    }
}
