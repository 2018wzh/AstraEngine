use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
};

use astra_byte_source::OwnedF32Buffer;
use kira::{
    sound::{Sound, SoundData},
    Frame,
};
use rtrb::{Consumer, Producer, RingBuffer};

const PLAYING: u8 = 0;
const PAUSED: u8 = 1;
const STOPPED: u8 = 2;

enum StreamChunk {
    Owned(OwnedF32Buffer),
    Recyclable(Vec<f32>),
}

impl StreamChunk {
    fn as_slice(&self) -> &[f32] {
        match self {
            Self::Owned(samples) => samples,
            Self::Recyclable(samples) => samples,
        }
    }
}

struct SharedState {
    state: AtomicU8,
    finish_requested: AtomicBool,
    completed: AtomicBool,
    underflow_count: AtomicU64,
}

pub struct AstraStreamSoundData {
    channels: u16,
    chunk_samples: usize,
    ready_producer: Producer<StreamChunk>,
    ready_consumer: Consumer<StreamChunk>,
    recycle_producer: Producer<Vec<f32>>,
    recycle_consumer: Consumer<Vec<f32>>,
    retired_owned_producer: Producer<OwnedF32Buffer>,
    retired_owned_consumer: Consumer<OwnedF32Buffer>,
    shared: Arc<SharedState>,
}

impl AstraStreamSoundData {
    pub fn new(
        channels: u16,
        chunk_frames: usize,
        chunk_capacity: usize,
    ) -> Result<Self, &'static str> {
        if !matches!(channels, 1 | 2) || chunk_frames == 0 || chunk_capacity == 0 {
            return Err("stream shape or capacity is invalid");
        }
        let chunk_samples = chunk_frames
            .checked_mul(usize::from(channels))
            .ok_or("stream chunk size overflowed")?;
        let (ready_producer, ready_consumer) = RingBuffer::new(chunk_capacity);
        let (mut recycle_producer, recycle_consumer) = RingBuffer::new(chunk_capacity);
        let retired_capacity = chunk_capacity
            .checked_add(1)
            .ok_or("stream retirement capacity overflowed")?;
        let (retired_owned_producer, retired_owned_consumer) = RingBuffer::new(retired_capacity);
        for _ in 0..chunk_capacity {
            recycle_producer
                .push(vec![0.0; chunk_samples])
                .map_err(|_| "stream recycle queue initialization failed")?;
        }
        Ok(Self {
            channels,
            chunk_samples,
            ready_producer,
            ready_consumer,
            recycle_producer,
            recycle_consumer,
            retired_owned_producer,
            retired_owned_consumer,
            shared: Arc::new(SharedState {
                state: AtomicU8::new(PLAYING),
                finish_requested: AtomicBool::new(false),
                completed: AtomicBool::new(false),
                underflow_count: AtomicU64::new(0),
            }),
        })
    }
}

pub struct AstraStreamSoundHandle {
    channels: u16,
    ready: Producer<StreamChunk>,
    recycled: Consumer<Vec<f32>>,
    retired_owned: Consumer<OwnedF32Buffer>,
    chunk_samples: usize,
    shared: Arc<SharedState>,
}

impl AstraStreamSoundHandle {
    pub fn has_capacity(&self) -> bool {
        self.ready.slots() > 0
    }

    pub fn has_recyclable_capacity(&self) -> bool {
        self.has_capacity()
            && self.recycled.slots() > 0
            && !self.shared.finish_requested.load(Ordering::Acquire)
    }

    pub fn submit_owned(&mut self, samples: OwnedF32Buffer) -> Result<(), &'static str> {
        self.reclaim_owned();
        if samples.is_empty()
            || !samples.len().is_multiple_of(usize::from(self.channels))
            || samples.iter().any(|sample| !sample.is_finite())
            || !self.has_capacity()
            || self.shared.finish_requested.load(Ordering::Acquire)
        {
            return Err("stream chunk is invalid or the queue has no capacity");
        }
        self.ready
            .push(StreamChunk::Owned(samples))
            .map_err(|_| "stream queue changed while submitting")
    }

    pub fn submit_recyclable(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, &'static str> {
        self.reclaim_owned();
        if samples.len() != self.chunk_samples
            || samples.iter().any(|sample| !sample.is_finite())
            || !self.has_recyclable_capacity()
        {
            return Err("stream chunk is invalid or the queue has no capacity");
        }
        self.ready
            .push(StreamChunk::Recyclable(samples))
            .map_err(|_| "stream queue changed while submitting")?;
        self.recycled
            .pop()
            .map_err(|_| "stream recycle allocation is unavailable")
    }

    pub fn pause(&self) {
        self.shared.state.store(PAUSED, Ordering::Release);
    }

    pub fn resume(&self) {
        self.shared.state.store(PLAYING, Ordering::Release);
    }

    pub fn stop(&self) {
        self.shared.state.store(STOPPED, Ordering::Release);
    }

    pub fn finish(&self) {
        self.shared.finish_requested.store(true, Ordering::Release);
    }

    pub fn is_completed(&self) -> bool {
        self.shared.completed.load(Ordering::Acquire)
    }

    pub fn underflow_count(&self) -> u64 {
        self.shared.underflow_count.load(Ordering::Relaxed)
    }

    fn reclaim_owned(&mut self) {
        while self.retired_owned.pop().is_ok() {}
    }
}

impl Drop for AstraStreamSoundHandle {
    fn drop(&mut self) {
        self.reclaim_owned();
    }
}

