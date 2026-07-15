use std::{fs, io::Cursor, path::Path, sync::Arc};

use astra_headless_protocol::{AudioMetrics, AudioTolerance, ImageMetrics, ImageTolerance};
use image::ImageReader;
use rustfft::{num_complex::Complex, Fft, FftPlanner};

pub(crate) fn compare_image(
    actual_rgba: &[u8],
    width: u32,
    height: u32,
    baseline: &Path,
    expected_hash: &str,
    tolerance: ImageTolerance,
) -> Result<(ImageMetrics, bool), String> {
    let bytes = fs::read(baseline)
        .map_err(|error| format!("ASTRA_HEADLESS_IMAGE_BASELINE_READ_FAILED: {error}"))?;
    verify_hash(
        &bytes,
        expected_hash,
        "ASTRA_HEADLESS_IMAGE_BASELINE_HASH_MISMATCH",
    )?;
    let decoded = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("ASTRA_HEADLESS_IMAGE_BASELINE_FORMAT_INVALID: {error}"))?
        .decode()
        .map_err(|error| format!("ASTRA_HEADLESS_IMAGE_BASELINE_DECODE_FAILED: {error}"))?
        .to_rgba8();
    if decoded.width() != width
        || decoded.height() != height
        || decoded.as_raw().len() != actual_rgba.len()
    {
        return Err("ASTRA_HEADLESS_IMAGE_DIMENSIONS_MISMATCH".into());
    }
    let expected = decoded.as_raw();
    let mut changed = 0_u64;
    let mut max_delta = 0_u8;
    for (actual, baseline) in actual_rgba.chunks_exact(4).zip(expected.chunks_exact(4)) {
        let mut pixel_changed = false;
        for channel in 0..4 {
            let delta = actual[channel].abs_diff(baseline[channel]);
            max_delta = max_delta.max(delta);
            pixel_changed |= delta != 0;
        }
        changed += u64::from(pixel_changed);
    }
    let pixels = u64::from(width) * u64::from(height);
    let changed_pixel_ratio = changed as f64 / pixels as f64;
    let ssim = global_ssim(actual_rgba, expected);
    let nonempty_bbox_offset_px = bbox_offset(actual_rgba, expected, width, height);
    let metrics = ImageMetrics {
        changed_pixel_ratio,
        max_channel_delta: max_delta,
        ssim,
        nonempty_bbox_offset_px,
    };
    let passed = changed_pixel_ratio <= tolerance.changed_pixel_ratio
        && max_delta <= tolerance.max_channel_delta
        && ssim >= tolerance.min_ssim
        && nonempty_bbox_offset_px <= tolerance.max_nonempty_bbox_offset_px;
    Ok((metrics, passed))
}

pub(crate) fn compare_audio_samples(
    actual_samples: &[f32],
    baseline: &Path,
    expected_hash: &str,
    tolerance: AudioTolerance,
) -> Result<(AudioMetrics, bool), String> {
    let baseline_bytes = fs::read(baseline)
        .map_err(|error| format!("ASTRA_HEADLESS_AUDIO_BASELINE_READ_FAILED: {error}"))?;
    verify_hash(
        &baseline_bytes,
        expected_hash,
        "ASTRA_HEADLESS_AUDIO_BASELINE_HASH_MISMATCH",
    )?;
    if actual_samples.len() % 2 != 0 || actual_samples.iter().any(|sample| !sample.is_finite()) {
        return Err("ASTRA_HEADLESS_AUDIO_SNAPSHOT_INVALID".into());
    }
    let actual = actual_samples
        .iter()
        .map(|sample| {
            let sample = sample.clamp(-1.0, 1.0);
            let scale = if sample < 0.0 { 32768.0 } else { 32767.0 };
            f64::from((sample * scale).round() as i16) / 32768.0
        })
        .collect();
    compare_audio_data((48_000, 2, actual), read_wav(&baseline_bytes)?, tolerance)
}

