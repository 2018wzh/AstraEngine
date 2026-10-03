//! Parent ownership for one KrKr native child. This module does not register
//! a Family or supply a native runtime. The fixed executable is selected by
//! the eventual Family adapter; Command is private integration/test plumbing.
mod shared;
mod workers;

use self::{
    shared::{failure_error, Shared},
    workers::Channels,
};
use crate::{
    sockets::{Rendezvous, Sockets, POLL},
    wire::{
        Command as WireCommand, Failure, FinalFrame, FrameInfo, Identity, ParentPacket, Reply,
        Sequence,
    },
};
use astra_emu_family_api::AudioSinkBox;
use std::{
    io,
    process::{Child, Command, Stdio},
    sync::{atomic::Ordering, Arc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SessionTransport {
    child: Option<Child>,
    sockets: Option<Sockets>,
    shared: Arc<Shared>,
    channels: Option<Channels>,
    workers: Vec<JoinHandle<io::Result<()>>>,
    sequence: Sequence,
    pid: u32,
    booted: bool,
}

/// Does not hold the request owner or ProviderModule lock. Cancellation
/// interrupts a blocked Host::write and wakes request/open failure cleanup.
#[derive(Clone)]
pub struct Cancellation(Arc<Shared>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.cancel_requested.store(true, Ordering::Release);
        self.0.cancel();
    }
}

impl SessionTransport {
    pub fn launch(mut command: Command, sink: Arc<AudioSinkBox>, session: u64) -> io::Result<Self> {
        let sequence = Sequence::new(session)?;
        let rendezvous = Rendezvous::bind()?;
        let port = rendezvous.port()?;
        let shared = Shared::configure(sink)?;
        command
            .arg("--session")
            .arg(session.to_string())
            .arg("--transport-port")
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                shared.cancel();
                return Err(error);
            }
        };
        let pid = child.id();
        let mut owner = Self {
            child: Some(child),
            sockets: None,
            shared,
            channels: None,
            workers: Vec::new(),
            sequence,
            pid,
            booted: false,
        };
        let shared = Arc::clone(&owner.shared);
        let sockets = match rendezvous.accept(owner.child.as_mut().unwrap(), session, || {
            shared.sink.is_cancelled() || shared.cancel_requested.load(Ordering::Acquire)
        }) {
            Ok(sockets) => sockets,
            Err(first) => {
                owner.shared.fail(Failure::Protocol);
                let _ = owner.cleanup();
                return Err(first);
            }
        };
        owner.sockets = Some(sockets);
        match workers::start(
            &owner.shared,
            session,
            owner.sockets.as_ref().unwrap(),
            &mut owner.workers,
        ) {
            Ok(channels) => owner.channels = Some(channels),
            Err(first) => {
                owner.shared.fail(Failure::Protocol);
                let _ = owner.cleanup();
                return Err(first);
            }
        }
        Ok(owner)
    }

    /// On every open failure, callbacks are cancelled, child reaped, and all
    /// workers joined before returning the original error.
    pub fn open(
        command: Command,
        sink: Arc<AudioSinkBox>,
        session: u64,
        boot: WireCommand,
    ) -> io::Result<(Self, FrameInfo)> {
        if !matches!(&boot, WireCommand::Boot { .. }) {
            return Err(failure_error(Failure::Protocol));
        }
        let mut owner = Self::launch(command, sink, session)?;
        match owner.boot(boot) {
            Ok(frame) => Ok((owner, frame)),
            Err(first) => {
                let _ = owner.cleanup();
                Err(first)
            }
        }
    }

    pub fn process_id(&self) -> u32 {
        self.pid
    }
    pub fn cancellation(&self) -> Cancellation {
        Cancellation(Arc::clone(&self.shared))
    }
    pub fn initial_progress(&self) -> crate::wire::Progress {
        self.shared.initial_progress()
    }

    pub fn boot(&mut self, boot: WireCommand) -> io::Result<FrameInfo> {
        if self.booted
            || !matches!(&boot, WireCommand::Boot { progress, .. } if *progress == self.shared.initial_progress())
        {
            return Err(failure_error(Failure::Protocol));
        }
        match self.request(boot)? {
            Reply::NativeReady { frame } => {
                self.shared.active.store(true, Ordering::Release);
                if !self.shared.progress_valid.load(Ordering::Acquire) {
                    self.shared.fail(Failure::Audio);
                    return Err(failure_error(Failure::Audio));
                }
                self.booted = true;
                Ok(frame)
            }
            _ => self.protocol_failure(),
        }
    }

    pub fn advance(
        &mut self,
        elapsed_ns: u64,
        events: crate::wire::Items<crate::wire::Event, 4096>,
    ) -> io::Result<Reply> {
        if !self.booted {
            return Err(failure_error(Failure::Protocol));
        }
        let reply = self.request(WireCommand::Advance { elapsed_ns, events })?;
        if matches!(reply, Reply::Advanced { .. }) {
            Ok(reply)
        } else {
            self.protocol_failure()
        }
    }

    pub fn capture(&mut self) -> io::Result<FinalFrame> {
        if !self.booted {
            return Err(failure_error(Failure::Protocol));
        }
        let identity = self.sequence.issue()?;
        self.channels
            .as_ref()
            .ok_or_else(|| failure_error(Failure::Protocol))?
            .frame_requests
            .try_send(identity)
            .map_err(|_| failure_error(Failure::Protocol))?;
        self.send(identity, WireCommand::Capture)?;
        let info = match self.wait_reply(identity, Instant::now() + REQUEST_TIMEOUT, false)? {
            Reply::FrameReady { info } => info,
            _ => return self.protocol_failure(),
        };
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            self.check_failure()?;
            match self.channels.as_ref().unwrap().frames.recv_timeout(POLL) {
                Ok(frame) if frame.identity == identity && frame.info == info => return Ok(frame),
                Ok(_) => return self.protocol_failure(),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(_) => return self.protocol_failure(),
            }
        }
    }

    fn request(&mut self, command: WireCommand) -> io::Result<Reply> {
        self.check_failure()?;
        let identity = self.sequence.issue()?;
        self.send(identity, command)?;
        self.wait_reply(identity, Instant::now() + REQUEST_TIMEOUT, false)
    }

    fn send(&self, identity: Identity, command: WireCommand) -> io::Result<()> {
        let channels = self
            .channels
            .as_ref()
            .ok_or_else(|| failure_error(Failure::Protocol))?;
        let packet = ParentPacket::Command(command);
        packet
            .validate()
            .map_err(|_| failure_error(Failure::Protocol))?;
        channels
            .control
            .try_send((identity, packet))
            .map_err(|_| failure_error(Failure::Protocol))
    }

    fn wait_reply(
        &self,
        expected: Identity,
        deadline: Instant,
        closing: bool,
    ) -> io::Result<Reply> {
        let channels = self
            .channels
            .as_ref()
            .ok_or_else(|| failure_error(Failure::Protocol))?;
        loop {
            if !closing {
                self.check_failure()?;
            }
            match channels.replies.recv_timeout(POLL) {
                Ok((identity, Reply::Failed(failure))) if identity == expected => {
                    self.shared.fail(failure);
                    return Err(failure_error(failure));
                }
                Ok((identity, reply)) if identity == expected => return Ok(reply),
                // Stop may overtake an already issued request. Its reply is
                // no longer delivered to the caller during cleanup.
                Ok((identity, _))
                    if closing
                        && identity.session == expected.session
                        && identity.sequence < expected.sequence => {}
                Ok(_) => return self.protocol_failure(),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(_) => return self.protocol_failure(),
            }
        }
    }

    fn check_failure(&self) -> io::Result<()> {
        if let Some(failure) = self.shared.first() {
            return Err(failure_error(failure));
        }
        if self.shared.cancel_requested.load(Ordering::Acquire)
            && !self.shared.closing.load(Ordering::Acquire)
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "KrKr request cancelled",
            ));
        }
        Ok(())
    }
    fn protocol_failure<T>(&self) -> io::Result<T> {
        self.shared.fail(Failure::Protocol);
        Err(failure_error(Failure::Protocol))
    }

    pub fn close(mut self) -> io::Result<()> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let Some(mut child) = self.child.take() else {
            return self.check_failure();
        };
        self.shared.closing.store(true, Ordering::Release);
        self.shared.cancel();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        let stopped = (|| {
            let identity = self.sequence.issue()?;
            self.channels
                .as_ref()
                .ok_or_else(|| failure_error(Failure::Shutdown))?
                .stop
                .try_send((identity, ParentPacket::Command(WireCommand::Stop)))
                .map_err(|_| failure_error(Failure::Shutdown))?;
            if self.wait_reply(identity, deadline, true)? != Reply::Stopped {
                return Err(failure_error(Failure::Shutdown));
            }
            loop {
                if let Some(status) = child.try_wait()? {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(failure_error(Failure::Shutdown))
                    };
                }
                if Instant::now() >= deadline {
                    return Err(failure_error(Failure::Shutdown));
                }
                thread::sleep(POLL);
            }
        })();
        if stopped.is_err() {
            // A killed child is always a shutdown failure. It is still reaped
            // and every local callback/pipe worker is joined below.
            self.shared.fail(Failure::Shutdown);
            let _ = child.kill();
        }
        if child.wait().is_err() {
            self.shared.fail(Failure::Shutdown);
        }
        self.shared.io_stop.store(true, Ordering::Release);
        if let Some(sockets) = self.sockets.take() {
            if sockets.shutdown().is_err() {
                self.shared.fail(Failure::Shutdown);
            }
        }
        self.channels.take();
        for worker in self.workers.drain(..) {
            match worker.join() {
                Ok(Ok(())) => {}
                // Once cancellation is requested, a blocked pipe's abort is
                // expected; genuine earlier errors were recorded by workers.
                Ok(Err(_)) => {}
                Err(_) => self.shared.fail(Failure::Shutdown),
            }
        }
        self.check_failure()
    }
}

impl Drop for SessionTransport {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
