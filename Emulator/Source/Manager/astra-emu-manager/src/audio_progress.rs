use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use astra_emu_family_api::{AudioPlaybackClock, AudioPlaybackProgress};

const MAX_PENDING_SPANS: usize = 4_096;

#[derive(Default)]
pub(super) struct PlaybackCounters {
    accepted: AtomicU64,
    played: AtomicU64,
    // Set once before publishing the configured sink to the Family worker.
    pub(super) resampler_delay: AtomicU64,
}

impl PlaybackCounters {
    pub(super) fn check_accept(&self, frames: u64) -> Result<(), &'static str> {
        self.accepted
            .load(Ordering::Acquire)
            .checked_add(frames)
            .map(|_| ())
            .ok_or("ASTRA_EMU_AUDIO_PROGRESS_OVERFLOW")
    }

    /// The serialized writer calls this only after the queue accepts its block.
    pub(super) fn accept(&self, frames: u64) {
        self.accepted.fetch_add(frames, Ordering::Release);
    }

    pub(super) fn snapshot(&self, clock: AudioPlaybackClock) -> AudioPlaybackProgress {
        // A callback only publishes frames bounded by an already accepted count.
        // Read played first so a concurrent successful write cannot invert them.
        let played = self.played.load(Ordering::Acquire);
        let accepted = self.accepted.load(Ordering::Acquire);
        AudioPlaybackProgress {
            accepted_source_frames: accepted,
            played_source_frames: played,
            clock,
        }
    }
}

struct OutputSpan {
    device_start: u64,
    media_start: u64,
    frames: u64,
}

/// Callback-owned mapping from submitted device frames to converted Family PCM.
/// Device silence has no media span. No device handle or query clock enters here.
pub(super) struct PlaybackTracker {
    counters: Arc<PlaybackCounters>,
    source_rate: u32,
    device_rate: u32,
    submitted: u64,
    media_submitted: u64,
    media_played: u64,
    observed_device: u64,
    spans: VecDeque<OutputSpan>,
}

impl PlaybackTracker {
    pub(super) fn new(counters: Arc<PlaybackCounters>, source_rate: u32, device_rate: u32) -> Self {
        Self {
            counters,
            source_rate,
            device_rate,
            submitted: 0,
            media_submitted: 0,
            media_played: 0,
            observed_device: 0,
            spans: VecDeque::with_capacity(MAX_PENDING_SPANS),
        }
    }

    /// CPAL WASAPI 0.18.2 derives this delay from IAudioClock and frames written,
    /// including the reported pipeline latency. Sample only at the callback;
    /// never advance the watermark using wall time between callbacks.
    pub(super) fn observe_device_delay(&mut self, delay: Duration) -> Result<(), &'static str> {
        let pending = delay
            .as_nanos()
            .checked_mul(u128::from(self.device_rate))
            .ok_or("ASTRA_EMU_AUDIO_PROGRESS_OVERFLOW")?
            .div_ceil(1_000_000_000);
        let pending = u64::try_from(pending).map_err(|_| "ASTRA_EMU_AUDIO_PROGRESS_OVERFLOW")?;
        self.observe(self.submitted.saturating_sub(pending))
    }

    pub(super) fn observe_test_consumption(&mut self) -> Result<(), &'static str> {
        self.observe(self.submitted)
    }

    pub(super) fn submitted_frame(&mut self, has_source: bool) -> Result<(), &'static str> {
        let next = self
            .submitted
            .checked_add(1)
            .ok_or("ASTRA_EMU_AUDIO_PROGRESS_OVERFLOW")?;
        if has_source {
            let media_next = self
                .media_submitted
                .checked_add(1)
                .ok_or("ASTRA_EMU_AUDIO_PROGRESS_OVERFLOW")?;
            match self.spans.back_mut() {
                Some(span) if span.device_start + span.frames == self.submitted => {
                    span.frames += 1;
                }
                _ => {
                    if self.spans.len() == MAX_PENDING_SPANS {
                        return Err("ASTRA_EMU_AUDIO_PROGRESS_BUDGET");
                    }
                    self.spans.push_back(OutputSpan {
                        device_start: self.submitted,
                        media_start: self.media_submitted,
                        frames: 1,
                    });
                }
            }
            self.media_submitted = media_next;
        }
        self.submitted = next;
        Ok(())
    }

    fn observe(&mut self, device_frame: u64) -> Result<(), &'static str> {
        // Device timestamp quantization may shift the sampled estimate by one
        // frame; never retract a previously published source watermark.
        if device_frame.saturating_add(1) < self.observed_device {
            return Err("ASTRA_EMU_AUDIO_PROGRESS_TIMESTAMP_REGRESSED");
        }
        self.observed_device = self.observed_device.max(device_frame);
        while let Some(span) = self.spans.front() {
            if self.observed_device <= span.device_start {
                break;
            }
            let played = (self.observed_device - span.device_start).min(span.frames);
            self.media_played = span.media_start + played;
            if played < span.frames {
                break;
            }
            self.spans.pop_front();
        }
        let delayed = self.counters.resampler_delay.load(Ordering::Acquire);
        let source = u128::from(self.media_played.saturating_sub(delayed))
            * u128::from(self.source_rate)
            / u128::from(self.device_rate);
        let source = u64::try_from(source).map_err(|_| "ASTRA_EMU_AUDIO_PROGRESS_OVERFLOW")?;
        // A device callback can run immediately after queue publication but
        // before the writer returns Accepted. The next observation catches up.
        let accepted = self.counters.accepted.load(Ordering::Acquire);
        self.counters
            .played
            .fetch_max(source.min(accepted), Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
#[path = "audio_progress_tests.rs"]
mod tests;
