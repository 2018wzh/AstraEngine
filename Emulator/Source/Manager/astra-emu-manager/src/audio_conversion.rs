use astra_emu_family_api::{PcmChunk, PcmFormatSpec};
use rubato::{
    audioadapter::Adapter, audioadapter_buffers::direct::SequentialSliceOfVecs, Async, FixedAsync,
    PolynomialDegree, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

use super::MAX_AUDIO_OUTPUT_SAMPLES;

#[derive(Clone, Copy)]
pub(super) struct OutputFormat {
    pub(super) source: PcmFormatSpec,
    pub(super) device_rate: u32,
    pub(super) device_channels: u16,
    pub(super) device_sample_format: cpal::SampleFormat,
}

pub(super) struct AudioConverter {
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
    target_channels: u16,
    pending: Vec<Vec<f32>>,
    resampler: Option<Box<dyn Resampler<f32>>>,
}

impl AudioConverter {
    pub(super) fn new(output: OutputFormat) -> Result<Self, String> {
        let source_channels = output.source.channels;
        let target_channels = output.device_channels;
        if source_channels == 0 || target_channels == 0 {
            return Err("ASTRA_EMU_AUDIO_CHANNELS".into());
        }
        if !matches!((source_channels, target_channels), (1, 1) | (1, 2) | (2, 1))
            && source_channels != target_channels
        {
            return Err("ASTRA_EMU_AUDIO_CHANNEL_LAYOUT_UNSUPPORTED".into());
        }
        let resampler = if output.source.sample_rate == output.device_rate {
            None
        } else {
            Some(new_resampler(
                output.source.sample_rate,
                output.device_rate,
                usize::from(target_channels),
            )?)
        };
        Ok(Self {
            source_rate: output.source.sample_rate,
            source_channels,
            target_rate: output.device_rate,
            target_channels,
            pending: vec![Vec::new(); usize::from(target_channels)],
            resampler,
        })
    }

    pub(super) fn push(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, String> {
        let source_channels = usize::from(self.source_channels);
        if !samples.len().is_multiple_of(source_channels) {
            return Err("ASTRA_EMU_AUDIO_ALIGNMENT".into());
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err("ASTRA_EMU_AUDIO_VALUE".into());
        }
        let input_frames = samples.len() / source_channels;
        let projected_frames = self
            .pending
            .first()
            .map_or(0, Vec::len)
            .checked_add(input_frames)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_CONVERSION_OVERFLOW".to_owned())?;
        let projected_samples = ((projected_frames as u128)
            .saturating_mul(u128::from(self.target_rate))
            .div_ceil(u128::from(self.source_rate)))
        .saturating_mul(u128::from(self.target_channels));
        if projected_samples > u128::from(MAX_AUDIO_OUTPUT_SAMPLES as u64) {
            return Err("ASTRA_EMU_AUDIO_CONVERSION_BUDGET".into());
        }
        append_channel_map(
            &mut self.pending,
            &samples,
            self.source_channels,
            self.target_channels,
        )?;
        if self.resampler.is_none() {
            return drain_interleaved(&mut self.pending, self.target_channels);
        }
        let required = self
            .resampler
            .as_ref()
            .expect("resampler checked above")
            .input_frames_next();
        let mut output = Vec::new();
        while self.pending.first().map_or(0, Vec::len) >= required {
            let input = self
                .pending
                .iter_mut()
                .map(|channel| channel.drain(..required).collect::<Vec<_>>())
                .collect::<Vec<_>>();
            let adapter =
                SequentialSliceOfVecs::new(&input, usize::from(self.target_channels), required)
                    .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_INPUT:{error}"))?;
            let converted = self
                .resampler
                .as_mut()
                .expect("resampler checked above")
                .process(&adapter, None)
                .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLE:{error}"))?;
            for frame in 0..converted.frames() {
                for channel in 0..converted.channels() {
                    output.push(
                        converted
                            .read_sample(channel, frame)
                            .ok_or_else(|| "ASTRA_EMU_AUDIO_RESAMPLE_READ".to_owned())?,
                    );
                }
            }
        }
        if output.is_empty() {
            return Ok(output);
        }
        if output.len() > MAX_AUDIO_OUTPUT_SAMPLES {
            return Err("ASTRA_EMU_AUDIO_CONVERSION_BUDGET".into());
        }
        Ok(output)
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
                1_024,
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
                1_024,
                channels,
                FixedAsync::Input,
            )
            .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_CREATE:{error}"))?,
        ))
    }
}

