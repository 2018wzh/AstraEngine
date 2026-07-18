use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use astra_core::Hash256;
use astra_emu_family_core::{LegacyCoreError, LegacyMountedVfs};
use globset::{Glob, GlobMatcher};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{enforce_private_directory_permissions, enforce_private_file_permissions};

const EXTRACT_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct ExtractSelection {
    pub prefix: Option<String>,
    pub glob: Option<String>,
    pub entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtractReport {
    pub schema: String,
    pub family_id: String,
    pub entry_count: u64,
    pub byte_count: u64,
    pub aggregate_hash: Hash256,
}

pub fn extract_vfs(
    vfs: &dyn LegacyMountedVfs,
    output: &Path,
    selection: &ExtractSelection,
    cancelled: &AtomicBool,
) -> Result<ExtractReport, LegacyCoreError> {
    if [
        selection.prefix.is_some(),
        selection.glob.is_some(),
        selection.entry.is_some(),
    ]
    .into_iter()
    .filter(|value| *value)
    .count()
        > 1
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_SELECTOR",
            "extract selectors are mutually exclusive",
        ));
    }
    if output.exists() {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_EXISTS",
            "extract destination already exists",
        ));
    }
    let parent = output.parent().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_VFS_EXTRACT_DESTINATION",
            "extract destination has no parent",
        )
    })?;
    if !parent.is_dir() {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_DESTINATION",
            "extract destination parent is not a directory",
        ));
    }
    let matcher = selection
        .glob
        .as_deref()
        .map(Glob::new)
        .transpose()
        .map_err(|_| invalid("ASTRA_EMU_VFS_EXTRACT_GLOB", "extract glob is invalid"))?
        .map(|glob| glob.compile_matcher());
    let manifest = vfs.manifest();
    let mut selected = Vec::new();
    let mut folded = BTreeSet::new();
    let mut total = 0u64;
    for entry in &manifest.entries {
        let relative = entry.uri.strip_prefix(&manifest.prefix).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_EXTRACT_URI",
                "manifest entry is outside its prefix",
            )
        })?;
        if !matches_selection(relative, selection, matcher.as_ref()) {
            continue;
        }
        let relative_path = normalized_relative(relative)?;
        let collision_key = relative.replace('\\', "/").to_lowercase();
        if !folded.insert(collision_key) {
            return Err(invalid(
                "ASTRA_EMU_VFS_EXTRACT_CASE_CONFLICT",
                "selected entries conflict under case folding",
            ));
        }
        total = total.checked_add(entry.decoded_size).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_EXTRACT_SIZE",
                "extract byte count overflowed",
            )
        })?;
        selected.push((entry, relative_path));
    }
    if selected.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_EMPTY",
            "extract selector matched no entries",
        ));
    }
    let available = fs2::available_space(parent).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_EXTRACT_CAPACITY",
            "destination capacity could not be inspected",
        )
    })?;
    if available < total {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_CAPACITY",
            "destination has insufficient free space",
        ));
    }
    let staging = parent.join(format!(
        ".astra-vfs-extract-{}-{}",
        std::process::id(),
        Hash256::from_sha256(output.as_os_str().to_string_lossy().as_bytes()).to_hex()
    ));
    if staging.exists() {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_STAGING",
            "extract staging destination already exists",
        ));
    }
    fs::create_dir(&staging).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_EXTRACT_STAGING",
            "extract staging directory could not be created",
        )
    })?;
    if enforce_private_directory_permissions(&staging).is_err() {
        if fs::remove_dir(&staging).is_err() {
            return Err(invalid(
                "ASTRA_EMU_VFS_EXTRACT_PERMISSION_CLEANUP",
                "extract staging permissions failed and cleanup also failed",
            ));
        }
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_PERMISSION",
            "extract staging permissions could not be restricted",
        ));
    }
    let result = (|| {
        let mut aggregate = Sha256::new();
        for (entry, relative) in &selected {
            if cancelled.load(Ordering::Relaxed) {
                return Err(invalid(
                    "ASTRA_EMU_VFS_EXTRACT_CANCELLED",
                    "extract operation was cancelled",
                ));
            }
            let destination = staging.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_VFS_EXTRACT_DIRECTORY",
                        "extract directory could not be created",
                    )
                })?;
            }
            let temporary = destination.with_extension(format!(
                "{}astra-tmp",
                destination
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| format!("{value}."))
                    .unwrap_or_default()
            ));
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_EXTRACT_FILE",
                    "extract temporary file could not be created",
                )
            })?;
            enforce_private_file_permissions(&temporary).map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_EXTRACT_PERMISSION",
                    "extract file permissions could not be restricted",
                )
            })?;
            let mut offset = 0u64;
            while offset < entry.decoded_size {
                let length = (entry.decoded_size - offset).min(EXTRACT_CHUNK_BYTES);
                let read = vfs.read_range(&entry.uri, offset, length)?;
                if read.bytes.len() as u64 != length {
                    return Err(invalid(
                        "ASTRA_EMU_VFS_EXTRACT_SHORT_READ",
                        "VFS returned a short extract range",
                    ));
                }
                file.write_all(&read.bytes).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_VFS_EXTRACT_WRITE",
                        "extract temporary file write failed",
                    )
                })?;
                aggregate.update(&read.bytes);
                offset += length;
            }
            file.sync_all().map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_EXTRACT_SYNC",
                    "extract temporary file sync failed",
                )
            })?;
            drop(file);
            fs::rename(&temporary, &destination).map_err(|_| {
                invalid("ASTRA_EMU_VFS_EXTRACT_COMMIT", "extract file commit failed")
            })?;
        }
        fs::rename(&staging, output).map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_EXTRACT_COMMIT",
                "extract directory commit failed",
            )
        })?;
        Ok(ExtractReport {
            schema: "astra.emu.vfs.extract.v1".into(),
            family_id: manifest.family_id.clone(),
            entry_count: selected.len() as u64,
            byte_count: total,
            aggregate_hash: Hash256::from_bytes(aggregate.finalize().into()),
        })
    })();
    if result.is_err() && staging.exists() && fs::remove_dir_all(&staging).is_err() {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_CLEANUP",
            "extract failed and staging cleanup also failed",
        ));
    }
    result
}

