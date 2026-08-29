use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use astra_platform::{AudioOutputLane, AudioWakeRegistration, PlatformError, PlatformErrorCode};
use rtrb::{Consumer, Producer, RingBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioQueueTelemetry {
    pub queued_samples: u64,
    pub consumed_samples: u64,
    pub underflow_count: u64,
}

#[derive(Default)]
struct SharedTelemetry {
    queued_samples: AtomicU64,
    consumed_samples: AtomicU64,
    underflow_count: AtomicU64,
}

#[derive(Clone)]
pub struct AudioQueueTelemetryReader {
    shared: Arc<SharedTelemetry>,
}

impl AudioQueueTelemetryReader {
    pub fn snapshot(&self) -> AudioQueueTelemetry {
        AudioQueueTelemetry {
            queued_samples: self.shared.queued_samples.load(Ordering::Acquire),
            consumed_samples: self.shared.consumed_samples.load(Ordering::Acquire),
            underflow_count: self.shared.underflow_count.load(Ordering::Relaxed),
        }
    }
}

pub struct NativeAudioProducer {
    ready: Producer<Vec<f32>>,
    recycled: Consumer<Vec<f32>>,
    chunk_samples: usize,
    wake: AudioWakeRegistration,
    observed_wake: u64,
    telemetry: Arc<SharedTelemetry>,
}

impl NativeAudioProducer {
    pub fn try_submit_owned(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        if samples.len() != self.chunk_samples || self.ready.slots() == 0 {
            return Err(queue_overflow(
                "audio output lane has no capacity for the chunk",
            ));
        }
        let sample_count = samples.len() as u64;
        self.ready
            .push(samples)
            .map_err(|_| queue_overflow("audio output lane changed while submitting"))?;
        self.telemetry
            .queued_samples
            .fetch_add(sample_count, Ordering::Release);
        self.recycled
            .pop()
            .map_err(|_| queue_overflow("audio output lane has no recycled allocation"))
    }
}

/// Bounded sink used when a native host has no physical audio endpoint.
///
/// The sink preserves the ownership and chunk-size contract of
/// [`NativeAudioProducer`] while consuming samples immediately.  It is not a
/// decoder or a mixer fallback: the selected host explicitly reports the
/// null endpoint and the caller still observes submitted/consumed counters.
pub struct NullAudioProducer {
    chunk_samples: usize,
    consumed_samples: u64,
}

impl NullAudioProducer {
    pub fn new(chunk_samples: usize) -> Result<Self, PlatformError> {
        if chunk_samples == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.null",
                "null audio output chunk size must be non-zero",
            ));
        }
        Ok(Self {
            chunk_samples,
            consumed_samples: 0,
        })
    }
}

impl AudioOutputLane for NullAudioProducer {
    fn wait_for_capacity(
        &mut self,
        requested_samples: usize,
        _stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        if requested_samples != self.chunk_samples {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.lane.wait",
                "mixer chunk size does not match the opened null output lane",
            ));
        }
        Ok(())
    }

    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        if samples.len() != self.chunk_samples {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.lane.submit",
                "mixer chunk size does not match the opened null output lane",
            ));
        }
        self.consumed_samples = self
            .consumed_samples
            .checked_add(samples.len() as u64)
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "audio.lane.submit",
                    "null audio consumed sample counter overflowed",
                )
            })?;
        Ok(samples)
    }

    fn consumed_samples(&self) -> u64 {
        self.consumed_samples
    }

    fn underflow_count(&self) -> u64 {
        0
    }
}

impl AudioOutputLane for NativeAudioProducer {
    fn wait_for_capacity(
        &mut self,
        requested_samples: usize,
        stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        if requested_samples != self.chunk_samples {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.lane.wait",
                "mixer chunk size does not match the opened output lane",
            ));
        }
        while self.ready.slots() == 0 || self.recycled.slots() == 0 {
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if let Some(sequence) = self
                .wake
                .wait_timeout(self.observed_wake, std::time::Duration::from_millis(20))
            {
                self.observed_wake = sequence;
            }
        }
        Ok(())
    }

    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        self.try_submit_owned(samples)
    }

    fn consumed_samples(&self) -> u64 {
        self.telemetry.consumed_samples.load(Ordering::Acquire)
    }

    fn underflow_count(&self) -> u64 {
        self.telemetry.underflow_count.load(Ordering::Relaxed)
    }
}

struct CurrentChunk {
    samples: Vec<f32>,
    offset: usize,
}

pub struct NativeAudioConsumer {
    ready: Consumer<Vec<f32>>,
    recycled: Producer<Vec<f32>>,
    current: Option<CurrentChunk>,
    telemetry: Arc<SharedTelemetry>,
}

impl NativeAudioConsumer {
    /// Copies only into the final device callback buffer. Source chunks remain owned and are
    /// returned whole to the mixer after the last sample is consumed.
    pub fn pop_samples(&mut self, target: &mut [f32]) -> usize {
        let mut written = 0;
        while written < target.len() {
            if self.current.is_none() {
                let Ok(samples) = self.ready.pop() else {
                    break;
                };
                self.current = Some(CurrentChunk { samples, offset: 0 });
            }
            let current = self.current.as_mut().expect("current chunk exists");
            let available = current.samples.len() - current.offset;
            let count = available.min(target.len() - written);
            target[written..written + count]
                .copy_from_slice(&current.samples[current.offset..current.offset + count]);
            current.offset += count;
            written += count;
            if current.offset == current.samples.len() {
                let chunk = self.current.take().expect("completed chunk exists").samples;
                self.recycled
                    .push(chunk)
                    .expect("recycle queue capacity invariant must hold");
            }
        }
        if written != 0 {
            self.telemetry
                .queued_samples
                .fetch_sub(written as u64, Ordering::Release);
            self.telemetry
                .consumed_samples
                .fetch_add(written as u64, Ordering::Release);
        }
        written
    }

