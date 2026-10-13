use std::{collections::BTreeMap, fs, path::Path};

use astra_headless_protocol::{
    ArtifactEntry, ArtifactManifest, CheckpointConfig, CheckpointResult, ProductSession,
};
use astra_platform::HeadlessHostProfile;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

use super::*;

pub(crate) async fn capture_checkpoint(
    product: &mut dyn ProductSession,
    id: &str,
    sequence: u64,
    observations: &[astra_headless_protocol::Observation],
    config: Option<&CheckpointConfig>,
    config_root: Option<&Path>,
    capture_audio: bool,
) -> Result<(CheckpointCapture, CheckpointResult), String> {
    let frame = product
        .capture_frame()
        .await
        .map_err(|error| error.to_string())?;
    let audio = if capture_audio {
        let audio = product.capture_audio().map_err(|error| error.to_string())?;
        if audio.sample_rate != 48_000
            || audio.channels != 2
            || !audio
                .samples
                .len()
                .is_multiple_of(usize::from(audio.channels))
            || audio.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err("ASTRA_HEADLESS_CHECKPOINT_AUDIO_FORMAT_INVALID".into());
        }
        Some(audio)
    } else {
        None
    };
    let observation_hash = astra_core::Hash256::from_sha256(
        &serde_json::to_vec(observations).map_err(|error| error.to_string())?,
    )
    .to_string();
    let expected = config.and_then(|config| config.checkpoints.iter().find(|item| item.id == id));
    let (image_metrics, image_passed) = match expected.and_then(|item| {
        item.image_baseline_path
            .as_deref()
            .zip(item.image_baseline_hash.as_deref())
            .map(|pair| (item, pair))
    }) {
        Some((expectation, (path, hash))) => {
            let root =
                config_root.ok_or_else(|| "ASTRA_HEADLESS_CHECKPOINT_ROOT_MISSING".to_string())?;
            let (metrics, passed) = compare::compare_image(
                &frame.rgba8,
                frame.width,
                frame.height,
                &root.join(path),
                hash,
                expectation.image_tolerance,
            )?;
            (Some(metrics), passed)
        }
        None => (None, true),
    };
    let observation_passed = expected
        .and_then(|item| item.observation_hash.as_ref())
        .is_none_or(|expected| expected == &observation_hash);
    Ok((
        CheckpointCapture {
            sequence,
            frame,
            audio,
        },
        CheckpointResult {
            id: id.to_owned(),
            passed: observation_passed && image_passed,
            observation_hash,
            image_metrics,
            audio_metrics: None,
        },
    ))
}

pub(crate) fn should_capture_checkpoint_audio(
    profile: &HeadlessHostProfile,
    config: Option<&CheckpointConfig>,
    checkpoint_id: &str,
) -> bool {
    !matches!(
        profile.artifacts.retention,
        astra_platform::HeadlessArtifactRetention::ManifestOnly
    ) || config
        .and_then(|config| {
            config
                .checkpoints
                .iter()
                .find(|checkpoint| checkpoint.id == checkpoint_id)
        })
        .is_some_and(|checkpoint| checkpoint.audio_baseline_path.is_some())
}

pub(crate) fn validate_required_checkpoints(
    config: Option<&CheckpointConfig>,
    profile_required: &[String],
    results: &[CheckpointResult],
) -> Result<(), String> {
    if let Some(failed) = results.iter().find(|result| !result.passed) {
        return Err(format!("ASTRA_HEADLESS_CHECKPOINT_FAILED: {}", failed.id));
    }
    let config_required = config
        .into_iter()
        .flat_map(|config| config.checkpoints.iter())
        .filter(|checkpoint| checkpoint.required)
        .map(|checkpoint| checkpoint.id.as_str());
    for expected_id in profile_required
        .iter()
        .map(String::as_str)
        .chain(config_required)
    {
        let result = results
            .iter()
            .find(|result| result.id == expected_id)
            .ok_or_else(|| format!("ASTRA_HEADLESS_REQUIRED_CHECKPOINT_MISSING: {expected_id}"))?;
        debug_assert!(result.passed);
    }
    Ok(())
}