fn matches_selection(
    relative: &str,
    selection: &ExtractSelection,
    matcher: Option<&GlobMatcher>,
) -> bool {
    if let Some(prefix) = &selection.prefix {
        relative == prefix || relative.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
    } else if let Some(entry) = &selection.entry {
        relative == entry
    } else if let Some(matcher) = matcher {
        matcher.is_match(relative)
    } else {
        true
    }
}

fn normalized_relative(value: &str) -> Result<PathBuf, LegacyCoreError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_EXTRACT_PATH",
            "extract entry path is unsafe",
        ));
    }
    Ok(path.to_path_buf())
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use crate::test_support::MemoryVfs;

    use super::{extract_vfs, ExtractSelection};

    #[test]
    fn selection_is_atomic_and_existing_destination_blocks() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        let vfs = MemoryVfs::new(&[
            ("test:/scr/a.sc", b"script", "script"),
            ("test:/sys/a.png", b"image", "image"),
        ]);
        let report = extract_vfs(
            &vfs,
            &output,
            &ExtractSelection {
                prefix: Some("scr".into()),
                ..ExtractSelection::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(report.entry_count, 1);
        assert_eq!(std::fs::read(output.join("scr/a.sc")).unwrap(), b"script");
        assert!(!output.join("sys/a.png").exists());
        assert_eq!(
            extract_vfs(
                &vfs,
                &output,
                &ExtractSelection::default(),
                &AtomicBool::new(false),
            )
            .unwrap_err()
            .code(),
            "ASTRA_EMU_VFS_EXTRACT_EXISTS"
        );
    }

    #[test]
    fn selector_conflict_and_cancellation_leave_no_output() {
        let temp = tempfile::tempdir().unwrap();
        let vfs = MemoryVfs::new(&[("test:/scr/a.sc", b"script", "script")]);
        let conflict = ExtractSelection {
            prefix: Some("scr".into()),
            entry: Some("scr/a.sc".into()),
            glob: None,
        };
        assert_eq!(
            extract_vfs(
                &vfs,
                &temp.path().join("conflict"),
                &conflict,
                &AtomicBool::new(false),
            )
            .unwrap_err()
            .code(),
            "ASTRA_EMU_VFS_EXTRACT_SELECTOR"
        );
        let output = temp.path().join("cancelled");
        assert_eq!(
            extract_vfs(
                &vfs,
                &output,
                &ExtractSelection::default(),
                &AtomicBool::new(true),
            )
            .unwrap_err()
            .code(),
            "ASTRA_EMU_VFS_EXTRACT_CANCELLED"
        );
        assert!(!output.exists());
    }
}
