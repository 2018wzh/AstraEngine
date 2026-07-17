use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use astra_headless_protocol::{
    ArtifactEntry, ArtifactManifest, RendererExecutionIdentity, HEADLESS_ARTIFACT_MANIFEST_SCHEMA,
};
use astra_platform::{
    HeadlessArtifactPolicy, HeadlessArtifactRetention, HeadlessHostProfile, PlatformError,
    PlatformErrorCode,
};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use sha2::{Digest, Sha256};

pub(crate) struct ArtifactRecorder {
    root: PathBuf,
    policy: HeadlessArtifactPolicy,
    manifest: ArtifactManifest,
    total_bytes: u64,
    submitted_frame_count: u64,
    rasterized_frame_count: u64,
    audio_frames: u64,
    audio_artifact_count: u64,
    final_frame: Option<(u64, u32, u32, Vec<u8>)>,
    submitted_scene_digest: Sha256,
    rasterized_frame_digest: Sha256,
    audio_digest: Sha256,
    audio_square_sum: f64,
    audio_sample_count: u64,
    audio_peak: f64,
}

impl ArtifactRecorder {
    pub(crate) fn set_renderer_identity(
        &mut self,
        identity: RendererExecutionIdentity,
    ) -> Result<(), PlatformError> {
        identity
            .validate()
            .map_err(|_| integrity("artifact.renderer_identity", "renderer identity is invalid"))?;
        if self.manifest.rasterized_frame_count != 0 {
            return Err(integrity(
                "artifact.renderer_identity",
                "renderer identity cannot change after rasterization",
            ));
        }
        let encoded = serde_json::to_vec(&identity).map_err(|_| {
            integrity(
                "artifact.renderer_identity",
                "renderer identity encoding failed",
            )
        })?;
        self.manifest.renderer_identity_hash =
            astra_core::Hash256::from_sha256(&encoded).to_string();
        self.manifest.renderer_identity = identity;
        self.commit_manifest()
    }

    pub(crate) fn new(
        root: PathBuf,
        profile: &HeadlessHostProfile,
        input_sequence_hash: String,
    ) -> Result<Self, PlatformError> {
        if !is_hash(&input_sequence_hash) {
            return Err(integrity(
                "artifact.input_identity",
                "input sequence identity is not a full sha256",
            ));
        }
        fs::create_dir_all(root.join("frames"))
            .and_then(|_| fs::create_dir_all(root.join("audio")))
            .map_err(|_| io_error("artifact.open"))?;
        let providers = serde_json::to_vec(&profile.providers)
            .map_err(|_| io_error("artifact.provider_identity"))?;
        Ok(Self {
            root,
            policy: profile.artifacts.clone(),
            manifest: ArtifactManifest {
                schema: HEADLESS_ARTIFACT_MANIFEST_SCHEMA.into(),
                run_id: profile.artifacts.namespace.clone(),
                build_fingerprint: profile.build_fingerprint.clone(),
                package_hash: profile.package_hash.clone(),
                input_sequence_hash,
                provider_identity_hash: astra_core::Hash256::from_sha256(&providers).to_string(),
                renderer_identity_hash: RendererExecutionIdentity::cpu_reference().hash().map_err(
                    |_| integrity("artifact.renderer_identity", "renderer identity is invalid"),
                )?,
                renderer_identity: RendererExecutionIdentity::cpu_reference(),
                render_policy: match profile.render_policy {
                    astra_platform::HeadlessRenderPolicy::All => "all",
                    astra_platform::HeadlessRenderPolicy::Checkpoints => "checkpoints",
                }
                .into(),
                submitted_frame_count: 0,
                rasterized_frame_count: 0,
                audio_frame_count: 0,
                submitted_scene_stream_hash: empty_hash(),
                rasterized_frame_stream_hash: empty_hash(),
                audio_stream_hash: empty_hash(),
                audio_peak_dbfs: None,
                audio_rms_dbfs: None,
                silence: true,
                clipping: false,
                artifacts: Vec::new(),
            },
            total_bytes: 0,
            submitted_frame_count: 0,
            rasterized_frame_count: 0,
            audio_frames: 0,
            audio_artifact_count: 0,
            final_frame: None,
            submitted_scene_digest: Sha256::new(),
            rasterized_frame_digest: Sha256::new(),
            audio_digest: Sha256::new(),
            audio_square_sum: 0.0,
            audio_sample_count: 0,
            audio_peak: 0.0,
        })
    }

