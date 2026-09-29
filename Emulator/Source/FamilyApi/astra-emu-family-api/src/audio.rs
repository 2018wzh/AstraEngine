#![allow(non_local_definitions)]

use abi_stable::{
    sabi_trait,
    std_types::{RBox, RVec},
    StableAbi,
};

use super::descriptor::{FamilyError, FamilyResult, FfiFamilyResult};

pub const MAX_AUDIO_SAMPLES_PER_CHUNK: usize = 1_048_576;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum PcmFormat {
    I16,
    F32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct PcmFormatSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: PcmFormat,
}

impl PcmFormatSpec {
    pub fn validate(&self) -> FamilyResult<()> {
        if !(8_000..=192_000).contains(&self.sample_rate) || self.channels == 0 {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_PCM_FORMAT",
                "PCM sample rate or channel count is outside the supported range",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub enum PcmChunk {
    I16(RVec<i16>),
    F32(RVec<f32>),
}

impl PcmChunk {
    pub fn sample_count(&self) -> usize {
        match self {
            Self::I16(s) => s.len(),
            Self::F32(s) => s.len(),
        }
    }
    pub fn format(&self) -> PcmFormat {
        match self {
            Self::I16(_) => PcmFormat::I16,
            Self::F32(_) => PcmFormat::F32,
        }
    }

    pub fn validate(&self, opened: PcmFormatSpec) -> FamilyResult<()> {
        opened.validate()?;
        if self.format() != opened.format || self.sample_count() > MAX_AUDIO_SAMPLES_PER_CHUNK {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_PCM_CHUNK",
                "PCM chunk format or sample count is invalid",
            ));
        }
        if let Self::F32(samples) = self {
            if samples.iter().any(|sample| !sample.is_finite()) {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_PCM_VALUE",
                    "F32 PCM samples must be finite",
                ));
            }
        }
        let channels = usize::from(opened.channels);
        if !self.sample_count().is_multiple_of(channels) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_PCM_ALIGNMENT",
                "PCM sample count is not aligned to the opened channel count",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum AudioWriteStatus {
    Accepted,
    Cancelled,
    Closed,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum AudioPlaybackClock {
    /// Most recent device callback timestamp; never extrapolated by a query.
    DeviceTimestamp,
    /// Explicit silent test output, counted when frames are consumed.
    TestConsumption,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct AudioPlaybackProgress {
    /// Successfully accepted frames in the configured source PCM format.
    pub accepted_source_frames: u64,
    /// Source frames reached by the most recent playback observation.
    pub played_source_frames: u64,
    pub clock: AudioPlaybackClock,
}

/// Host audio is a bounded, cancellable queue. `configure` is called during
/// family open and must complete before the family starts an audio worker.
#[sabi_trait]
pub trait AudioSink: Send + Sync {
    fn configure(&self, format: PcmFormatSpec) -> FfiFamilyResult<()>;
    fn write(&self, chunk: PcmChunk) -> FfiFamilyResult<AudioWriteStatus>;
    fn is_cancelled(&self) -> bool;
    fn cancel(&self) -> FfiFamilyResult<()>;
    /// Optional playback observation. Hosts without an appropriate device
    /// timestamp must explicitly reject this query; zero is not a fallback.
    fn playback_progress(&self) -> FfiFamilyResult<AudioPlaybackProgress> {
        Err(FamilyError::new(
            "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED",
            "this audio sink does not expose playback progress",
        ))
        .into()
    }
}

pub type AudioSinkBox = AudioSink_TO<'static, RBox<()>>;

#[cfg(test)]
mod tests {
    use super::*;
    use abi_stable::type_level::downcasting::TD_Opaque;

    struct BasicSink;
    impl AudioSink for BasicSink {
        fn configure(&self, _: PcmFormatSpec) -> FfiFamilyResult<()> {
            Ok(()).into()
        }
        fn write(&self, _: PcmChunk) -> FfiFamilyResult<AudioWriteStatus> {
            Ok(AudioWriteStatus::Accepted).into()
        }
        fn is_cancelled(&self) -> bool {
            false
        }
        fn cancel(&self) -> FfiFamilyResult<()> {
            Ok(()).into()
        }
    }

    #[test]
    fn basic_sink_works_through_v10_vtable_but_progress_is_explicitly_unsupported() {
        let sink = AudioSink_TO::from_value(BasicSink, TD_Opaque);
        sink.configure(PcmFormatSpec {
            sample_rate: 48_000,
            channels: 2,
            format: PcmFormat::I16,
        })
        .into_result()
        .unwrap();
        assert_eq!(
            sink.write(PcmChunk::I16(vec![1, 2].into()))
                .into_result()
                .unwrap(),
            AudioWriteStatus::Accepted
        );
        assert_eq!(
            sink.playback_progress().into_result().unwrap_err().code(),
            "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED"
        );
        assert!(!sink.is_cancelled());
    }
}
