//! Shared PCM format contract used by decode output validation and the
//! canonical decoded-audio type. Keeping the ranges and alignment rules in
//! one place prevents the decode layer and the Player audio contract from
//! drifting apart.

pub(crate) const MIN_PCM_SAMPLE_RATE: u32 = 8_000;
pub(crate) const MAX_PCM_SAMPLE_RATE: u32 = 384_000;
pub(crate) const MIN_PCM_CHANNELS: u16 = 1;
pub(crate) const MAX_PCM_CHANNELS: u16 = 8;

pub(crate) fn is_supported_pcm_rate(sample_rate: u32) -> bool {
    (MIN_PCM_SAMPLE_RATE..=MAX_PCM_SAMPLE_RATE).contains(&sample_rate)
}

pub(crate) fn is_supported_pcm_channel_count(channels: u16) -> bool {
    (MIN_PCM_CHANNELS..=MAX_PCM_CHANNELS).contains(&channels)
}

pub(crate) fn is_aligned_pcm_sample_count(sample_count: usize, channels: u16) -> bool {
    sample_count.is_multiple_of(usize::from(channels))
}

pub(crate) fn contains_non_finite_sample(samples: &[f32]) -> bool {
    samples.iter().any(|sample| !sample.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_format_bounds() {
        assert!(is_supported_pcm_rate(8_000));
        assert!(is_supported_pcm_rate(384_000));
        assert!(!is_supported_pcm_rate(7_999));
        assert!(!is_supported_pcm_rate(384_001));
        assert!(is_supported_pcm_channel_count(1));
        assert!(is_supported_pcm_channel_count(8));
        assert!(!is_supported_pcm_channel_count(0));
        assert!(!is_supported_pcm_channel_count(9));
    }

    #[test]
    fn pcm_alignment_and_finiteness() {
        assert!(is_aligned_pcm_sample_count(6, 2));
        assert!(!is_aligned_pcm_sample_count(5, 2));
        assert!(!contains_non_finite_sample(&[0.0, -1.0]));
        assert!(contains_non_finite_sample(&[0.0, f32::NAN]));
        assert!(contains_non_finite_sample(&[f32::INFINITY]));
    }
}
