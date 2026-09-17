use super::*;
use astra_emu_sdk::video::{AudioFramePacket, PcmQueue};
use std::sync::atomic::AtomicU64;

pub(crate) struct MoviePcm {
    queue: Mutex<PcmQueue>,
    position: AtomicU64,
    prepared: AtomicU64,
    pub paused: AtomicBool,
}
impl MoviePcm {
    pub fn new(generation: u64, position_us: u64) -> FamilyResult<Self> {
        Ok(Self {
            queue: Mutex::new(
                PcmQueue::new(
                    FORMAT.sample_rate,
                    FORMAT.channels,
                    generation,
                    position_us,
                    48000 * 2,
                    128,
                )
                .map_err(crate::scene::core_error)?,
            ),
            position: AtomicU64::new(position_us),
            prepared: AtomicU64::new(position_us),
            paused: AtomicBool::new(false),
        })
    }
    pub fn push(&self, packet: AudioFramePacket, samples: Vec<i16>) -> FamilyResult<()> {
        self.queue
            .lock()
            .map_err(|_| state_error())?
            .push(packet, samples)
            .map_err(crate::scene::core_error)
    }
    pub fn can_buffer(&self, frames: usize, packets: usize) -> FamilyResult<bool> {
        Ok(self
            .queue
            .lock()
            .map_err(|_| state_error())?
            .can_buffer(frames, packets))
    }
    pub fn position_us(&self) -> u64 {
        self.position.load(Ordering::Acquire)
    }
    pub fn drained(&self) -> FamilyResult<bool> {
        Ok(self.queue.lock().map_err(|_| state_error())?.is_empty()
            && self.prepared.load(Ordering::Acquire) == self.position_us())
    }
    pub(super) fn mix(&self, samples: &mut [f32]) -> FamilyResult<u64> {
        if self.paused.load(Ordering::Acquire) {
            return Ok(self.position_us());
        }
        let mut queue = self.queue.lock().map_err(|_| state_error())?;
        let position = queue
            .mix_into(samples)
            .map_err(crate::scene::core_error)?
            .position_us;
        self.prepared.store(position, Ordering::Release);
        Ok(position)
    }
    pub(super) fn accept(&self, position: u64) {
        self.position.store(position, Ordering::Release);
    }
}
impl Audio {
    pub fn set_movie(&self, movie: Option<Arc<MoviePcm>>) -> FamilyResult<()> {
        self.check()?;
        *self.movie.lock().map_err(|_| state_error())? = movie;
        Ok(())
    }
}
fn state_error() -> FamilyError {
    error("ASTRA_EMU_MUSICA_AUDIO_STATE", "movie PCM state poisoned")
}
