use crate::CoreError;
use astra_media::AudioFramePacket;
use std::collections::VecDeque;

struct Packet {
    start: u64,
    samples: Vec<i16>,
}

/// A bounded, device-independent PCM track for a host's existing audio worker.
/// Decoding/resampling belongs to the decoder; this track only adds matching PCM
/// to the host mix. No audio device, thread, or Engine session is created here.
pub struct PcmQueue {
    rate: u32,
    channels: u16,
    generation: u64,
    position: u64,
    last_sequence: u64,
    last_end: Option<u64>,
    max_frames: usize,
    max_packets: usize,
    resident_frames: usize,
    packets: VecDeque<Packet>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PcmMixResult {
    /// PCM timeline position. Publish this only after the host accepts the mix.
    pub position_us: u64,
    /// Includes authored gaps, but excludes the unfilled tail on underrun.
    pub advanced_frames: usize,
    pub starved: bool,
}

impl PcmQueue {
    pub fn new(
        rate: u32,
        channels: u16,
        generation: u64,
        position_us: u64,
        max_frames: usize,
        max_packets: usize,
    ) -> Result<Self, CoreError> {
        if !(8000..=192000).contains(&rate)
            || !(1..=2).contains(&channels)
            || generation == 0
            || max_frames == 0
            || max_packets == 0
        {
            return Err(invalid("invalid PCM format, generation or budget"));
        }
        Ok(Self {
            rate,
            channels,
            generation,
            position: frame_at(position_us, rate)?,
            last_sequence: 0,
            last_end: None,
            max_frames,
            max_packets,
            resident_frames: 0,
            packets: VecDeque::new(),
        })
    }

    /// Validate before taking ownership. A rejected packet leaves the queue intact.
    /// The budget counts entire resident allocations, including partially read packets.
    pub fn push(&mut self, packet: AudioFramePacket, samples: Vec<i16>) -> Result<(), CoreError> {
        if packet.generation != self.generation {
            return Err(CoreError::invalid(
                "ASTRA_EMU_PCM_GENERATION",
                "stale PCM packet",
            ));
        }
        let frames = packet.frame_count as usize;
        if packet.sample_rate != self.rate
            || packet.channels != self.channels
            || frames == 0
            || frames.checked_mul(usize::from(self.channels)) != Some(samples.len())
            || packet.sequence <= self.last_sequence
            || packet.duration_us == 0
        {
            return Err(invalid("invalid PCM format, length or sequence"));
        }
        let resident = self
            .resident_frames
            .checked_add(frames)
            .filter(|count| *count <= self.max_frames)
            .ok_or_else(|| {
                CoreError::invalid("ASTRA_EMU_PCM_BUDGET", "PCM frame budget exceeded")
            })?;
        if self.packets.len() >= self.max_packets {
            return Err(CoreError::invalid(
                "ASTRA_EMU_PCM_BUDGET",
                "PCM packet budget exceeded",
            ));
        }
        let mut start = frame_at(packet.pts_us, self.rate)?;
        if let Some(end) = self.last_end {
            // Microsecond timestamps cannot represent every sample boundary.
            if start.abs_diff(end) <= 1 {
                start = end;
            } else if start < end {
                return Err(invalid("PCM timestamps overlap or move backwards"));
            }
        }
        let end = start
            .checked_add(u64::from(packet.frame_count))
            .ok_or_else(|| invalid("PCM timeline overflow"))?;
        self.packets.push_back(Packet { start, samples });
        self.resident_frames = resident;
        self.last_end = Some(end);
        self.last_sequence = packet.sequence;
        Ok(())
    }

    /// Add interleaved signed-16 PCM to an existing floating-point mix.
    /// Call only while playing. When data is unavailable, leave the mix and clock
    /// unchanged for the remaining frames; never invent elapsed movie audio.
    pub fn mix_into(&mut self, output: &mut [f32]) -> Result<PcmMixResult, CoreError> {
        let channels = usize::from(self.channels);
        if !output.len().is_multiple_of(channels) {
            return Err(invalid("output is not a whole number of PCM frames"));
        }
        let requested = output.len() / channels;
        let end = self
            .position
            .checked_add(requested as u64)
            .ok_or_else(|| invalid("PCM timeline overflow"))?;
        position_us(end, self.rate)?;
        let mut advanced = 0;
        while advanced < requested {
            let Some(packet) = self.packets.front() else {
                break;
            };
            let frames = packet.samples.len() / channels;
            let end = packet.start + frames as u64;
            if self.position >= end {
                self.packets.pop_front();
                self.resident_frames -= frames;
                continue;
            }
            if self.position < packet.start {
                let gap =
                    (packet.start - self.position).min((requested - advanced) as u64) as usize;
                self.position += gap as u64;
                advanced += gap;
                continue;
            }
            let offset = (self.position - packet.start) as usize;
            let count = (frames - offset).min(requested - advanced);
            let input = &packet.samples[offset * channels..(offset + count) * channels];
            let target = &mut output[advanced * channels..(advanced + count) * channels];
            for (target, sample) in target.iter_mut().zip(input) {
                *target += f32::from(*sample) / 32768.0;
            }
            advanced += count;
            self.position += count as u64;
            if self.position == end {
                self.packets.pop_front();
                self.resident_frames -= frames;
            }
        }
        Ok(PcmMixResult {
            position_us: self.position_us(),
            advanced_frames: advanced,
            starved: advanced < requested,
        })
    }

    pub fn position_us(&self) -> u64 {
        position_us(self.position, self.rate).expect("validated PCM position")
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    pub fn resident_frames(&self) -> usize {
        self.resident_frames
    }

    /// Restore/seek discards queued PCM before any packet of the new generation.
    pub fn reset(&mut self, generation: u64, position_us: u64) -> Result<(), CoreError> {
        if generation <= self.generation {
            return Err(CoreError::invalid(
                "ASTRA_EMU_PCM_GENERATION",
                "PCM generation must increase",
            ));
        }
        let position = frame_at(position_us, self.rate)?;
        self.packets.clear();
        self.resident_frames = 0;
        self.last_sequence = 0;
        self.last_end = None;
        self.generation = generation;
        self.position = position;
        Ok(())
    }
}

fn frame_at(us: u64, rate: u32) -> Result<u64, CoreError> {
    let frame = u64::try_from((u128::from(us) * u128::from(rate) + 500_000) / 1_000_000)
        .map_err(|_| invalid("PCM timestamp overflow"))?;
    position_us(frame, rate)?;
    Ok(frame)
}

fn position_us(frame: u64, rate: u32) -> Result<u64, CoreError> {
    u64::try_from(u128::from(frame) * 1_000_000 / u128::from(rate))
        .map_err(|_| invalid("PCM timestamp overflow"))
}

fn invalid(message: &str) -> CoreError {
    CoreError::invalid("ASTRA_EMU_PCM_INVALID", message)
}
