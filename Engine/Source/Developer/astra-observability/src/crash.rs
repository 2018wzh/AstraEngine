use std::{fs, path::Path};

use astra_core::Hash256;
use serde::{Deserialize, Serialize};

use crate::{config::HostRole, ring::RingBuffer, ObservabilityError};

pub const CRASH_BUNDLE_SCHEMA: &str = "astra.crash_bundle.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashArtifactRef {
    pub path: String,
    pub sha256: String,
    pub byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashBundleManifestV1 {
    pub schema: String,
    pub reason_code: String,
    pub session_id: String,
    pub process_role: String,
    pub log_tail: CrashArtifactRef,
    pub ring_record_count: usize,
    pub dropped_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minidump: Option<CrashArtifactRef>,
}

pub(crate) fn write_crash_bundle(
    root: &Path,
    reason_code: &str,
    session_id: &str,
    role: HostRole,
    ring: &RingBuffer,
    dropped_count: u64,
    max_bundles: usize,
) -> Result<CrashBundleManifestV1, ObservabilityError> {
    fs::create_dir_all(root)?;
    prune_old_bundles(root, max_bundles.saturating_sub(1))?;
    let timestamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let directory_name = format!("crash-{session_id}-{timestamp}");
    let directory = root.join(&directory_name);
    fs::create_dir(&directory)?;

    let records = ring.snapshot();
    let mut tail = records.join("\n").into_bytes();
    if !tail.is_empty() {
        tail.push(b'\n');
    }
    let tail_name = "log-tail.jsonl";
    fs::write(directory.join(tail_name), &tail)?;
    let log_tail = CrashArtifactRef {
        path: format!("{directory_name}/{tail_name}"),
        sha256: Hash256::from_sha256(&tail).to_string(),
        byte_size: tail.len() as u64,
    };
    let manifest = CrashBundleManifestV1 {
        schema: CRASH_BUNDLE_SCHEMA.to_string(),
        reason_code: reason_code.to_string(),
        session_id: session_id.to_string(),
        process_role: role.as_str().to_string(),
        log_tail,
        ring_record_count: records.len(),
        dropped_count,
        minidump: None,
    };
    fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn prune_old_bundles(root: &Path, retain: usize) -> Result<(), std::io::Error> {
    let mut bundles = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("crash-"))
        .collect::<Vec<_>>();
    bundles.sort_by_key(|entry| entry.file_name());
    let remove_count = bundles.len().saturating_sub(retain);
    for entry in bundles.into_iter().take(remove_count) {
        fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}