    pub(crate) fn record_submission(
        &mut self,
        sequence: u64,
        scene_bytes: &[u8],
    ) -> Result<(), PlatformError> {
        let next_count = self
            .submitted_frame_count
            .checked_add(1)
            .ok_or_else(|| limit("artifact.scene", "submitted frame counter overflowed"))?;
        if next_count > self.policy.max_submitted_frames {
            return Err(limit("artifact.scene", "submitted frame limit exceeded"));
        }
        self.submitted_frame_count = next_count;
        self.submitted_scene_digest.update(sequence.to_le_bytes());
        self.submitted_scene_digest
            .update((scene_bytes.len() as u64).to_le_bytes());
        self.submitted_scene_digest.update(scene_bytes);
        self.refresh_analysis();
        Ok(())
    }

    pub(crate) fn record_rasterized_frame(
        &mut self,
        sequence: u64,
        width: u32,
        height: u32,
        rgba8: &[u8],
    ) -> Result<(), PlatformError> {
        let next_count = self
            .rasterized_frame_count
            .checked_add(1)
            .ok_or_else(|| limit("artifact.frame", "frame counter overflowed"))?;
        if next_count > self.policy.max_rasterized_frames {
            return Err(limit("artifact.frame", "rasterized frame limit exceeded"));
        }
        self.rasterized_frame_count = next_count;
        self.rasterized_frame_digest.update(sequence.to_le_bytes());
        self.rasterized_frame_digest.update(width.to_le_bytes());
        self.rasterized_frame_digest.update(height.to_le_bytes());
        self.rasterized_frame_digest.update(rgba8);
        self.refresh_analysis();
        if self.policy.retention == HeadlessArtifactRetention::Final {
            self.final_frame = Some((sequence, width, height, rgba8.to_vec()));
            return Ok(());
        }
        if self.policy.retention == HeadlessArtifactRetention::ManifestOnly {
            return Ok(());
        }
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(rgba8, width, height, ExtendedColorType::Rgba8)
            .map_err(|_| io_error("artifact.png.encode"))?;
        self.reserve(bytes.len() as u64)?;
        let relative = format!("frames/frame-{sequence:010}.png");
        atomic_write(&self.root.join(&relative), &bytes)?;
        self.manifest.artifacts.push(ArtifactEntry::Frame {
            relative_path: relative,
            sha256: astra_core::Hash256::from_sha256(&bytes).to_string(),
            byte_size: bytes.len() as u64,
            width,
            height,
            color_space: "rgba8_srgb".into(),
            sequence,
            checkpoint_ids: Vec::new(),
        });
        self.commit_manifest()
    }

    pub(crate) fn record_audio(
        &mut self,
        sequence: u64,
        samples: &[f32],
    ) -> Result<(), PlatformError> {
        self.validate_audio_timeline_samples(samples.len(), Some(samples))?;
        let frames = (samples.len() / 2) as u64;
        let next_audio_frames = self
            .audio_frames
            .checked_add(frames)
            .ok_or_else(|| limit("artifact.audio", "audio frame counter overflowed"))?;
        let duration_ns = audio_duration_ns(frames)?;
        if self.policy.retention == HeadlessArtifactRetention::ManifestOnly {
            self.commit_audio_analysis(next_audio_frames, samples);
            return Ok(());
        }
        let next_artifact_count = self
            .audio_artifact_count
            .checked_add(1)
            .ok_or_else(|| limit("artifact.audio", "audio artifact counter overflowed"))?;
        let bytes = wav_bytes(samples)?;
        let next_total_bytes = self.validate_reserve(bytes.len() as u64)?;
        // `sequence` belongs to an individual audio output and may restart after a
        // snapshot restore recreates platform handles. Artifact identity is session-wide,
        // so use a recorder-owned monotonic ordinal and retain the source sequence only
        // as diagnostic context in the file name.
        let relative = format!(
            "audio/output-{:010}-source-{sequence:010}.wav",
            next_artifact_count
        );
        atomic_write(&self.root.join(&relative), &bytes)?;
        self.audio_artifact_count = next_artifact_count;
        self.total_bytes = next_total_bytes;
        self.commit_audio_analysis(next_audio_frames, samples);
        self.manifest.artifacts.push(ArtifactEntry::Audio {
            relative_path: relative,
            sha256: astra_core::Hash256::from_sha256(&bytes).to_string(),
            byte_size: bytes.len() as u64,
            sample_rate: 48_000,
            channels: 2,
            frame_count: frames,
            duration_ns,
            checkpoint: None,
        });
        self.commit_manifest()
    }