pub(crate) fn append_checkpoint_artifacts(
    root: &Path,
    profile: &HeadlessHostProfile,
    captures: &BTreeMap<String, CheckpointCapture>,
    manifest: &mut ArtifactManifest,
    performance_observer: Option<&product_performance::ProductPerformanceRecorder>,
) -> Result<(), String> {
    if matches!(
        profile.artifacts.retention,
        astra_platform::HeadlessArtifactRetention::ManifestOnly
    ) {
        return Ok(());
    }
    let directory = root.join("checkpoints");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_ARTIFACT_ROOT_FAILED: {error}"))?;
    let selected = match profile.artifacts.retention {
        astra_platform::HeadlessArtifactRetention::Final => {
            captures.iter().next_back().into_iter().collect::<Vec<_>>()
        }
        _ => captures.iter().collect::<Vec<_>>(),
    };
    if let Some(observer) = performance_observer {
        observer.begin_cpu_scope("artifact.cpu", "checkpoint.encode_write", None)?;
    }
    for (id, capture) in selected {
        let frame = &capture.frame;
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                &frame.rgba8,
                frame.width,
                frame.height,
                ExtendedColorType::Rgba8,
            )
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_PNG_FAILED: {error}"))?;
        let audio = capture.audio.as_ref().ok_or_else(|| {
            "ASTRA_HEADLESS_CHECKPOINT_AUDIO_CAPTURE_MISSING: retained checkpoint requires audio"
                .to_string()
        })?;
        let audio_artifact = if audio.samples.is_empty() {
            None
        } else {
            let audio_bytes = wav_bytes(&audio.samples)?;
            let frame_count = (audio.samples.len() / usize::from(audio.channels)) as u64;
            let duration_ns = frame_count
                .checked_mul(1_000_000_000)
                .and_then(|value| value.checked_div(u64::from(audio.sample_rate)))
                .ok_or_else(|| "ASTRA_HEADLESS_CHECKPOINT_AUDIO_DURATION_OVERFLOW".to_string())?;
            Some((audio_bytes, frame_count, duration_ns))
        };
        let mut added_bytes = vec![bytes.len() as u64];
        if let Some((audio_bytes, _, _)) = &audio_artifact {
            added_bytes.push(audio_bytes.len() as u64);
        }
        reserve_appended_artifacts(profile, manifest, &added_bytes)?;
        let relative = format!("checkpoints/{id}.png");
        let path = root.join(&relative);
        let partial = path.with_extension("png.partial");
        let audio_relative = format!("checkpoints/{id}.wav");
        let audio_path = root.join(&audio_relative);
        let audio_partial = audio_path.with_extension("wav.partial");
        if let Err(error) = fs::write(&partial, &bytes).and_then(|_| {
            if let Some((audio_bytes, _, _)) = &audio_artifact {
                fs::write(&audio_partial, audio_bytes)
            } else {
                Ok(())
            }
        }) {
            let _ = fs::remove_file(&partial);
            let _ = fs::remove_file(&audio_partial);
            return Err(format!(
                "ASTRA_HEADLESS_CHECKPOINT_ARTIFACT_WRITE_FAILED: {error}"
            ));
        }
        if let Err(error) = fs::rename(&partial, &path).and_then(|_| {
            if audio_artifact.is_some() {
                fs::rename(&audio_partial, &audio_path)
            } else {
                Ok(())
            }
        }) {
            let _ = fs::remove_file(&partial);
            let _ = fs::remove_file(&audio_partial);
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(&audio_path);
            return Err(format!(
                "ASTRA_HEADLESS_CHECKPOINT_ARTIFACT_COMMIT_FAILED: {error}"
            ));
        }
        manifest.artifacts.push(ArtifactEntry::Frame {
            relative_path: relative,
            sha256: astra_core::Hash256::from_sha256(&bytes).to_string(),
            byte_size: bytes.len() as u64,
            width: frame.width,
            height: frame.height,
            color_space: "rgba8_srgb".into(),
            sequence: capture.sequence,
            checkpoint_ids: vec![id.clone()],
        });
        if let Some((audio_bytes, frame_count, duration_ns)) = audio_artifact {
            manifest.artifacts.push(ArtifactEntry::Audio {
                relative_path: audio_relative,
                sha256: astra_core::Hash256::from_sha256(&audio_bytes).to_string(),
                byte_size: audio_bytes.len() as u64,
                sample_rate: audio.sample_rate,
                channels: audio.channels,
                frame_count,
                duration_ns,
                checkpoint: Some(id.clone()),
            });
        }
    }
    if let Some(observer) = performance_observer {
        observer.end_cpu_scope("artifact.cpu", "checkpoint.encode_write", None)?;
    }
    Ok(())
}

