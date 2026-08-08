use std::sync::Arc;

use crate::MediaError;

pub const CANONICAL_SAMPLE_RATE: u32 = 48_000;
pub const CANONICAL_CHANNELS: u16 = 2;

/// Decoder-owned canonical PCM moved into the selected audio service.
#[derive(Debug, Clone)]
pub struct PcmAsset {
    pub identity: String,
    pub samples: Arc<Vec<f32>>,
}

impl PcmAsset {
    pub fn from_canonical_samples(
        identity: impl Into<String>,
        samples: Vec<f32>,
    ) -> Result<Self, MediaError> {
        validate_canonical_samples(&samples)?;
        Ok(Self {
            identity: identity.into(),
            samples: Arc::new(samples),
        })
    }

    pub fn with_identity(&self, identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            samples: Arc::clone(&self.samples),
        }
    }

    pub fn frame_count(&self) -> usize {
        self.samples.len() / usize::from(CANONICAL_CHANNELS)
    }
}

fn validate_canonical_samples(samples: &[f32]) -> Result<(), MediaError> {
    if samples.is_empty()
        || !samples
            .len()
            .is_multiple_of(usize::from(CANONICAL_CHANNELS))
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(MediaError::Diagnostics(vec![
            astra_core::Diagnostic::error(
                "ASTRA_AUDIO_ASSET_INVALID",
                "canonical PCM asset is empty, misaligned, or non-finite",
            ),
        ]));
    }
    Ok(())
}
