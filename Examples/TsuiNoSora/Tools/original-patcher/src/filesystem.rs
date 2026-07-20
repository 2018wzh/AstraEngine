use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
};

use filetime::{set_file_times, FileTime};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::error::{PatchError, PatchResult};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileDigest {
    pub relative_path: String,
    pub byte_size: u64,
    pub sha256: String,
}

pub fn canonical_source(path: &Path) -> PatchResult<PathBuf> {
    canonical_existing_directory(path, "source game")
}

pub fn canonical_existing_directory(path: &Path, role: &'static str) -> PatchResult<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_DIRECTORY_UNREADABLE",
            "resolve game directory",
            error,
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_DIRECTORY_UNREADABLE",
            "inspect game directory",
            error,
        )
    })?;
    if !metadata.is_dir() {
        return Err(PatchError::validation(
            "TSUI_PATCH_DIRECTORY_REQUIRED",
            format!("{role} must be a directory"),
        ));
    }
    Ok(canonical)
}

pub fn resolve_new_output(source: &Path, output: &Path) -> PatchResult<PathBuf> {
    if output.exists() {
        return Err(PatchError::validation(
            "TSUI_PATCH_OUTPUT_EXISTS",
            "output must not already exist",
        ));
    }
    let file_name = output.file_name().ok_or_else(|| {
        PatchError::validation(
            "TSUI_PATCH_OUTPUT_NAME_INVALID",
            "output must name a new directory",
        )
    })?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let parent = canonical_existing_directory(parent, "output parent")?;
    let resolved = parent.join(file_name);
    if resolved == source || resolved.starts_with(source) || source.starts_with(&resolved) {
        return Err(PatchError::validation(
            "TSUI_PATCH_OUTPUT_OVERLAPS_SOURCE",
            "output must be separate from the source game directory",
        ));
    }
    Ok(resolved)
}

pub fn join_relative(root: &Path, relative: &str) -> PatchResult<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PatchError::validation(
            "TSUI_PATCH_RELATIVE_PATH_INVALID",
            "patch contract contains an unsafe relative path",
        ));
    }
    Ok(root.join(path))
}

pub fn hash_file(path: &Path) -> PatchResult<String> {
    let file = File::open(path)
        .map_err(|error| PatchError::io("TSUI_PATCH_FILE_READ_FAILED", "open game file", error))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            PatchError::io("TSUI_PATCH_FILE_READ_FAILED", "read game file", error)
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn collect_file_digests(root: &Path, excluded: Option<&str>) -> PatchResult<Vec<FileDigest>> {
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(PatchError::validation(
                "TSUI_PATCH_SYMLINK_REJECTED",
                "game directory must not contain symbolic links or reparse points",
            ));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = safe_relative(root, entry.path())?;
        if excluded == Some(relative.as_str()) {
            continue;
        }
        if !seen.insert(relative.clone()) {
            return Err(PatchError::validation(
                "TSUI_PATCH_DUPLICATE_PATH",
                "game directory contains duplicate normalized paths",
            ));
        }
        let metadata = entry.metadata().map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_FILE_METADATA_FAILED",
                "inspect game file",
                error.into(),
            )
        })?;
        records.push(FileDigest {
            relative_path: relative,
            byte_size: metadata.len(),
            sha256: hash_file(entry.path())?,
        });
    }
    records.sort();
    Ok(records)
}

pub fn copy_tree(source: &Path, destination: &Path) -> PatchResult<()> {
    let mut directories = Vec::new();
    for entry in WalkDir::new(source).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(PatchError::validation(
                "TSUI_PATCH_SYMLINK_REJECTED",
                "game directory must not contain symbolic links or reparse points",
            ));
        }
        let relative = entry.path().strip_prefix(source).map_err(|_| {
            PatchError::validation(
                "TSUI_PATCH_SOURCE_ESCAPE",
                "game directory entry escaped the source root",
            )
        })?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                PatchError::io(
                    "TSUI_PATCH_DIRECTORY_COPY_FAILED",
                    "create output directory",
                    error,
                )
            })?;
            directories.push((entry.path().to_owned(), target));
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), &target).map_err(|error| {
                PatchError::io("TSUI_PATCH_FILE_COPY_FAILED", "copy game file", error)
            })?;
            preserve_file_metadata(entry.path(), &target)?;
        } else {
            return Err(PatchError::validation(
                "TSUI_PATCH_SPECIAL_FILE_REJECTED",
                "game directory contains an unsupported special file",
            ));
        }
    }
    for (source_dir, target_dir) in directories.into_iter().rev() {
        preserve_file_metadata(&source_dir, &target_dir)?;
    }
    Ok(())
}

