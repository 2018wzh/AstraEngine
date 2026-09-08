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

/// Host audio is a bounded, cancellable queue. `configure` is called during
/// family open and must complete before the family starts an audio worker.
#[sabi_trait]
pub trait AudioSink: Send + Sync {
    fn configure(&self, format: PcmFormatSpec) -> FfiFamilyResult<()>;
    fn write(&self, chunk: PcmChunk) -> FfiFamilyResult<AudioWriteStatus>;
    fn is_cancelled(&self) -> bool;
    fn cancel(&self) -> FfiFamilyResult<()>;
}

pub type AudioSinkBox = AudioSink_TO<'static, RBox<()>>;
