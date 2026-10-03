//! Dedicated native owner thread with independent control, PCM and final
//! frame TCP lanes. Stdio remains available to core/dependency diagnostics.
mod audio;
#[cfg(feature = "transport-test-child")]
mod fixture;
use crate::{
    sockets::{self, Sockets, POLL},
    wire::{
        self, ChildPacket, Command, Failure, FinalFrame, FrameInfo, Identity, ParentPacket, Reply,
        Sequence, WindowState, WriteStatus,
    },
};
pub use audio::AudioCallbacks;
use audio::AudioState;
use crossbeam_channel::{bounded, Receiver, Sender};
#[cfg(feature = "transport-test-child")]
pub use fixture::transport_fixture;
use std::{
    io,
    net::Shutdown,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};

pub trait NativeOwner {
    fn boot(
        &mut self,
        game_path: &str,
        window: WindowState,
        audio: AudioCallbacks,
    ) -> Result<FrameInfo, Failure>;
    fn advance(&mut self, elapsed_ns: u64, events: &[wire::Event]) -> Result<Reply, Failure>;
    fn capture(&mut self) -> Result<(FrameInfo, Vec<u8>), Failure>;
    /// Cancel/join audio producer first, then tear down VM/GL and join all
    /// native workers, completing every cleanup step even after an error.
    fn shutdown(&mut self) -> Result<(), Failure>;
}

/// Explicit unavailable shell, never emits NativeReady. No VM, native GPU
/// capture or functioning input is implemented by this transport crate.
pub struct UnavailableOwner;
impl NativeOwner for UnavailableOwner {
    fn boot(&mut self, _: &str, _: WindowState, _: AudioCallbacks) -> Result<FrameInfo, Failure> {
        Err(Failure::NativeUnavailable)
    }
    fn advance(&mut self, _: u64, _: &[wire::Event]) -> Result<Reply, Failure> {
        Err(Failure::NativeUnavailable)
    }
    fn capture(&mut self) -> Result<(FrameInfo, Vec<u8>), Failure> {
        Err(Failure::NativeUnavailable)
    }
    fn shutdown(&mut self) -> Result<(), Failure> {
        Ok(())
    }
}

enum Output {
    Packet(Identity, ChildPacket),
    Final(Identity, Reply),
}
struct ChildSockets {
    state: Arc<AudioState>,
    sockets: Sockets,
    incoming: Receiver<(Identity, Command)>,
    stop: Receiver<Identity>,
    outgoing: Sender<Output>,
    frames: Sender<FinalFrame>,
    read_stop: Arc<AtomicBool>,
    write_stop: Arc<AtomicBool>,
    frame_stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<io::Result<()>>>,
    writer: Option<JoinHandle<io::Result<()>>>,
    pcm_worker: Option<JoinHandle<io::Result<()>>>,
    frame_writer: Option<JoinHandle<io::Result<()>>>,
}

fn run_worker(
    state: Arc<AudioState>,
    name: &'static str,
    stop: Arc<AtomicBool>,
    job: impl FnOnce() -> io::Result<()> + Send + 'static,
) -> io::Result<JoinHandle<io::Result<()>>> {
    thread::Builder::new().name(name.to_owned()).spawn(move || {
        let result = job();
        if result.is_err() && !stop.load(Ordering::Acquire) {
            state.fail(Failure::Protocol);
        }
        result
    })
}