fn preserve_file_metadata(source: &Path, target: &Path) -> PatchResult<()> {
    let metadata = fs::metadata(source).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_FILE_METADATA_FAILED",
            "read source metadata",
            error,
        )
    })?;
    make_metadata_writable(target)?;
    let modified = FileTime::from_last_modification_time(&metadata);
    let accessed = normalized_access_time(FileTime::from_last_access_time(&metadata), modified);
    set_file_times(target, accessed, modified).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_FILE_METADATA_FAILED",
            "copy file timestamps",
            error,
        )
    })?;
    // Optical media and archival source trees commonly mark every file read-only.
    // Apply that bit only after all metadata writes, otherwise Windows rejects the
    // timestamp update on the newly copied destination file.
    fs::set_permissions(target, metadata.permissions()).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_FILE_METADATA_FAILED",
            "copy file permissions",
            error,
        )
    })
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn make_metadata_writable(target: &Path) -> PatchResult<()> {
    // On Windows this only clears FILE_ATTRIBUTE_READONLY. The Unix branch is
    // compiled separately and never calls Permissions::set_readonly(false),
    // which would otherwise broaden all write bits and trigger this lint.
    let mut permissions = fs::metadata(target)
        .map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_FILE_METADATA_FAILED",
                "read copied file permissions",
                error,
            )
        })?
        .permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(target, permissions).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_FILE_METADATA_FAILED",
                "prepare copied file metadata",
                error,
            )
        })?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn make_metadata_writable(_target: &Path) -> PatchResult<()> {
    Ok(())
}

#[cfg(windows)]
fn normalized_access_time(accessed: FileTime, modified: FileTime) -> FileTime {
    const WINDOWS_FILETIME_EPOCH_UNIX_SECONDS: i64 = -11_644_473_600;
    if accessed.unix_seconds() <= WINDOWS_FILETIME_EPOCH_UNIX_SECONDS {
        // ISO 9660 media commonly exposes an unspecified access time as the
        // Windows FILETIME epoch. SetFileTime rejects that zero FILETIME, so use
        // the source modification time as the deterministic access time.
        modified
    } else {
        accessed
    }
}

#[cfg(not(windows))]
fn normalized_access_time(accessed: FileTime, _modified: FileTime) -> FileTime {
    accessed
}

fn safe_relative(root: &Path, path: &Path) -> PatchResult<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        PatchError::validation(
            "TSUI_PATCH_SOURCE_ESCAPE",
            "game directory entry escaped the source root",
        )
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    PatchError::validation(
                        "TSUI_PATCH_PATH_ENCODING_INVALID",
                        "game directory contains a non-UTF-8 path",
                    )
                })?;
                if value.is_empty() || value.contains(['\r', '\n']) {
                    return Err(PatchError::validation(
                        "TSUI_PATCH_PATH_ENCODING_INVALID",
                        "game directory contains an unsafe path",
                    ));
                }
                parts.push(value);
            }
            _ => {
                return Err(PatchError::validation(
                    "TSUI_PATCH_RELATIVE_PATH_INVALID",
                    "game directory contains an unsafe path",
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn output_must_not_exist_or_overlap_source() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        fs::create_dir(&source).expect("source");
        let source = fs::canonicalize(source).expect("canonical source");
        let overlap = source.join("patched");
        let error = resolve_new_output(&source, &overlap).expect_err("overlap must fail");
        assert_eq!(error.code, "TSUI_PATCH_OUTPUT_OVERLAPS_SOURCE");
    }

    #[test]
    fn digest_paths_are_relative_and_sorted() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("DATA")).expect("data");
        fs::write(temp.path().join("z.bin"), b"z").expect("z");
        fs::write(temp.path().join("DATA").join("a.bin"), b"a").expect("a");
        let records = collect_file_digests(temp.path(), None).expect("records");
        assert_eq!(records[0].relative_path, "DATA/a.bin");
        assert_eq!(records[1].relative_path, "z.bin");
    }

    #[test]
    fn hashes_large_files_with_heap_backed_buffer() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("large.bin");
        fs::write(&path, vec![0x5a; 2 * 1024 * 1024]).expect("large fixture");
        assert_eq!(hash_file(&path).expect("hash").len(), 64);
    }

    #[test]
    fn copy_tree_preserves_read_only_files_after_timestamp_copy() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).expect("source");
        let source_file = source.join("disc.bin");
        fs::write(&source_file, b"disc").expect("fixture");
        let mut permissions = fs::metadata(&source_file).expect("metadata").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source_file, permissions).expect("read-only fixture");

        copy_tree(&source, &destination).expect("copy read-only source tree");

        let copied = destination.join("disc.bin");
        assert_eq!(fs::read(copied.as_path()).expect("copied bytes"), b"disc");
        assert!(
            fs::metadata(copied)
                .expect("copied metadata")
                .permissions()
                .readonly(),
            "source read-only permission must be preserved"
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_unspecified_optical_media_access_time() {
        let unspecified = FileTime::from_unix_time(-11_644_473_600, 0);
        let modified = FileTime::from_unix_time(932_054_400, 0);
        assert_eq!(normalized_access_time(unspecified, modified), modified);
    }
}
