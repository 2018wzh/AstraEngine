//! Native device conversion extracted from the Manager PCM output backend.
//! Mono/stereo sources use the front channels; additional device channels stay
//! silent. Downmix precedes resampling, expansion follows it. Storage is reused.
use rubato::{
    audioadapter_buffers::direct::SequentialSliceOfVecs, Async, FixedAsync, PolynomialDegree,
    Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

const BLOCK_FRAMES: usize = 1024;
const MAX_OUTPUT_SAMPLES: usize = 1_048_576;

pub struct NativeAudioConverter {
    source_channels: usize,
    target_channels: usize,
    resampler: Option<Box<dyn Resampler<f32>>>,
    pending: Vec<Vec<f32>>,
    planar_output: Vec<Vec<f32>>,
    interleaved: Vec<f32>,
}
impl NativeAudioConverter {
    pub fn new(
        source_rate: u32,
        source_channels: u16,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<Self, String> {
        if source_rate == 0 || target_rate == 0 || source_channels == 0 || target_channels == 0 {
            return Err("ASTRA_EMU_AUDIO_CHANNELS".into());
        }
        if source_channels > 2 && source_channels != target_channels {
            return Err("ASTRA_EMU_AUDIO_CHANNEL_LAYOUT_UNSUPPORTED".into());
        }
        if source_rate == target_rate && source_channels == target_channels {
            return Ok(Self {
                source_channels: usize::from(source_channels),
                target_channels: usize::from(target_channels),
                resampler: None,
                pending: Vec::new(),
                planar_output: Vec::new(),
                interleaved: Vec::new(),
            });
        }
        let target_channels = usize::from(target_channels);
        let processing_channels = usize::from(source_channels).min(target_channels);
        let resampler = if source_rate == target_rate {
            None
        } else {
            Some(new_resampler(
                source_rate,
                target_rate,
                processing_channels,
            )?)
        };
        let output_frames = resampler
            .as_ref()
            .map_or(BLOCK_FRAMES, |resampler| resampler.output_frames_max());
        let output_samples = output_frames
            .checked_mul(target_channels)
            .filter(|size| *size <= MAX_OUTPUT_SAMPLES)
            .ok_or("ASTRA_EMU_AUDIO_CONVERSION_BUDGET")?;
        let planar_output = if resampler.is_some() {
            vec![vec![0.0; output_frames]; processing_channels]
        } else {
            Vec::new()
        };
        Ok(Self {
            source_channels: usize::from(source_channels),
            target_channels,
            resampler,
            pending: (0..processing_channels)
                .map(|_| Vec::with_capacity(BLOCK_FRAMES))
                .collect(),
            planar_output,
            interleaved: Vec::with_capacity(output_samples),
        })
    }
    pub fn needs_conversion(&self) -> bool {
        self.resampler.is_some() || self.source_channels != self.target_channels
    }
    pub fn output_delay(&self) -> usize {
        self.resampler
            .as_ref()
            .map_or(0, |resampler| resampler.output_delay())
    }

    /// Emit each completed output block synchronously. The slice is borrowed
    /// only for the callback; the device queue must own any deferred samples.
    pub fn convert(
        &mut self,
        samples: &[f32],
        mut emit: impl FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<(), String> {
        if !samples.len().is_multiple_of(self.source_channels) {
            return Err("ASTRA_EMU_AUDIO_ALIGNMENT".into());
        }
        if samples.len() > MAX_OUTPUT_SAMPLES || samples.iter().any(|sample| !sample.is_finite()) {
            return Err("ASTRA_EMU_AUDIO_VALUE".into());
        }
        if !self.needs_conversion() {
            return emit(samples);
        }
        let mut remaining = samples;
        while !remaining.is_empty() {
            let required = self
                .resampler
                .as_ref()
                .map_or(BLOCK_FRAMES, |resampler| resampler.input_frames_next());
            let count =
                (required - self.pending[0].len()).min(remaining.len() / self.source_channels);
            let (input, rest) = remaining.split_at(count * self.source_channels);
            let processing_channels = self.pending.len();
            append_channel_map(
                &mut self.pending,
                input,
                self.source_channels,
                processing_channels,
            );
            remaining = rest;
            if self.resampler.is_some() && self.pending[0].len() < required {
                break;
            }
            self.interleaved.clear();
            if let Some(resampler) = self.resampler.as_mut() {
                let input =
                    SequentialSliceOfVecs::new(&self.pending, processing_channels, required)
                        .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_INPUT:{error}"))?;
                let max_output = self.planar_output[0].len();
                let mut output = SequentialSliceOfVecs::new_mut(
                    &mut self.planar_output,
                    processing_channels,
                    max_output,
                )
                .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_OUTPUT:{error}"))?;
                let (consumed, produced) = resampler
                    .process_into_buffer(&input, &mut output, None)
                    .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLE:{error}"))?;
                if consumed != required {
                    return Err("ASTRA_EMU_AUDIO_RESAMPLE_ALIGNMENT".into());
                }
                interleave(
                    &self.planar_output,
                    produced,
                    self.target_channels,
                    &mut self.interleaved,
                );
            } else {
                interleave(
                    &self.pending,
                    self.pending[0].len(),
                    self.target_channels,
                    &mut self.interleaved,
                );
            }
            for channel in &mut self.pending {
                channel.clear();
            }
            emit(&self.interleaved)?;
        }
        Ok(())
    }
}
fn append_channel_map(pending: &mut [Vec<f32>], samples: &[f32], source: usize, target: usize) {
    match (source, target) {
        (source, target) if source == target => {
            for frame in samples.chunks_exact(source) {
                for (output, sample) in pending.iter_mut().zip(frame) {
                    output.push(*sample);
                }
            }
        }
        (2, 1) => {
            for frame in samples.as_chunks::<2>().0 {
                pending[0].push((frame[0] + frame[1]) * 0.5);
            }
        }
        _ => unreachable!("constructor validates the channel map"),
    }
}

fn interleave(planar: &[Vec<f32>], frames: usize, target_channels: usize, output: &mut Vec<f32>) {
    let duplicate_mono = planar.len() == 1 && target_channels > 1;
    let silent_channels = target_channels - planar.len() - usize::from(duplicate_mono);
    for frame in 0..frames {
        for channel in planar {
            output.push(channel[frame]);
        }
        if duplicate_mono {
            output.push(planar[0][frame]);
        }
        output.resize(output.len() + silent_channels, 0.0);
    }
}
fn new_resampler(
    source_rate: u32,
    target_rate: u32,
    channels: usize,
) -> Result<Box<dyn Resampler<f32>>, String> {
    let ratio = f64::from(target_rate) / f64::from(source_rate);
    if target_rate > source_rate {
        Ok(Box::new(
            Async::<f32>::new_poly(
                ratio,
                1.0,
                PolynomialDegree::Septic,
                BLOCK_FRAMES,
                channels,
                FixedAsync::Input,
            )
            .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_CREATE:{error}"))?,
        ))
    } else {
        Ok(Box::new(
            Async::<f32>::new_sinc(
                ratio,
                1.0,
                &SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: Some(0.95),
                    interpolation: SincInterpolationType::Cubic,
                    oversampling_factor: 256,
                    window: WindowFunction::BlackmanHarris2,
                },
                BLOCK_FRAMES,
                channels,
                FixedAsync::Input,
            )
            .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_CREATE:{error}"))?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_borrows_input_and_stereo_maps_to_mono() {
        let input = [1.0, -1.0, 0.5, 0.5];
        NativeAudioConverter::new(48000, 2, 48000, 2)
            .unwrap()
            .convert(&input, |output| {
                assert_eq!(output.as_ptr(), input.as_ptr());
                Ok(())
            })
            .unwrap();
        let mut output = Vec::new();
        NativeAudioConverter::new(48000, 2, 48000, 1)
            .unwrap()
            .convert(&input, |chunk| {
                output.extend_from_slice(chunk);
                Ok(())
            })
            .unwrap();
        assert_eq!(output, [0.0, 0.5]);
    }
    #[test]
    fn streaming_resampling_keeps_fractional_frame_accounting() {
        for rate in [44100, 96000] {
            let mut converter = NativeAudioConverter::new(48000, 2, rate, 2).unwrap();
            let mut count = 0;
            let input = vec![0.25; 1024];
            for _ in 0..96 {
                converter
                    .convert(&input, |output| {
                        assert!(output.iter().all(|v| v.is_finite()));
                        count += output.len() / 2;
                        Ok(())
                    })
                    .unwrap();
            }
            let expected = 49152usize * rate as usize / 48000;
            assert!(count.abs_diff(expected) <= converter.output_delay() + 2);
        }
    }
    #[test]
    fn front_channels_preserve_stereo_and_mono_on_surround_devices() {
        for (channels, input, front) in [
            (2, vec![0.25, -0.75], [0.25, -0.75]),
            (1, vec![0.5], [0.5, 0.5]),
        ] {
            for target in [6, 8] {
                let mut output = Vec::new();
                NativeAudioConverter::new(48000, channels, 48000, target)
                    .unwrap()
                    .convert(&input, |chunk| {
                        output.extend_from_slice(chunk);
                        Ok(())
                    })
                    .unwrap();
                let mut expected = vec![0.0; usize::from(target)];
                expected[..2].copy_from_slice(&front);
                assert_eq!(output, expected);
            }
        }
    }
    #[test]
    fn surround_resampling_preserves_front_channels_and_silent_rears() {
        let input: Vec<_> = (0..1024).flat_map(|_| [0.25, -0.75]).collect();
        let mut converter = NativeAudioConverter::new(48000, 2, 44100, 8).unwrap();
        let mut last = [0.0; 2];
        for _ in 0..8 {
            converter
                .convert(&input, |chunk| {
                    for frame in chunk.as_chunks::<8>().0 {
                        assert!(frame[2..].iter().all(|sample| *sample == 0.0));
                        assert!((frame[0] * 3.0 + frame[1]).abs() < 1e-5);
                        last.copy_from_slice(&frame[..2]);
                    }
                    Ok(())
                })
                .unwrap();
        }
        assert!((last[0] - 0.25).abs() < 1e-4);
        assert!((last[1] + 0.75).abs() < 1e-4);
    }
    #[test]
    fn unsupported_channel_layout_and_nonfinite_samples_fail() {
        assert!(NativeAudioConverter::new(48000, 6, 48000, 2).is_err());
        assert!(NativeAudioConverter::new(48000, 2, 44100, 2)
            .unwrap()
            .convert(&[f32::NAN, 0.0], |_| Ok(()))
            .is_err());
    }
}