fn append_channel_map(
    pending: &mut [Vec<f32>],
    samples: &[f32],
    source_channels: u16,
    target_channels: u16,
) -> Result<(), String> {
    let source_channels = usize::from(source_channels);
    let target_channels = usize::from(target_channels);
    if pending.len() != target_channels {
        return Err("ASTRA_EMU_AUDIO_CHANNELS".into());
    }
    let frames = samples.len() / source_channels;
    match (source_channels, target_channels) {
        (source, target) if source == target => {
            for frame in 0..frames {
                for channel in 0..target {
                    pending[channel].push(samples[frame * source + channel]);
                }
            }
        }
        (1, 2) => {
            for sample in samples {
                pending[0].push(*sample);
                pending[1].push(*sample);
            }
        }
        (2, 1) => {
            for frame in 0..frames {
                pending[0].push((samples[frame * 2] + samples[frame * 2 + 1]) * 0.5);
            }
        }
        _ => return Err("ASTRA_EMU_AUDIO_CHANNEL_LAYOUT_UNSUPPORTED".into()),
    }
    Ok(())
}

fn drain_interleaved(pending: &mut [Vec<f32>], channels: u16) -> Result<Vec<f32>, String> {
    let frames = pending.first().map_or(0, Vec::len);
    if pending.iter().any(|channel| channel.len() != frames) {
        return Err("ASTRA_EMU_AUDIO_CHANNEL_ALIGNMENT".into());
    }
    let output_len = frames
        .checked_mul(usize::from(channels))
        .ok_or_else(|| "ASTRA_EMU_AUDIO_CONVERSION_OVERFLOW".to_owned())?;
    if output_len > MAX_AUDIO_OUTPUT_SAMPLES {
        return Err("ASTRA_EMU_AUDIO_CONVERSION_BUDGET".into());
    }
    let mut output = Vec::with_capacity(output_len);
    for frame in 0..frames {
        for channel in pending.iter() {
            output.push(channel[frame]);
        }
    }
    for channel in pending {
        channel.clear();
    }
    Ok(output)
}

pub(super) fn pcm_chunk_samples(chunk: PcmChunk) -> Vec<f32> {
    match chunk {
        PcmChunk::I16(values) => values
            .iter()
            .map(|sample| {
                if *sample < 0 {
                    f32::from(*sample) / 32_768.0
                } else {
                    f32::from(*sample) / 32_767.0
                }
            })
            .collect::<Vec<_>>(),
        PcmChunk::F32(values) => values.into_iter().collect(),
    }
}

#[cfg(test)]
pub(super) fn convert_chunk(chunk: PcmChunk, output: OutputFormat) -> Result<Vec<f32>, String> {
    let samples = pcm_chunk_samples(chunk);
    convert_samples(
        samples,
        output.source.sample_rate,
        output.source.channels,
        output.device_rate,
        output.device_channels,
    )
}

#[cfg(test)]
pub(super) fn convert_samples(
    samples: Vec<f32>,
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
    target_channels: u16,
) -> Result<Vec<f32>, String> {
    if source_channels == 0 || target_channels == 0 {
        return Err("ASTRA_EMU_AUDIO_CHANNELS".into());
    }
    let source_channels_usize = usize::from(source_channels);
    if !samples.len().is_multiple_of(source_channels_usize) {
        return Err("ASTRA_EMU_AUDIO_ALIGNMENT".into());
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err("ASTRA_EMU_AUDIO_VALUE".into());
    }
    let mut planar = vec![Vec::new(); usize::from(target_channels)];
    append_channel_map(&mut planar, &samples, source_channels, target_channels)?;
    if source_rate == target_rate {
        return drain_interleaved(&mut planar, target_channels);
    }
    let frames = samples.len() / source_channels_usize;
    let adapter = SequentialSliceOfVecs::new(&planar, usize::from(target_channels), frames)
        .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLER_INPUT:{error}"))?;
    let mut resampler = new_resampler(source_rate, target_rate, usize::from(target_channels))?;
    let output = resampler
        .process_all(&adapter, frames, None)
        .map_err(|error| format!("ASTRA_EMU_AUDIO_RESAMPLE:{error}"))?;
    let output_len = output
        .frames()
        .checked_mul(output.channels())
        .ok_or_else(|| "ASTRA_EMU_AUDIO_CONVERSION_OVERFLOW".to_owned())?;
    if output_len == 0 || output_len > MAX_AUDIO_OUTPUT_SAMPLES {
        return Err("ASTRA_EMU_AUDIO_CONVERSION_BUDGET".into());
    }
    let mut interleaved = Vec::with_capacity(output_len);
    for frame in 0..output.frames() {
        for channel in 0..output.channels() {
            interleaved.push(
                output
                    .read_sample(channel, frame)
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_RESAMPLE_READ".to_owned())?,
            );
        }
    }
    Ok(interleaved)
}