    pub fn pop_sample(&mut self) -> Option<f32> {
        let mut sample = [0.0];
        (self.pop_samples(&mut sample) == 1).then_some(sample[0])
    }

    pub fn record_underflow(&self) {
        self.telemetry
            .underflow_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Fills an f32 device callback buffer, silence-fills any shortfall and
    /// records the underflow. Returns true when the buffer was underfilled.
    pub fn fill_output_f32(&mut self, output: &mut [f32]) -> bool {
        let filled = self.pop_samples(output);
        output[filled..].fill(0.0);
        if filled != output.len() {
            self.record_underflow();
        }
        filled != output.len()
    }

    /// Fills an i16 device callback buffer by converting from the canonical
    /// f32 stream. Returns true when the buffer was underfilled.
    pub fn fill_output_i16(&mut self, output: &mut [i16]) -> bool {
        let mut scratch = [0.0_f32; 1024];
        let mut written = 0;
        while written < output.len() {
            let requested = scratch.len().min(output.len() - written);
            let filled = self.pop_samples(&mut scratch[..requested]);
            for (target, sample) in output[written..written + filled]
                .iter_mut()
                .zip(&scratch[..filled])
            {
                *target = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            }
            written += filled;
            if filled != requested {
                break;
            }
        }
        output[written..].fill(0);
        if written != output.len() {
            self.record_underflow();
        }
        written != output.len()
    }

    /// Fills a u16 device callback buffer by converting from the canonical
    /// f32 stream. Returns true when the buffer was underfilled.
    pub fn fill_output_u16(&mut self, output: &mut [u16]) -> bool {
        let mut scratch = [0.0_f32; 1024];
        let mut written = 0;
        while written < output.len() {
            let requested = scratch.len().min(output.len() - written);
            let filled = self.pop_samples(&mut scratch[..requested]);
            for (target, sample) in output[written..written + filled]
                .iter_mut()
                .zip(&scratch[..filled])
            {
                *target = ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * f32::from(u16::MAX)) as u16;
            }
            written += filled;
            if filled != requested {
                break;
            }
        }
        output[written..].fill(u16::MAX / 2);
        if written != output.len() {
            self.record_underflow();
        }
        written != output.len()
    }
}

pub struct NativeAudioQueue;

impl NativeAudioQueue {
    pub fn create(
        chunk_capacity: usize,
        chunk_samples: usize,
        wake: AudioWakeRegistration,
    ) -> Result<
        (
            NativeAudioProducer,
            NativeAudioConsumer,
            AudioQueueTelemetryReader,
        ),
        PlatformError,
    > {
        if chunk_capacity == 0 || chunk_samples == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.queue.create",
                "audio output chunk capacity and size must be non-zero",
            ));
        }
        let (ready_producer, ready_consumer) = RingBuffer::new(chunk_capacity);
        let (mut recycle_producer, recycle_consumer) = RingBuffer::new(chunk_capacity);
        for _ in 0..chunk_capacity {
            recycle_producer
                .push(vec![0.0; chunk_samples])
                .expect("new recycle queue has declared capacity");
        }
        let telemetry = Arc::new(SharedTelemetry::default());
        Ok((
            NativeAudioProducer {
                ready: ready_producer,
                recycled: recycle_consumer,
                chunk_samples,
                wake,
                observed_wake: 0,
                telemetry: Arc::clone(&telemetry),
            },
            NativeAudioConsumer {
                ready: ready_consumer,
                recycled: recycle_producer,
                current: None,
                telemetry: Arc::clone(&telemetry),
            },
            AudioQueueTelemetryReader { shared: telemetry },
        ))
    }
}

fn queue_overflow(message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::QueueOverflow, "audio.submit", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_output_preserves_chunk_ownership_and_telemetry() {
        let mut lane = NullAudioProducer::new(4).expect("non-zero chunk is valid");
        let stop = AtomicBool::new(false);
        lane.wait_for_capacity(4, &stop)
            .expect("null output is immediately available");
        let samples = vec![0.25; 4];
        let returned = lane
            .submit(samples)
            .expect("null output accepts a full chunk");
        assert_eq!(returned, vec![0.25; 4]);
        assert_eq!(lane.consumed_samples(), 4);
        assert_eq!(lane.underflow_count(), 0);
    }

    #[test]
    fn null_output_rejects_wrong_chunk_size() {
        let mut lane = NullAudioProducer::new(4).expect("non-zero chunk is valid");
        let stop = AtomicBool::new(false);
        let error = lane
            .wait_for_capacity(2, &stop)
            .expect_err("wrong chunk size must be rejected");
        assert_eq!(error.code, PlatformErrorCode::IntegrityMismatch);
        let error = lane
            .submit(vec![0.0; 2])
            .expect_err("wrong chunk size must be rejected");
        assert_eq!(error.code, PlatformErrorCode::IntegrityMismatch);
    }
}
