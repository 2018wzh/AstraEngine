use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use astra_platform::{PlatformError, PlatformErrorCode};
use rtrb::{Consumer, Producer, RingBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioQueueTelemetry {
    pub sample_count: u64,
    pub underflow_count: u64,
}

#[derive(Clone)]
pub struct AudioQueueTelemetryReader {
    sample_count: Arc<AtomicU64>,
    underflow_count: Arc<AtomicU64>,
}

impl AudioQueueTelemetryReader {
    pub fn snapshot(&self) -> AudioQueueTelemetry {
        AudioQueueTelemetry {
            sample_count: self.sample_count.load(Ordering::Relaxed),
            underflow_count: self.underflow_count.load(Ordering::Relaxed),
        }
    }
}

pub struct NativeAudioProducer {
    inner: Producer<f32>,
}

impl NativeAudioProducer {
    pub fn push_samples(&mut self, samples: &[f32]) -> Result<(), PlatformError> {
        if self.inner.slots() < samples.len() {
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "audio.submit",
                "audio output queue is full",
            ));
        }
        for &sample in samples {
            self.inner.push(sample).map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    "audio.submit",
                    "audio output queue changed while producer was submitting",
                )
            })?;
        }
        Ok(())
    }
}

pub struct NativeAudioConsumer {
    inner: Consumer<f32>,
    sample_count: Arc<AtomicU64>,
    underflow_count: Arc<AtomicU64>,
}

impl NativeAudioConsumer {
    pub fn pop_sample(&mut self) -> Option<f32> {
        match self.inner.pop() {
            Ok(sample) => {
                self.sample_count.fetch_add(1, Ordering::Relaxed);
                Some(sample)
            }
            Err(_) => {
                self.underflow_count.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
}

pub struct NativeAudioQueue;

impl NativeAudioQueue {
    pub fn new(
        capacity: usize,
    ) -> Result<
        (
            NativeAudioProducer,
            NativeAudioConsumer,
            AudioQueueTelemetryReader,
        ),
        PlatformError,
    > {
        if capacity == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.queue.create",
                "audio output queue capacity must be non-zero",
            ));
        }
        let (producer, consumer) = RingBuffer::new(capacity);
        let sample_count = Arc::new(AtomicU64::new(0));
        let underflow_count = Arc::new(AtomicU64::new(0));
        Ok((
            NativeAudioProducer { inner: producer },
            NativeAudioConsumer {
                inner: consumer,
                sample_count: Arc::clone(&sample_count),
                underflow_count: Arc::clone(&underflow_count),
            },
            AudioQueueTelemetryReader {
                sample_count,
                underflow_count,
            },
        ))
    }
}
