use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
};

use kira::{
    sound::{Sound, SoundData},
    Frame,
};
use rtrb::{Consumer, Producer, RingBuffer};

const PLAYING: u8 = 0;
const PAUSED: u8 = 1;
const STOPPED: u8 = 2;

struct SharedState {
    state: AtomicU8,
    finish_requested: AtomicBool,
    completed: AtomicBool,
    underflow_count: AtomicU64,
}

pub struct AstraStreamSoundData {
    channels: u16,
    chunk_samples: usize,
    ready_producer: Producer<Vec<f32>>,
    ready_consumer: Consumer<Vec<f32>>,
    recycle_producer: Producer<Vec<f32>>,
    recycle_consumer: Consumer<Vec<f32>>,
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
    ready: Producer<Vec<f32>>,
    recycled: Consumer<Vec<f32>>,
    chunk_samples: usize,
    shared: Arc<SharedState>,
}

impl AstraStreamSoundHandle {
    pub fn has_capacity(&self) -> bool {
        self.ready.slots() > 0 && self.recycled.slots() > 0
    }

    pub fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, &'static str> {
        if samples.len() != self.chunk_samples
            || samples.iter().any(|sample| !sample.is_finite())
            || !self.has_capacity()
            || self.shared.finish_requested.load(Ordering::Acquire)
        {
            return Err("stream chunk is invalid or the queue has no capacity");
        }
        self.ready
            .push(samples)
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
}

impl SoundData for AstraStreamSoundData {
    type Error = std::convert::Infallible;
    type Handle = AstraStreamSoundHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let handle = AstraStreamSoundHandle {
            ready: self.ready_producer,
            recycled: self.recycle_consumer,
            chunk_samples: self.chunk_samples,
            shared: Arc::clone(&self.shared),
        };
        Ok((
            Box::new(AstraStreamSound {
                channels: self.channels,
                ready: self.ready_consumer,
                recycled: self.recycle_producer,
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
    ready: Consumer<Vec<f32>>,
    recycled: Producer<Vec<f32>>,
    current: Option<Vec<f32>>,
    offset: usize,
    shared: Arc<SharedState>,
}

impl AstraStreamSound {
    fn next_frame(&mut self) -> Option<Frame> {
        if self.current.is_none() {
            self.current = self.ready.pop().ok();
            self.offset = 0;
        }
        let current = self.current.as_ref()?;
        let frame = if self.channels == 1 {
            Frame::from_mono(current[self.offset])
        } else {
            Frame::new(current[self.offset], current[self.offset + 1])
        };
        self.offset += usize::from(self.channels);
        if self.offset == current.len() {
            let chunk = self.current.take().expect("completed stream chunk exists");
            self.recycled
                .push(chunk)
                .expect("stream recycle capacity invariant must hold");
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
