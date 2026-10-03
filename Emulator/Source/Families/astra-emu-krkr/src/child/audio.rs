use crate::{
    sockets::POLL,
    wire::{self, Failure, Identity, ParentPacket, PlaybackClock, Progress, Sequence, WriteStatus},
};
use crossbeam_channel::{Receiver, Sender};
use std::{
    io,
    net::{Shutdown, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
};

struct ProgressSlot {
    accepted: AtomicU64,
    played: AtomicU64,
    clock: AtomicU8,
}
impl ProgressSlot {
    fn new() -> Self {
        Self {
            accepted: AtomicU64::new(0),
            played: AtomicU64::new(0),
            clock: AtomicU8::new(0),
        }
    }
}

/// Control/PCM writers serialize through AudioState::previous. The native
/// mixer reads the latest published slot without a lock, RPC or clock estimate;
/// a preempted writer updating the other slot leaves that observation usable.
struct ProgressCache {
    revision: AtomicU64,
    slots: [ProgressSlot; 2],
    valid: AtomicBool,
}

impl ProgressCache {
    fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
            slots: [ProgressSlot::new(), ProgressSlot::new()],
            valid: AtomicBool::new(false),
        }
    }
    fn store(&self, p: Progress) -> io::Result<()> {
        let revision = self
            .revision
            .load(Ordering::Relaxed)
            .checked_add(1)
            .ok_or_else(|| io::Error::other("progress cache revision exhausted"))?;
        let slot = &self.slots[revision as usize & 1];
        slot.accepted
            .store(p.accepted_source_frames, Ordering::Relaxed);
        slot.played.store(p.played_source_frames, Ordering::Relaxed);
        slot.clock.store(
            match p.clock {
                PlaybackClock::DeviceTimestamp => 1,
                PlaybackClock::TestConsumption => 2,
            },
            Ordering::Relaxed,
        );
        self.revision.store(revision, Ordering::Release);
        self.valid.store(true, Ordering::Release);
        Ok(())
    }
    fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }
    fn snapshot(&self) -> Option<Progress> {
        for _ in 0..8 {
            if !self.valid.load(Ordering::Acquire) {
                return None;
            }
            let first = self.revision.load(Ordering::Acquire);
            let slot = &self.slots[first as usize & 1];
            let accepted_source_frames = slot.accepted.load(Ordering::Relaxed);
            let played_source_frames = slot.played.load(Ordering::Relaxed);
            let clock = match slot.clock.load(Ordering::Relaxed) {
                1 => PlaybackClock::DeviceTimestamp,
                2 => PlaybackClock::TestConsumption,
                _ => return None,
            };
            // Keep data reads ahead of the final generation validation.
            std::sync::atomic::fence(Ordering::Acquire);
            if first == self.revision.load(Ordering::Relaxed) && self.valid.load(Ordering::Acquire)
            {
                return Some(Progress {
                    accepted_source_frames,
                    played_source_frames,
                    clock,
                });
            }
        }
        None
    }
}

pub(super) struct AudioState {
    pub stop: Arc<AtomicBool>,
    cache: ProgressCache,
    mixed: AtomicU64,
    sequence: Mutex<Sequence>,
    pending: Mutex<Option<Identity>>,
    previous: Mutex<Option<Progress>>,
    first: Mutex<Option<Failure>>,
    pcm: Sender<(Identity, wire::ChildPacket)>,
    ack: Receiver<(Identity, WriteStatus)>,
    cancel_pcm: TcpStream,
}