    pub(crate) fn validate_audio_timeline(&self, sample_count: usize) -> Result<(), PlatformError> {
        self.validate_audio_timeline_samples(sample_count, None)
    }

    fn validate_audio_timeline_samples(
        &self,
        sample_count: usize,
        samples: Option<&[f32]>,
    ) -> Result<(), PlatformError> {
        if !sample_count.is_multiple_of(2) {
            return Err(integrity(
                "artifact.wav",
                "stereo samples are not frame aligned",
            ));
        }
        if samples.is_some_and(|samples| samples.iter().any(|sample| !sample.is_finite())) {
            return Err(integrity("artifact.wav", "audio sample is not finite"));
        }
        let frames = (sample_count / 2) as u64;
        let total_frames = self
            .audio_frames
            .checked_add(frames)
            .ok_or_else(|| limit("artifact.audio", "audio frame counter overflowed"))?;
        if total_frames > self.policy.max_audio_frames {
            return Err(limit("artifact.audio", "audio frame limit exceeded"));
        }
        // Multiple outputs may overlap in wall time. `max_audio_frames` bounds their
        // aggregate retained data; `max_duration_ns` bounds each individual artifact.
        let duration_ns = audio_duration_ns(frames)?;
        if duration_ns > self.policy.max_duration_ns {
            return Err(limit("artifact.audio", "audio duration limit exceeded"));
        }
        if self.policy.retention != HeadlessArtifactRetention::ManifestOnly {
            let wav_size = 44_u64
                .checked_add((sample_count as u64).saturating_mul(2))
                .ok_or_else(|| limit("artifact.audio", "audio byte size overflowed"))?;
            if self
                .total_bytes
                .checked_add(wav_size)
                .is_none_or(|bytes| bytes > self.policy.max_total_bytes)
            {
                return Err(limit("artifact.audio", "audio byte limit exceeded"));
            }
        }
        Ok(())
    }

    fn commit_audio_analysis(&mut self, next_audio_frames: u64, samples: &[f32]) {
        self.audio_frames = next_audio_frames;
        for sample in samples {
            let value = f64::from(*sample);
            self.audio_digest.update(sample.to_le_bytes());
            self.audio_square_sum += value * value;
            self.audio_peak = self.audio_peak.max(value.abs());
            self.audio_sample_count = self.audio_sample_count.saturating_add(1);
        }
        self.refresh_analysis();
    }

    pub(crate) fn finish(&mut self) -> Result<String, PlatformError> {
        if let Some((sequence, width, height, rgba8)) = self.final_frame.take() {
            let mut bytes = Vec::new();
            PngEncoder::new(&mut bytes)
                .write_image(&rgba8, width, height, ExtendedColorType::Rgba8)
                .map_err(|_| io_error("artifact.png.encode"))?;
            self.reserve(bytes.len() as u64)?;
            let relative = "frames/final.png".to_string();
            atomic_write(&self.root.join(&relative), &bytes)?;
            self.manifest.artifacts.push(ArtifactEntry::Frame {
                relative_path: relative,
                sha256: astra_core::Hash256::from_sha256(&bytes).to_string(),
                byte_size: bytes.len() as u64,
                width,
                height,
                color_space: "rgba8_srgb".into(),
                sequence,
                checkpoint_ids: Vec::new(),
            });
        }
        self.commit_manifest()?;
        let bytes = fs::read(self.root.join("artifact-manifest.json"))
            .map_err(|_| io_error("artifact.manifest.read"))?;
        Ok(astra_core::Hash256::from_sha256(&bytes).to_string())
    }