impl ChildSockets {
    fn open(session: u64, port: u16) -> io::Result<Self> {
        let sockets = Sockets::connect(port, session)?;
        let (commands, incoming) = bounded(1);
        let (stops, stop) = bounded(1);
        let (outgoing, control) = bounded::<Output>(1);
        let (pcm, pcm_requests) = bounded::<(Identity, ChildPacket)>(1);
        let (acks, pcm_replies) = bounded::<(Identity, WriteStatus)>(1);
        let (frames, bulk_frames) = bounded::<FinalFrame>(1);
        let state = AudioState::new(session, pcm, pcm_replies, sockets.pcm.try_clone()?)?;
        let mut pipes = Self {
            state,
            sockets,
            incoming,
            stop,
            outgoing,
            frames,
            read_stop: Arc::new(AtomicBool::new(false)),
            write_stop: Arc::new(AtomicBool::new(false)),
            frame_stop: Arc::new(AtomicBool::new(false)),
            reader: None,
            writer: None,
            pcm_worker: None,
            frame_writer: None,
        };
        let mut input = pipes.sockets.control.try_clone()?;
        let mut output = pipes.sockets.control.try_clone()?;
        let mut pcm_stream = pipes.sockets.pcm.try_clone()?;
        let mut bulk = pipes.sockets.frame.try_clone()?;
        let state = Arc::clone(&pipes.state);
        pipes.reader = Some(run_worker(
            Arc::clone(&state),
            "krkr-child-control-reader",
            Arc::clone(&pipes.read_stop),
            move || {
                let mut controls = Sequence::new(session)?;
                let mut progress = Sequence::new(session)?;
                loop {
                    let Some((identity, packet)) =
                        wire::read_packet::<ParentPacket>(&mut input, session)?
                    else {
                        return Err(io::Error::other("parent disconnected"));
                    };
                    match packet {
                        ParentPacket::Command(Command::Stop) => {
                            controls.accept(identity)?;
                            state.cancel();
                            stops
                                .try_send(identity)
                                .map_err(|_| io::Error::other("duplicate stop"))?;
                            return Ok(());
                        }
                        ParentPacket::Command(command) => {
                            controls.accept(identity)?;
                            if let Command::Boot { progress, .. } = &command {
                                state.observe(*progress)?;
                            }
                            commands
                                .try_send((identity, command))
                                .map_err(|_| io::Error::other("command queue full"))?;
                        }
                        ParentPacket::Progress(p) => {
                            progress.accept(identity)?;
                            state.observe(p)?;
                        }
                        ParentPacket::PcmReply { .. } => {
                            return Err(io::Error::other("PCM reply on control lane"))
                        }
                    }
                }
            },
        )?);
        let state = Arc::clone(&pipes.state);
        let write_stop = Arc::clone(&pipes.write_stop);
        pipes.writer = Some(run_worker(
            state,
            "krkr-child-control-writer",
            Arc::clone(&write_stop),
            move || {
                while !write_stop.load(Ordering::Acquire) {
                    match control.recv_timeout(POLL) {
                        Ok(Output::Packet(id, packet)) => {
                            wire::write_packet(&mut output, id, &packet)?
                        }
                        Ok(Output::Final(id, reply)) => {
                            wire::write_packet(&mut output, id, &ChildPacket::Reply(reply))?;
                            return Ok(());
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        Err(_) => return Ok(()),
                    }
                }
                Ok(())
            },
        )?);
        let state = Arc::clone(&pipes.state);
        pipes.pcm_worker = Some(run_worker(
            Arc::clone(&state),
            "krkr-child-pcm",
            Arc::clone(&state.stop),
            move || {
                while !state.stop.load(Ordering::Acquire) {
                    let (identity, packet) = match pcm_requests.recv_timeout(POLL) {
                        Ok(value) => value,
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Ok(()),
                    };
                    wire::write_packet(&mut pcm_stream, identity, &packet)?;
                    let Some((reply_id, reply)) =
                        wire::read_packet::<ParentPacket>(&mut pcm_stream, session)?
                    else {
                        return Err(io::Error::other("parent PCM disconnected"));
                    };
                    if reply_id != identity {
                        return Err(io::Error::other("PCM acknowledgement identity"));
                    }
                    state.acknowledge(reply_id, reply, &acks)?;
                }
                Ok(())
            },
        )?);
        let state = Arc::clone(&pipes.state);
        let frame_stop = Arc::clone(&pipes.frame_stop);
        pipes.frame_writer = Some(run_worker(
            state,
            "krkr-child-frame",
            Arc::clone(&frame_stop),
            move || {
                while !frame_stop.load(Ordering::Acquire) {
                    match bulk_frames.recv_timeout(POLL) {
                        Ok(frame) => {
                            wire::write_frame(&mut bulk, frame.identity, frame.info, &frame.pixels)?
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        Err(_) => return Ok(()),
                    }
                }
                Ok(())
            },
        )?);
        Ok(pipes)
    }

    fn next(&self) -> io::Result<(Identity, Command)> {
        loop {
            if let Ok(id) = self.stop.try_recv() {
                return Ok((id, Command::Stop));
            }
            if self.state.first().is_some() {
                return Err(io::Error::other("child transport failed"));
            }
            crossbeam_channel::select_biased! {
                recv(self.stop) -> value => return value.map(|id| (id, Command::Stop)).map_err(io::Error::other),
                recv(self.incoming) -> value => return value.map_err(io::Error::other),
                default(POLL) => {},
            }
        }
    }
    fn reply(&self, identity: Identity, reply: Reply) -> io::Result<()> {
        self.outgoing
            .try_send(Output::Packet(identity, ChildPacket::Reply(reply)))
            .map_err(io::Error::other)
    }

    fn finish(&mut self, identity: Identity, reply: Reply) -> io::Result<()> {
        self.state.cancel();
        self.read_stop.store(true, Ordering::Release);
        self.frame_stop.store(true, Ordering::Release);
        let _ = self.sockets.control.shutdown(Shutdown::Read);
        let _ = self.sockets.frame.shutdown(Shutdown::Both);
        for worker in [
            &mut self.reader,
            &mut self.pcm_worker,
            &mut self.frame_writer,
        ] {
            if let Some(worker) = worker.take() {
                if worker.join().is_err() {
                    self.state.fail(Failure::Shutdown);
                }
            }
        }
        let reply = match self.state.first() {
            Some(failure) => Reply::Failed(failure),
            None => reply,
        };
        let result = self
            .outgoing
            .try_send(Output::Final(identity, reply))
            .map_err(io::Error::other);
        if result.is_ok() {
            if let Some(worker) = self.writer.take() {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !worker.is_finished() && std::time::Instant::now() < deadline {
                    thread::sleep(POLL);
                }
                if !worker.is_finished() {
                    self.write_stop.store(true, Ordering::Release);
                    let _ = self.sockets.control.shutdown(Shutdown::Both);
                    self.state.fail(Failure::Shutdown);
                    let _ = worker.join();
                    return Err(io::Error::other("child final reply blocked"));
                }
                worker
                    .join()
                    .map_err(|_| io::Error::other("child writer panicked"))??;
            }
        }
        result
    }
}
impl Drop for ChildSockets {
    fn drop(&mut self) {
        self.state.cancel();
        self.read_stop.store(true, Ordering::Release);
        self.frame_stop.store(true, Ordering::Release);
        self.write_stop.store(true, Ordering::Release);
        let _ = self.sockets.shutdown();
        for worker in [
            &mut self.reader,
            &mut self.pcm_worker,
            &mut self.frame_writer,
            &mut self.writer,
        ] {
            if let Some(worker) = worker.take() {
                if worker.join().is_err() {
                    self.state.fail(Failure::Shutdown);
                }
            }
        }
    }
}

pub fn serve(session: u64, port: u16, mut owner: impl NativeOwner) -> io::Result<()> {
    let mut pipes = ChildSockets::open(session, port)?;
    let callbacks = AudioCallbacks(Arc::clone(&pipes.state));
    let mut booted = false;
    let mut last_id = Identity {
        session,
        sequence: 1,
    };
    let mut first = None;
    let final_reply = loop {
        let (identity, command) = match pipes.next() {
            Ok(value) => value,
            Err(_) => {
                first = Some(pipes.state.first().unwrap_or(Failure::Protocol));
                break Reply::Failed(first.unwrap());
            }
        };
        last_id = identity;
        let result = match command {
            Command::Stop => break Reply::Stopped,
            Command::Boot {
                game_path, window, ..
            } if !booted => owner
                .boot(game_path.as_str(), window, callbacks.clone())
                .map(|frame| {
                    booted = true;
                    Reply::NativeReady { frame }
                }),
            Command::Advance { elapsed_ns, events } if booted => owner
                .advance(elapsed_ns, events.as_slice())
                .and_then(|reply| {
                    if matches!(reply, Reply::Advanced { .. }) {
                        Ok(reply)
                    } else {
                        Err(Failure::Protocol)
                    }
                }),
            Command::Capture if booted => owner.capture().and_then(|(info, pixels)| {
                if info.required_bytes().ok() != Some(pixels.len()) {
                    return Err(Failure::Frame);
                }
                pipes
                    .frames
                    .try_send(FinalFrame {
                        identity,
                        info,
                        pixels,
                    })
                    .map_err(|_| Failure::Frame)?;
                Ok(Reply::FrameReady { info })
            }),
            _ => Err(Failure::Protocol),
        };
        match result {
            Ok(reply) => {
                if pipes.reply(identity, reply).is_err() {
                    first = Some(Failure::Protocol);
                    break Reply::Failed(Failure::Protocol);
                }
            }
            Err(failure) => {
                first = Some(failure);
                break Reply::Failed(failure);
            }
        }
    };
    callbacks.cancel();
    // Keep owner and its callback contexts alive until every native producer,
    // VM and GL worker is stopped/joined, including a partially failed boot.
    if let Err(failure) = owner.shutdown() {
        if first.is_none() {
            first = Some(failure);
        }
    }
    pipes.finish(last_id, first.map_or(final_reply, Reply::Failed))?;
    if let Some(failure) = first.or_else(|| pipes.state.first()) {
        return Err(io::Error::other(format!("KrKr {failure:?}")));
    }
    Ok(())
}

pub fn connection_arguments() -> io::Result<(u64, u16)> {
    sockets::arguments()
}