impl AudioState {
    pub fn new(
        session: u64,
        pcm: Sender<(Identity, wire::ChildPacket)>,
        ack: Receiver<(Identity, WriteStatus)>,
        cancel_pcm: TcpStream,
    ) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            stop: Arc::new(AtomicBool::new(false)),
            cache: ProgressCache::new(),
            mixed: AtomicU64::new(0),
            sequence: Mutex::new(Sequence::new(session)?),
            pending: Mutex::new(None),
            previous: Mutex::new(None),
            first: Mutex::new(None),
            pcm,
            ack,
            cancel_pcm,
        }))
    }
    pub fn cancel(&self) {
        let was_stopped = self.stop.swap(true, Ordering::AcqRel);
        self.cache.invalidate();
        if !was_stopped {
            let _ = self.cancel_pcm.shutdown(Shutdown::Both);
        }
    }
    pub fn fail(&self, failure: Failure) {
        let mut first = self.first.lock().unwrap();
        if first.is_none() {
            *first = Some(failure);
        }
        self.cancel();
    }
    pub fn first(&self) -> Option<Failure> {
        *self.first.lock().unwrap()
    }

    pub fn observe(&self, p: Progress) -> io::Result<()> {
        if self.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        p.validate()
            .map_err(|_| io::Error::other("invalid audio progress"))?;
        if p.accepted_source_frames > self.mixed.load(Ordering::Acquire) {
            return Err(io::Error::other("accepted exceeds mixed"));
        }
        let mut previous = self.previous.lock().unwrap();
        if let Some(last) = *previous {
            // Independent control/PCM lanes can deliver older real Host
            // snapshots after a newer observation on the other lane.
            if p.clock == last.clock
                && p.accepted_source_frames <= last.accepted_source_frames
                && p.played_source_frames <= last.played_source_frames
            {
                return Ok(());
            }
            p.validate_after(last)
                .map_err(|_| io::Error::other("audio progress regressed"))?;
        }
        *previous = Some(p);
        self.cache.store(p)?;
        // Cancellation racing the store must leave the snapshot invalid.
        if self.stop.load(Ordering::Acquire) {
            self.cache.invalidate();
        }
        Ok(())
    }

    pub fn acknowledge(
        &self,
        identity: Identity,
        packet: ParentPacket,
        ack: &Sender<(Identity, WriteStatus)>,
    ) -> io::Result<()> {
        let ParentPacket::PcmReply { status, progress } = packet else {
            return Err(io::Error::other("expected PCM reply"));
        };
        let pending = self.pending.lock().unwrap();
        if *pending != Some(identity) {
            return Err(io::Error::other("stale PCM reply"));
        }
        if matches!(status, WriteStatus::Accepted)
            && progress.map(|p| p.accepted_source_frames)
                != Some(self.mixed.load(Ordering::Acquire))
        {
            return Err(io::Error::other("accepted PCM frame count mismatch"));
        }
        if let Some(p) = progress {
            self.observe(p)?;
        }
        ack.try_send((identity, status))
            .map_err(|_| io::Error::other("duplicate PCM reply"))?;
        // The producer clears pending only when it consumes the exact reply.
        Ok(())
    }
}

/// Remains alive through native producer cancellation/join and VM/GL
/// shutdown. The future C++ callback context owns a clone of this value.
#[derive(Clone)]
pub struct AudioCallbacks(pub(super) Arc<AudioState>);

