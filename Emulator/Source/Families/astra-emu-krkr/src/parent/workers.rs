use super::shared::{failure_error, Shared};
use crate::{
    sockets::{Sockets, POLL},
    wire::{
        self, ChildPacket, Command, Failure, FinalFrame, Identity, ParentPacket, Progress, Reply,
        Sequence, WriteStatus,
    },
};
use astra_emu_family_api::{AudioWriteStatus, PcmChunk};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::{
    io,
    net::TcpStream,
    sync::{atomic::Ordering, Arc},
    thread::{self, JoinHandle},
};

type Envelope = (Identity, ParentPacket);
pub(super) struct Channels {
    pub stop: Sender<Envelope>,
    pub control: Sender<Envelope>,
    pub replies: Receiver<(Identity, Reply)>,
    pub frame_requests: Sender<Identity>,
    pub frames: Receiver<FinalFrame>,
}

fn spawn(
    shared: &Arc<Shared>,
    name: &'static str,
    job: impl FnOnce(Arc<Shared>) -> io::Result<()> + Send + 'static,
) -> io::Result<JoinHandle<io::Result<()>>> {
    let shared = Arc::clone(shared);
    thread::Builder::new().name(name.to_owned()).spawn(move || {
        let result = job(Arc::clone(&shared));
        if result.is_err()
            && !shared.io_stop.load(Ordering::Acquire)
            && !shared.closing.load(Ordering::Acquire)
        {
            shared.fail(Failure::Protocol);
        }
        result
    })
}