pub(crate) fn reserve_appended_artifacts(
    profile: &HeadlessHostProfile,
    manifest: &ArtifactManifest,
    added_bytes: &[u64],
) -> Result<(), String> {
    let next_count = (manifest.artifacts.len() as u64)
        .checked_add(added_bytes.len() as u64)
        .ok_or_else(|| "ASTRA_HEADLESS_ARTIFACT_COUNT_LIMIT_EXCEEDED".to_string())?;
    if next_count > profile.artifacts.max_artifacts {
        return Err("ASTRA_HEADLESS_ARTIFACT_COUNT_LIMIT_EXCEEDED".into());
    }
    let current_bytes = manifest
        .artifacts
        .iter()
        .map(|entry| match entry {
            ArtifactEntry::Frame { byte_size, .. } | ArtifactEntry::Audio { byte_size, .. } => {
                *byte_size
            }
        })
        .sum::<u64>();
    let appended_bytes = added_bytes.iter().try_fold(0_u64, |total, bytes| {
        total
            .checked_add(*bytes)
            .ok_or_else(|| "ASTRA_HEADLESS_ARTIFACT_BYTE_LIMIT_EXCEEDED".to_string())
    })?;
    if current_bytes
        .checked_add(appended_bytes)
        .is_none_or(|total| total > profile.artifacts.max_total_bytes)
    {
        return Err("ASTRA_HEADLESS_ARTIFACT_BYTE_LIMIT_EXCEEDED".into());
    }
    Ok(())
}

pub(crate) fn apply_audio_comparisons(
    config: Option<&CheckpointConfig>,
    config_root: Option<&Path>,
    captures: &BTreeMap<String, CheckpointCapture>,
    results: &mut [CheckpointResult],
) -> Result<(), String> {
    let Some(config) = config else {
        return Ok(());
    };
    for result in results {
        let Some(expectation) = config.checkpoints.iter().find(|item| item.id == result.id) else {
            continue;
        };
        let Some((relative, hash)) = expectation
            .audio_baseline_path
            .as_deref()
            .zip(expectation.audio_baseline_hash.as_deref())
        else {
            continue;
        };
        let root =
            config_root.ok_or_else(|| "ASTRA_HEADLESS_CHECKPOINT_ROOT_MISSING".to_string())?;
        let actual = captures
            .get(&result.id)
            .ok_or_else(|| "ASTRA_HEADLESS_AUDIO_CHECKPOINT_MISSING".to_string())?;
        let audio = actual
            .audio
            .as_ref()
            .ok_or_else(|| "ASTRA_HEADLESS_AUDIO_CHECKPOINT_CAPTURE_MISSING".to_string())?;
        let (metrics, passed) = compare::compare_audio_samples(
            &audio.samples,
            &root.join(relative),
            hash,
            expectation.audio_tolerance,
        )?;
        result.audio_metrics = Some(metrics);
        result.passed &= passed;
    }
    Ok(())
}

pub(crate) fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_WAV_OPEN_FAILED: {error}"))?;
        for sample in samples {
            if !sample.is_finite() {
                return Err("ASTRA_HEADLESS_CHECKPOINT_WAV_SAMPLE_INVALID".into());
            }
            let sample = sample.clamp(-1.0, 1.0);
            let scale = if sample < 0.0 { 32768.0 } else { 32767.0 };
            writer
                .write_sample((sample * scale).round() as i16)
                .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_WAV_WRITE_FAILED: {error}"))?;
        }
        writer
            .finalize()
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_WAV_FINALIZE_FAILED: {error}"))?;
    }
    Ok(cursor.into_inner())
}