    fn reserve(&mut self, bytes: u64) -> Result<(), PlatformError> {
        let next = self.validate_reserve(bytes)?;
        self.total_bytes = next;
        Ok(())
    }

    fn validate_reserve(&self, bytes: u64) -> Result<u64, PlatformError> {
        if (self.manifest.artifacts.len() as u64) >= self.policy.max_artifacts {
            return Err(limit("artifact.commit", "artifact count limit exceeded"));
        }
        let next = self
            .total_bytes
            .checked_add(bytes)
            .ok_or_else(|| limit("artifact.commit", "artifact byte counter overflowed"))?;
        if next > self.policy.max_total_bytes {
            return Err(limit("artifact.commit", "artifact byte limit exceeded"));
        }
        Ok(next)
    }

    fn commit_manifest(&self) -> Result<(), PlatformError> {
        let bytes = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|_| io_error("artifact.manifest.encode"))?;
        atomic_write(&self.root.join("artifact-manifest.json"), &bytes)
    }

    fn refresh_analysis(&mut self) {
        self.manifest.submitted_frame_count = self.submitted_frame_count;
        self.manifest.rasterized_frame_count = self.rasterized_frame_count;
        self.manifest.audio_frame_count = self.audio_frames;
        self.manifest.submitted_scene_stream_hash = format!(
            "sha256:{:x}",
            self.submitted_scene_digest.clone().finalize()
        );
        self.manifest.rasterized_frame_stream_hash = format!(
            "sha256:{:x}",
            self.rasterized_frame_digest.clone().finalize()
        );
        self.manifest.audio_stream_hash =
            format!("sha256:{:x}", self.audio_digest.clone().finalize());
        self.manifest.audio_peak_dbfs = finite_db(self.audio_peak);
        let rms = if self.audio_sample_count == 0 {
            0.0
        } else {
            (self.audio_square_sum / self.audio_sample_count as f64).sqrt()
        };
        self.manifest.audio_rms_dbfs = finite_db(rms);
        self.manifest.silence = self.audio_peak < 1.0 / 32768.0;
        self.manifest.clipping = self.audio_peak >= 1.0;
    }
}

fn finite_db(value: f64) -> Option<f64> {
    if value == 0.0 {
        None
    } else {
        Some(20.0 * value.log10())
    }
}

fn audio_duration_ns(frames: u64) -> Result<u64, PlatformError> {
    frames
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_div(48_000))
        .ok_or_else(|| limit("artifact.audio", "audio duration overflowed"))
}

fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>, PlatformError> {
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer =
            hound::WavWriter::new(&mut cursor, spec).map_err(|_| io_error("artifact.wav.open"))?;
        for sample in samples {
            if !sample.is_finite() {
                return Err(integrity("artifact.wav", "audio sample is not finite"));
            }
            let scaled = (sample.clamp(-1.0, 1.0) * if *sample < 0.0 { 32768.0 } else { 32767.0 })
                .round() as i16;
            writer
                .write_sample(scaled)
                .map_err(|_| io_error("artifact.wav.write"))?;
        }
        writer
            .finalize()
            .map_err(|_| io_error("artifact.wav.finalize"))?;
    }
    Ok(cursor.into_inner())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), PlatformError> {
    let partial = path.with_extension("partial");
    fs::write(&partial, bytes).map_err(|_| io_error("artifact.write"))?;
    fs::rename(partial, path).map_err(|_| io_error("artifact.commit"))
}
fn is_hash(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
fn empty_hash() -> String {
    astra_core::Hash256::from_sha256(&[]).to_string()
}
fn io_error(operation: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::Io, operation, "artifact I/O failed")
}
fn limit(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::QueueOverflow, operation, message)
}
fn integrity(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::IntegrityMismatch, operation, message)
}
