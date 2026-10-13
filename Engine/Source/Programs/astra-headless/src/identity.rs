use std::{fs, path::Path};

use super::*;
use astra_headless_protocol::{ArtifactManifest, RendererExecutionIdentity};
use astra_platform::HeadlessHostProfile;
use sha2::{Digest, Sha256};

pub(crate) fn read_profile(
    path: &Path,
    identity_hash: &str,
) -> Result<HeadlessHostProfile, String> {
    let profile: HeadlessHostProfile = serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("ASTRA_HEADLESS_PROFILE_READ_FAILED: {e}"))?,
    )
    .map_err(|e| format!("ASTRA_HEADLESS_PROFILE_INVALID: {e}"))?;
    astra_platform::validate_headless_host_profile(&profile).map_err(|e| e.to_string())?;
    if profile.build_fingerprint != identity_hash {
        return Err("ASTRA_HEADLESS_BUILD_IDENTITY_MISMATCH".into());
    }
    Ok(profile)
}
pub(crate) fn provider_hash(profile: &HeadlessHostProfile) -> Result<String, String> {
    let bytes = serde_json::to_vec(&profile.providers).map_err(|e| e.to_string())?;
    Ok(astra_core::Hash256::from_sha256(&bytes).to_string())
}

pub(crate) fn render_policy_name(profile: &HeadlessHostProfile) -> &'static str {
    match profile.render_policy {
        astra_platform::HeadlessRenderPolicy::All => "all",
        astra_platform::HeadlessRenderPolicy::Checkpoints => "checkpoints",
    }
}

pub(crate) fn blocked_renderer_identity(
    profile: &HeadlessHostProfile,
) -> RendererExecutionIdentity {
    let gpu = profile.providers.renderer == "wgpu_offscreen";
    RendererExecutionIdentity {
        provider: profile.providers.renderer.clone(),
        backend: if gpu { "unavailable" } else { "cpu" }.into(),
        device_type: if gpu { "unavailable" } else { "cpu" }.into(),
        vendor_id: 0,
        device_id: 0,
        adapter_name_hash: empty_hash(),
        driver_identity_hash: empty_hash(),
    }
}

pub(crate) fn renderer_identity_hash(identity: &RendererExecutionIdentity) -> String {
    serde_json::to_vec(identity)
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
        .unwrap_or_else(|_| empty_hash())
}
pub(crate) fn hash_file(path: &Path) -> Result<String, String> {
    Ok(astra_core::Hash256::from_sha256_file(path)
        .map_err(|e| e.to_string())?
        .to_string())
}
pub(crate) fn empty_hash() -> String {
    astra_core::Hash256::from_sha256(&[]).to_string()
}
pub(crate) fn is_hash(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(crate) fn ensure_blocked_manifest(
    profile_path: &Path,
    input_path: &Path,
    manifest_path: &Path,
) -> Result<(), String> {
    let profile: HeadlessHostProfile = serde_json::from_slice(
        &fs::read(profile_path)
            .map_err(|error| format!("ASTRA_HEADLESS_BLOCKED_PROFILE_READ_FAILED: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_HEADLESS_BLOCKED_PROFILE_INVALID: {error}"))?;
    let input_hash = fs::read(input_path)
        .map(|bytes| canonical_or_raw_input_hash(&bytes))
        .unwrap_or_else(|_| empty_hash());
    let provider_identity_hash = provider_hash(&profile)?;
    let renderer_identity = blocked_renderer_identity(&profile);
    let render_policy = render_policy_name(&profile).to_string();
    write_atomic_json(
        manifest_path,
        &ArtifactManifest {
            schema: astra_headless_protocol::HEADLESS_ARTIFACT_MANIFEST_SCHEMA.into(),
            run_id: profile.artifacts.namespace,
            build_fingerprint: profile.build_fingerprint,
            package_hash: profile.package_hash,
            input_sequence_hash: input_hash,
            provider_identity_hash,
            renderer_identity_hash: renderer_identity_hash(&renderer_identity),
            renderer_identity,
            render_policy,
            submitted_frame_count: 0,
            rasterized_frame_count: 0,
            audio_frame_count: 0,
            audio_peak_dbfs: None,
            audio_rms_dbfs: None,
            silence: true,
            clipping: false,
            artifacts: Vec::new(),
        },
    )
}

pub(crate) fn write_atomic_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let partial = path.with_extension("partial");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(partial, path).map_err(|e| e.to_string())
}