fn compare_audio_data(
    actual: (u32, u16, Vec<f64>),
    baseline: (u32, u16, Vec<f64>),
    tolerance: AudioTolerance,
) -> Result<(AudioMetrics, bool), String> {
    if actual.0 != baseline.0 || actual.1 != baseline.1 {
        return Err("ASTRA_HEADLESS_AUDIO_FORMAT_MISMATCH".into());
    }
    let duration_delta_ms =
        ((actual.2.len() as i64 - baseline.2.len() as i64).unsigned_abs() as f64) * 1000.0
            / f64::from(actual.0)
            / f64::from(actual.1);
    let (actual_peak, actual_rms) = level(&actual.2);
    let (baseline_peak, baseline_rms) = level(&baseline.2);
    let peak_delta_db = db_delta(actual_peak, baseline_peak);
    let rms_delta_db = db_delta(actual_rms, baseline_rms);
    let loudness_delta_lufs = level_delta(
        integrated_loudness_lufs(&actual.2),
        integrated_loudness_lufs(&baseline.2),
    );
    let normalized_spectrum_distance = spectrum_distance(&actual.2, &baseline.2);
    let silence_matches = (actual_peak < 1.0 / 32768.0) == (baseline_peak < 1.0 / 32768.0);
    let clipping_matches = (actual_peak >= 1.0) == (baseline_peak >= 1.0);
    let metrics = AudioMetrics {
        duration_delta_ms,
        peak_delta_db,
        rms_delta_db,
        loudness_delta_lufs,
        normalized_spectrum_distance,
        silence_matches,
        clipping_matches,
    };
    let passed = duration_delta_ms <= tolerance.max_duration_delta_ms
        && peak_delta_db <= tolerance.max_peak_delta_db
        && rms_delta_db <= tolerance.max_rms_delta_db
        && loudness_delta_lufs <= tolerance.max_loudness_delta_lufs
        && normalized_spectrum_distance <= tolerance.max_normalized_spectrum_distance
        && silence_matches
        && clipping_matches;
    Ok((metrics, passed))
}

fn read_wav(bytes: &[u8]) -> Result<(u32, u16, Vec<f64>), String> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes))
        .map_err(|error| format!("ASTRA_HEADLESS_WAV_INVALID: {error}"))?;
    let spec = reader.spec();
    if spec.sample_rate != 48_000
        || spec.channels != 2
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("ASTRA_HEADLESS_WAV_FORMAT_INVALID".into());
    }
    let samples = reader
        .samples::<i16>()
        .map(|sample| sample.map(|value| f64::from(value) / 32768.0))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("ASTRA_HEADLESS_WAV_SAMPLE_INVALID: {error}"))?;
    if samples.len() % usize::from(spec.channels) != 0 {
        return Err("ASTRA_HEADLESS_WAV_ALIGNMENT_INVALID".into());
    }
    Ok((spec.sample_rate, spec.channels, samples))
}

fn verify_hash(bytes: &[u8], expected: &str, code: &str) -> Result<(), String> {
    if astra_core::Hash256::from_sha256(bytes).to_string() != expected {
        return Err(code.into());
    }
    Ok(())
}

fn level(samples: &[f64]) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let peak = samples
        .iter()
        .fold(0.0_f64, |value, sample| value.max(sample.abs()));
    let rms =
        (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len() as f64).sqrt();
    (peak, rms)
}

fn db_delta(left: f64, right: f64) -> f64 {
    if left == 0.0 && right == 0.0 {
        0.0
    } else if left == 0.0 || right == 0.0 {
        999.0
    } else {
        (20.0 * (left / right).log10()).abs()
    }
}