impl AudioCallbacks {
    pub fn progress(&self) -> Option<Progress> {
        if self.is_cancelled() {
            return None;
        }
        self.0.cache.snapshot().filter(|_| !self.is_cancelled())
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.stop.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// One synchronous producer, exactly 480 stereo source frames. Returning
    /// Accepted means the real parent Host::write has already copied them.
    pub fn write(&self, samples: &[i16]) -> io::Result<WriteStatus> {
        if self.is_cancelled() {
            return Ok(WriteStatus::Cancelled);
        }
        if samples.len() != wire::PCM_SAMPLES {
            return Err(io::Error::other("PCM block size"));
        }
        let identity = {
            let mut pending = self.0.pending.lock().unwrap();
            if pending.is_some() {
                self.0.fail(Failure::Protocol);
                return Err(io::Error::other("concurrent PCM producer"));
            }
            let identity = self.0.sequence.lock().unwrap().issue()?;
            self.0.mixed.store(
                self.0
                    .mixed
                    .load(Ordering::Relaxed)
                    .checked_add(wire::PCM_FRAMES as u64)
                    .ok_or_else(|| io::Error::other("mixed frame count overflow"))?,
                Ordering::Release,
            );
            *pending = Some(identity);
            identity
        };
        let packet = wire::ChildPacket::PcmWrite(
            wire::Items::new(samples.to_vec()).map_err(io::Error::other)?,
        );
        if self.0.pcm.try_send((identity, packet)).is_err() {
            self.0.fail(Failure::Protocol);
            return Err(io::Error::other("PCM pipe unavailable"));
        }
        loop {
            if self.is_cancelled() {
                return Ok(WriteStatus::Cancelled);
            }
            match self.0.ack.recv_timeout(POLL) {
                Ok((reply_identity, status)) => {
                    if reply_identity != identity {
                        self.0.fail(Failure::Protocol);
                        return Err(io::Error::other("stale PCM acknowledgement"));
                    }
                    self.0.pending.lock().unwrap().take();
                    if !matches!(status, WriteStatus::Accepted) {
                        self.0.cancel();
                    }
                    return Ok(status);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    self.0.fail(Failure::Protocol);
                    return Err(io::Error::other("PCM acknowledgement disconnected"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;

    #[test]
    fn cached_snapshot_never_mixes_counts_from_two_observations() {
        let cache = Arc::new(ProgressCache::new());
        let writer_cache = Arc::clone(&cache);
        let writer = std::thread::spawn(move || {
            for n in 1..=20000 {
                writer_cache
                    .store(Progress {
                        accepted_source_frames: n * 480,
                        played_source_frames: n * 240,
                        clock: PlaybackClock::DeviceTimestamp,
                    })
                    .unwrap();
            }
        });
        while !writer.is_finished() {
            if let Some(progress) = cache.snapshot() {
                assert_eq!(
                    progress.accepted_source_frames,
                    progress.played_source_frames * 2
                );
                assert_eq!(progress.clock, PlaybackClock::DeviceTimestamp);
            }
        }
        writer.join().unwrap();
        assert_eq!(
            cache.snapshot().unwrap().accepted_source_frames,
            20000 * 480
        );
        cache.invalidate();
        assert!(cache.snapshot().is_none());
    }

    #[test]
    fn accepted_ack_cannot_use_a_zero_or_invented_counter() {
        let (pcm, _) = bounded(1);
        let (ack, replies) = bounded(1);
        let (socket, _peer) = crate::sockets::pair();
        let state = AudioState::new(9, pcm, replies, socket).unwrap();
        let id = Identity {
            session: 9,
            sequence: 1,
        };
        state.mixed.store(480, Ordering::Release);
        *state.pending.lock().unwrap() = Some(id);
        for accepted_source_frames in [0, 479, 481] {
            let p = Progress {
                accepted_source_frames,
                played_source_frames: 0,
                clock: PlaybackClock::DeviceTimestamp,
            };
            assert!(state
                .acknowledge(
                    id,
                    ParentPacket::PcmReply {
                        status: WriteStatus::Accepted,
                        progress: Some(p)
                    },
                    &ack
                )
                .is_err());
        }
    }

    #[test]
    fn snapshots_are_observations_and_invalidate_on_cancel() {
        let (pcm, _) = bounded(1);
        let (_, ack) = bounded(1);
        let (socket, _peer) = crate::sockets::pair();
        let state = AudioState::new(1, pcm, ack, socket).unwrap();
        let cb = AudioCallbacks(Arc::clone(&state));
        assert!(cb.progress().is_none());
        let p = Progress {
            accepted_source_frames: 0,
            played_source_frames: 0,
            clock: PlaybackClock::DeviceTimestamp,
        };
        state.observe(p).unwrap();
        assert_eq!(cb.progress(), Some(p));
        state.cancel();
        state.observe(p).unwrap();
        assert!(cb.progress().is_none());
    }

    #[test]
    fn cancellation_releases_pcm_ack_wait_without_any_reply() {
        let (pcm, requests) = bounded(1);
        let (_ack, replies) = bounded(1);
        let (socket, _peer) = crate::sockets::pair();
        let state = AudioState::new(1, pcm, replies, socket).unwrap();
        let cb = AudioCallbacks(Arc::clone(&state));
        let worker = std::thread::spawn(move || cb.write(&[0; wire::PCM_SAMPLES]).unwrap());
        requests
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        state.cancel();
        assert_eq!(worker.join().unwrap(), WriteStatus::Cancelled);
    }
}