/// Caller already owns all three sockets. On a partial spawn error it shuts
/// them down and joins every handle added here before returning.
pub(super) fn start(
    shared: &Arc<Shared>,
    session: u64,
    sockets: &Sockets,
    workers: &mut Vec<JoinHandle<io::Result<()>>>,
) -> io::Result<Channels> {
    let (stop_tx, stop_rx) = bounded::<Envelope>(1);
    let (control_tx, control_rx) = bounded::<Envelope>(1);
    let (progress_tx, progress_rx) = bounded::<Progress>(1);
    let (reply_tx, reply_rx) = bounded::<(Identity, Reply)>(2);
    let (frame_request_tx, frame_request_rx) = bounded::<Identity>(1);
    let (frame_tx, frame_rx) = bounded::<FinalFrame>(1);
    let input = sockets.control.try_clone()?;
    let mut output = sockets.control.try_clone()?;
    let mut pcm = sockets.pcm.try_clone()?;
    let mut bulk = sockets.frame.try_clone()?;

    workers.push(spawn(
        shared,
        "krkr-parent-control-writer",
        move |shared| write_loop(shared, session, input, stop_rx, control_rx, progress_rx),
    )?);
    workers.push(spawn(
        shared,
        "krkr-parent-control-reader",
        move |shared| {
            loop {
                let Some((identity, packet)) =
                    wire::read_packet::<ChildPacket>(&mut output, session)?
                else {
                    shared.invalidate();
                    if shared.closing.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    return Err(failure_error(Failure::Protocol));
                };
                let ChildPacket::Reply(reply) = packet else {
                    return Err(failure_error(Failure::Protocol));
                };
                // Record the native failure before EOF races the request owner.
                if let Reply::Failed(failure) = reply {
                    shared.fail(failure);
                }
                reply_tx
                    .try_send((identity, reply))
                    .map_err(|_| failure_error(Failure::Protocol))?;
            }
        },
    )?);
    workers.push(spawn(shared, "krkr-parent-pcm", move |shared| {
        let mut sequence = Sequence::new(session)?;
        let mut accepted = 0_u64;
        loop {
            if shared.pcm_stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let Some((identity, packet)) = wire::read_packet::<ChildPacket>(&mut pcm, session)?
            else {
                // A failed boot stops its audio before sending the final
                // control result. Invalidate immediately, while the control
                // lane supplies that original failure. Active EOF is fatal.
                shared.cancel();
                if shared.active.load(Ordering::Acquire) && !shared.closing.load(Ordering::Acquire)
                {
                    shared.fail(Failure::Audio);
                }
                return Ok(());
            };
            sequence.accept(identity)?;
            let ChildPacket::PcmWrite(samples) = packet else {
                return Err(failure_error(Failure::Protocol));
            };
            // This stream and Host call never hold ProviderModule/request locks.
            let result = shared
                .sink
                .write(PcmChunk::I16(samples.into_vec().into()))
                .into_result();
            let (status, progress) = match result {
                Ok(AudioWriteStatus::Accepted) => match shared.observe() {
                    Ok(progress)
                        if accepted.checked_add(wire::PCM_FRAMES as u64)
                            == Some(progress.accepted_source_frames) =>
                    {
                        accepted = progress.accepted_source_frames;
                        (WriteStatus::Accepted, Some(progress))
                    }
                    Ok(_) => {
                        shared.fail(Failure::Audio);
                        (WriteStatus::Failed(Failure::Audio), None)
                    }
                    Err(_)
                        if shared.closing.load(Ordering::Acquire)
                            || shared.cancel_requested.load(Ordering::Acquire) =>
                    {
                        (WriteStatus::Cancelled, None)
                    }
                    Err(_) => {
                        shared.fail(Failure::Audio);
                        (WriteStatus::Failed(Failure::Audio), None)
                    }
                },
                Ok(AudioWriteStatus::Cancelled) => (WriteStatus::Cancelled, None),
                Ok(AudioWriteStatus::Closed) => (WriteStatus::Closed, None),
                Err(_) => {
                    shared.fail(Failure::Audio);
                    (WriteStatus::Failed(Failure::Audio), None)
                }
            };
            if !matches!(status, WriteStatus::Accepted)
                && !shared.closing.load(Ordering::Acquire)
                && !shared.cancel_requested.load(Ordering::Acquire)
            {
                shared.fail(Failure::Audio);
            }
            wire::write_packet(
                &mut pcm,
                identity,
                &ParentPacket::PcmReply { status, progress },
            )?;
        }
    })?);
    workers.push(spawn(shared, "krkr-parent-progress", move |shared| {
        while !shared.pcm_stop.load(Ordering::Acquire) {
            match shared.observe() {
                Ok(progress) => {
                    let _ = progress_tx.try_send(progress);
                }
                Err(_)
                    if shared.pcm_stop.load(Ordering::Acquire)
                        || shared.closing.load(Ordering::Acquire)
                        || shared.cancel_requested.load(Ordering::Acquire) =>
                {
                    return Ok(())
                }
                Err(_) => {
                    shared.fail(Failure::Audio);
                    return Ok(());
                }
            }
            thread::sleep(POLL);
        }
        Ok(())
    })?);
    workers.push(spawn(shared, "krkr-parent-frame", move |shared| {
        while !shared.io_stop.load(Ordering::Acquire) {
            let expected = match frame_request_rx.recv_timeout(POLL) {
                Ok(identity) => identity,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Ok(()),
            };
            let frame = wire::read_frame(&mut bulk, expected)?
                .ok_or_else(|| failure_error(Failure::Protocol))?;
            frame_tx
                .try_send(frame)
                .map_err(|_| failure_error(Failure::Protocol))?;
        }
        Ok(())
    })?);
    Ok(Channels {
        stop: stop_tx,
        control: control_tx,
        replies: reply_rx,
        frame_requests: frame_request_tx,
        frames: frame_rx,
    })
}

fn write_loop(
    shared: Arc<Shared>,
    session: u64,
    mut input: TcpStream,
    stop: Receiver<Envelope>,
    control: Receiver<Envelope>,
    progress: Receiver<Progress>,
) -> io::Result<()> {
    let mut sequence = Sequence::new(session)?;
    let mut previous = shared.initial_progress();
    while !shared.io_stop.load(Ordering::Acquire) {
        let envelope = crossbeam_channel::select_biased! {
            recv(stop) -> value => value.ok(),
            recv(control) -> value => value.ok(),
            recv(progress) -> value => match value {
                Ok(p) if p.validate_after(previous).is_ok() => { previous = p; Some((sequence.issue()?, ParentPacket::Progress(p))) },
                Ok(_) => continue,
                Err(_) => None,
            },
            default(POLL) => continue,
        };
        let Some((identity, packet)) = envelope else {
            return Ok(());
        };
        wire::write_packet(&mut input, identity, &packet)?;
        if matches!(packet, ParentPacket::Command(Command::Stop)) {
            return Ok(());
        }
    }
    Ok(())
}