fn level_delta(left: f64, right: f64) -> f64 {
    match (left.is_finite(), right.is_finite()) {
        (true, true) => (left - right).abs(),
        (false, false) if left.is_sign_negative() == right.is_sign_negative() => 0.0,
        _ => 999.0,
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BiquadState {
    fn process(&mut self, input: f64, b: [f64; 3], a: [f64; 2]) -> f64 {
        let output =
            b[0] * input + b[1] * self.x1 + b[2] * self.x2 - a[0] * self.y1 - a[1] * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

fn integrated_loudness_lufs(samples: &[f64]) -> f64 {
    const CHANNELS: usize = 2;
    const BLOCK_FRAMES: usize = 19_200;
    const STEP_FRAMES: usize = 4_800;
    const PRE_B: [f64; 3] = [
        1.535_124_859_586_97,
        -2.691_696_189_406_38,
        1.198_392_810_852_85,
    ];
    const PRE_A: [f64; 2] = [-1.690_659_293_182_41, 0.732_480_774_215_85];
    const RLB_B: [f64; 3] = [1.0, -2.0, 1.0];
    const RLB_A: [f64; 2] = [-1.990_047_454_833_98, 0.990_072_250_366_21];

    let frames = samples.len() / CHANNELS;
    if frames == 0 {
        return f64::NEG_INFINITY;
    }
    let mut pre = [BiquadState::default(); CHANNELS];
    let mut rlb = [BiquadState::default(); CHANNELS];
    let mut weighted = Vec::with_capacity(frames * CHANNELS);
    for frame in samples.chunks_exact(CHANNELS) {
        for channel in 0..CHANNELS {
            let value = pre[channel].process(frame[channel], PRE_B, PRE_A);
            weighted.push(rlb[channel].process(value, RLB_B, RLB_A));
        }
    }

    let mut block_energies = Vec::new();
    let mut start = 0_usize;
    loop {
        let available = frames.saturating_sub(start).min(BLOCK_FRAMES);
        let energy = weighted[start * CHANNELS..(start + available) * CHANNELS]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f64>()
            / BLOCK_FRAMES as f64;
        block_energies.push(energy);
        if start + BLOCK_FRAMES >= frames {
            break;
        }
        start += STEP_FRAMES;
    }

    let absolute = block_energies
        .iter()
        .copied()
        .filter(|energy| energy_to_lufs(*energy) > -70.0)
        .collect::<Vec<_>>();
    if absolute.is_empty() {
        return f64::NEG_INFINITY;
    }
    let absolute_mean = absolute.iter().sum::<f64>() / absolute.len() as f64;
    let relative_gate = energy_to_lufs(absolute_mean) - 10.0;
    let relative = absolute
        .into_iter()
        .filter(|energy| energy_to_lufs(*energy) > relative_gate)
        .collect::<Vec<_>>();
    if relative.is_empty() {
        return f64::NEG_INFINITY;
    }
    energy_to_lufs(relative.iter().sum::<f64>() / relative.len() as f64)
}

fn energy_to_lufs(energy: f64) -> f64 {
    if energy <= 0.0 {
        f64::NEG_INFINITY
    } else {
        -0.691 + 10.0 * energy.log10()
    }
}

fn spectrum_distance(left: &[f64], right: &[f64]) -> f64 {
    const WINDOW: usize = 1024;
    const HOP: usize = WINDOW / 2;
    const BINS: usize = 128;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let left_bins = aggregate_spectrum(left, Arc::clone(&fft), WINDOW, HOP, BINS);
    let right_bins = aggregate_spectrum(right, fft, WINDOW, HOP, BINS);
    left_bins
        .iter()
        .zip(right_bins)
        .map(|(left, right)| (left - right).abs())
        .sum::<f64>()
        * 0.5
}

fn aggregate_spectrum(
    source: &[f64],
    fft: Arc<dyn Fft<f64>>,
    window_size: usize,
    hop: usize,
    bins: usize,
) -> Vec<f64> {
    let frames = source.len() / 2;
    let mut output = vec![0.0; bins];
    if frames == 0 {
        return output;
    }
    let mut buffer = vec![Complex::new(0.0, 0.0); window_size];
    let mut start = 0_usize;
    loop {
        buffer.fill(Complex::new(0.0, 0.0));
        let available = frames.saturating_sub(start).min(window_size);
        for (offset, value) in buffer.iter_mut().take(available).enumerate() {
            let frame = start + offset;
            let mono = (source[frame * 2] + source[frame * 2 + 1]) * 0.5;
            let hann = 0.5
                - 0.5 * (std::f64::consts::TAU * offset as f64 / (window_size - 1) as f64).cos();
            value.re = mono * hann;
        }
        fft.process(&mut buffer);
        for (target, value) in output.iter_mut().zip(buffer.iter()).take(bins) {
            *target += value.norm();
        }
        if start + window_size >= frames {
            break;
        }
        start += hop;
    }
    let norm = output.iter().sum::<f64>();
    if norm > 0.0 {
        output.iter_mut().for_each(|value| *value /= norm);
    }
    output
}

fn global_ssim(left: &[u8], right: &[u8]) -> f64 {
    let luminance = |pixel: &[u8]| {
        0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])
    };
    let count = (left.len() / 4) as f64;
    let mean_left = left.chunks_exact(4).map(luminance).sum::<f64>() / count;
    let mean_right = right.chunks_exact(4).map(luminance).sum::<f64>() / count;
    let mut variance_left = 0.0;
    let mut variance_right = 0.0;
    let mut covariance = 0.0;
    for (left, right) in left.chunks_exact(4).zip(right.chunks_exact(4)) {
        let left = luminance(left) - mean_left;
        let right = luminance(right) - mean_right;
        variance_left += left * left;
        variance_right += right * right;
        covariance += left * right;
    }
    let denominator = (count - 1.0).max(1.0);
    variance_left /= denominator;
    variance_right /= denominator;
    covariance /= denominator;
    let c1 = (0.01_f64 * 255.0).powi(2);
    let c2 = (0.03_f64 * 255.0).powi(2);
    ((2.0 * mean_left * mean_right + c1) * (2.0 * covariance + c2)
        / ((mean_left.powi(2) + mean_right.powi(2) + c1) * (variance_left + variance_right + c2)))
        .clamp(-1.0, 1.0)
}

fn bbox_offset(left: &[u8], right: &[u8], width: u32, height: u32) -> u32 {
    let bbox = |bytes: &[u8]| {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for y in 0..height {
            for x in 0..width {
                let index = ((u64::from(y) * u64::from(width) + u64::from(x)) * 4) as usize;
                if bytes[index..index + 4] != [0, 0, 0, 0] {
                    bounds = Some(match bounds {
                        None => (x, y, x, y),
                        Some((min_x, min_y, max_x, max_y)) => {
                            (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                        }
                    });
                }
            }
        }
        bounds
    };
    match (bbox(left), bbox(right)) {
        (None, None) => 0,
        (Some(left), Some(right)) => [
            left.0.abs_diff(right.0),
            left.1.abs_diff(right.1),
            left.2.abs_diff(right.2),
            left.3.abs_diff(right.3),
        ]
        .into_iter()
        .max()
        .unwrap_or(0),
        _ => u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use astra_headless_protocol::{AudioTolerance, ImageTolerance};
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

    use super::{
        compare_audio_samples, compare_image, integrated_loudness_lufs, spectrum_distance,
    };

    #[astra_headless_test::test]
    fn image_comparison_checks_exact_pixels_and_declared_tolerance() {
        let temp = tempfile::tempdir().unwrap();
        let baseline = temp.path().join("baseline.png");
        let rgba = vec![10, 20, 30, 255, 40, 50, 60, 255];
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 2, 1, ExtendedColorType::Rgba8)
            .unwrap();
        fs::write(&baseline, &png).unwrap();
        let hash = astra_core::Hash256::from_sha256(&png).to_string();
        let (_, exact) = compare_image(
            &rgba,
            2,
            1,
            &baseline,
            &hash,
            ImageTolerance {
                changed_pixel_ratio: 0.0,
                max_channel_delta: 0,
                min_ssim: 1.0,
                max_nonempty_bbox_offset_px: 0,
            },
        )
        .unwrap();
        assert!(exact);
        let mut changed = rgba;
        changed[0] = 20;
        assert!(
            !compare_image(
                &changed,
                2,
                1,
                &baseline,
                &hash,
                ImageTolerance {
                    changed_pixel_ratio: 0.0,
                    max_channel_delta: 0,
                    min_ssim: 1.0,
                    max_nonempty_bbox_offset_px: 0,
                },
            )
            .unwrap()
            .1
        );
    }

    #[astra_headless_test::test]
    fn audio_comparison_quantizes_actual_pcm_to_canonical_wav() {
        let temp = tempfile::tempdir().unwrap();
        let baseline = temp.path().join("baseline.wav");
        let samples = (0..800)
            .flat_map(|frame| {
                let sample =
                    ((frame as f32 / 48_000.0) * 220.0 * std::f32::consts::TAU).sin() * 0.2;
                [sample, sample]
            })
            .collect::<Vec<_>>();
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut cursor,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 48_000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for sample in &samples {
                let scale = if *sample < 0.0 { 32768.0 } else { 32767.0 };
                writer
                    .write_sample((*sample * scale).round() as i16)
                    .unwrap();
            }
            writer.finalize().unwrap();
        }
        let wav = cursor.into_inner();
        fs::write(&baseline, &wav).unwrap();
        let hash = astra_core::Hash256::from_sha256(&wav).to_string();
        let (metrics, passed) =
            compare_audio_samples(&samples, &baseline, &hash, AudioTolerance::default()).unwrap();
        assert!(passed);
        assert_eq!(metrics.duration_delta_ms, 0.0);
        let truncated = &samples[..samples.len() - 200];
        assert!(
            !compare_audio_samples(truncated, &baseline, &hash, AudioTolerance::default(),)
                .unwrap()
                .1
        );
    }

    #[astra_headless_test::test]
    fn audio_analysis_uses_bs1770_loudness_and_the_complete_timeline() {
        let quiet = (0..48_000)
            .flat_map(|frame| {
                let sample =
                    ((frame as f64 / 48_000.0) * 440.0 * std::f64::consts::TAU).sin() * 0.05;
                [sample, sample]
            })
            .collect::<Vec<_>>();
        let loud = quiet.iter().map(|sample| sample * 2.0).collect::<Vec<_>>();
        let loudness_delta = integrated_loudness_lufs(&loud) - integrated_loudness_lufs(&quiet);
        assert!((loudness_delta - 6.020_599_913).abs() < 0.01);

        let mut tail_changed = quiet.clone();
        for frame in 24_000..48_000 {
            let sample = ((frame as f64 / 48_000.0) * 880.0 * std::f64::consts::TAU).sin() * 0.05;
            tail_changed[frame * 2] = sample;
            tail_changed[frame * 2 + 1] = sample;
        }
        assert!(spectrum_distance(&quiet, &tail_changed) > 0.05);
    }
}