impl SoundData for AstraStreamSoundData {
    type Error = std::convert::Infallible;
    type Handle = AstraStreamSoundHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let handle = AstraStreamSoundHandle {
            channels: self.channels,
            ready: self.ready_producer,
            recycled: self.recycle_consumer,
            retired_owned: self.retired_owned_consumer,
            chunk_samples: self.chunk_samples,
            shared: Arc::clone(&self.shared),
        };
        Ok((
            Box::new(AstraStreamSound {
                channels: self.channels,
                ready: self.ready_consumer,
                recycled: self.recycle_producer,
                retired_owned: self.retired_owned_producer,
                current: None,
                offset: 0,
                shared: self.shared,
            }),
            handle,
        ))
    }
}

struct AstraStreamSound {
    channels: u16,
    ready: Consumer<StreamChunk>,
    recycled: Producer<Vec<f32>>,
    retired_owned: Producer<OwnedF32Buffer>,
    current: Option<StreamChunk>,
    offset: usize,
    shared: Arc<SharedState>,
}

impl AstraStreamSound {
    fn retire_chunk(&mut self, chunk: StreamChunk) {
        match chunk {
            StreamChunk::Owned(samples) => self
                .retired_owned
                .push(samples)
                .expect("stream retired-owner capacity invariant must hold"),
            StreamChunk::Recyclable(samples) => self
                .recycled
                .push(samples)
                .expect("stream recycle capacity invariant must hold"),
        }
    }

    fn retire_all(&mut self) {
        if let Some(chunk) = self.current.take() {
            self.retire_chunk(chunk);
        }
        while let Ok(chunk) = self.ready.pop() {
            self.retire_chunk(chunk);
        }
        self.offset = 0;
    }

    fn next_frame(&mut self) -> Option<Frame> {
        if self.current.is_none() {
            self.current = self.ready.pop().ok();
            self.offset = 0;
        }
        let current = self.current.as_ref()?.as_slice();
        let frame = if self.channels == 1 {
            Frame::from_mono(current[self.offset])
        } else {
            Frame::new(current[self.offset], current[self.offset + 1])
        };
        self.offset += usize::from(self.channels);
        if self.offset == current.len() {
            let chunk = self.current.take().expect("completed stream chunk exists");
            self.retire_chunk(chunk);
        }
        Some(frame)
    }
}

impl Sound for AstraStreamSound {
    fn process(&mut self, out: &mut [Frame], _dt: f64, _info: &kira::info::Info) {
        match self.shared.state.load(Ordering::Acquire) {
            PAUSED => {
                out.fill(Frame::ZERO);
                return;
            }
            STOPPED => {
                out.fill(Frame::ZERO);
                self.retire_all();
                self.shared.completed.store(true, Ordering::Release);
                return;
            }
            _ => {}
        }
        let mut underflow = false;
        for output in out {
            if let Some(frame) = self.next_frame() {
                *output = frame;
            } else {
                *output = Frame::ZERO;
                underflow = true;
            }
        }
        if underflow {
            if self.shared.finish_requested.load(Ordering::Acquire) {
                self.shared.completed.store(true, Ordering::Release);
            } else {
                self.shared.underflow_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn finished(&self) -> bool {
        self.shared.completed.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::*;

    struct Samples {
        values: Vec<f32>,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Samples {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn consumed_owned_chunk_is_reclaimed_by_producer() {
        let AstraStreamSoundData {
            channels,
            chunk_samples,
            ready_producer,
            ready_consumer,
            recycle_producer,
            recycle_consumer,
            retired_owned_producer,
            retired_owned_consumer,
            shared,
        } = AstraStreamSoundData::new(2, 1, 2).expect("stream");
        let mut handle = AstraStreamSoundHandle {
            channels,
            ready: ready_producer,
            recycled: recycle_consumer,
            retired_owned: retired_owned_consumer,
            chunk_samples,
            shared: Arc::clone(&shared),
        };
        let mut sound = AstraStreamSound {
            channels,
            ready: ready_consumer,
            recycled: recycle_producer,
            retired_owned: retired_owned_producer,
            current: None,
            offset: 0,
            shared,
        };
        let drops = Arc::new(AtomicUsize::new(0));
        handle
            .submit_owned(OwnedF32Buffer::from_owner(
                Samples {
                    values: vec![0.25, -0.25],
                    drops: Arc::clone(&drops),
                },
                |samples| &samples.values,
            ))
            .expect("submit owned");

        assert_eq!(sound.next_frame(), Some(Frame::new(0.25, -0.25)));
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        handle
            .submit_owned(vec![0.0, 0.0].into())
            .expect("producer reclaims and submits");
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn recyclable_capacity_waits_for_the_audio_thread_to_return_a_buffer() {
        let AstraStreamSoundData {
            channels,
            chunk_samples,
            ready_producer,
            ready_consumer,
            recycle_producer,
            recycle_consumer,
            retired_owned_producer,
            retired_owned_consumer,
            shared,
        } = AstraStreamSoundData::new(2, 2, 1).expect("stream");
        let mut handle = AstraStreamSoundHandle {
            channels,
            ready: ready_producer,
            recycled: recycle_consumer,
            retired_owned: retired_owned_consumer,
            chunk_samples,
            shared: Arc::clone(&shared),
        };
        let mut sound = AstraStreamSound {
            channels,
            ready: ready_consumer,
            recycled: recycle_producer,
            retired_owned: retired_owned_producer,
            current: None,
            offset: 0,
            shared,
        };
        let reusable = handle
            .submit_recyclable(vec![0.25, -0.25, 0.5, -0.5])
            .expect("initial submit");

        assert_eq!(sound.next_frame(), Some(Frame::new(0.25, -0.25)));
        assert!(handle.has_capacity());
        assert!(!handle.has_recyclable_capacity());

        assert_eq!(sound.next_frame(), Some(Frame::new(0.5, -0.5)));
        assert!(handle.has_recyclable_capacity());
        handle
            .submit_recyclable(reusable)
            .expect("submit after recycle handoff");
    }
}
