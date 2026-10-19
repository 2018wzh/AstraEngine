use crate::{
    error::check_cancelled,
    paths::AuthorizedRoot,
    storage::TaskFile,
    vm::{Execution, InputRecord, ScriptRecord},
    OutputProfile, PatchError, PatchErrorKind, PatchProgress,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    sync::{atomic::AtomicBool, Arc},
};

pub(crate) const RECEIPT_FILE: &str = "astra-patch-result.json";

#[derive(Serialize)]
struct Recipe<'a> {
    id: &'a str,
    version: u64,
}

#[derive(Serialize)]
struct OutputRecord {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize)]
struct Receipt<'a> {
    schema: &'static str,
    recipe: Recipe<'a>,
    scripts: &'a [ScriptRecord],
    inputs: &'a [InputRecord],
    outputs: Vec<OutputRecord>,
    profile: &'a OutputProfile,
}

struct CancelledWriter<'a> {
    file: File,
    cancelled: &'a AtomicBool,
}

impl Write for CancelledWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::other("conversion cancelled"));
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub(crate) fn write(
    root: &AuthorizedRoot,
    execution: &Execution,
    cancelled: Arc<AtomicBool>,
    progress: &(impl Fn(PatchProgress) + Send + Sync),
) -> Result<(), PatchError> {
    let mut outputs = Vec::new();
    let mut pending = vec![String::new()];
    while let Some(directory) = pending.pop() {
        check_cancelled(&cancelled)?;
        for entry in root.list(&directory)? {
            let path = if directory.is_empty() {
                entry.name
            } else {
                format!("{directory}/{}", entry.name)
            };
            match entry.kind.as_str() {
                "directory" => pending.push(path),
                "file" => {
                    if path == RECEIPT_FILE {
                        return Err(PatchError::new(
                            PatchErrorKind::OutputExists,
                            "receipt filename is reserved",
                        ));
                    }
                    let mut file = TaskFile::new(root.open_read(&path)?, false, cancelled.clone())?;
                    let size = file.size()?;
                    let sha256 = file.sha256()?;
                    outputs.push(OutputRecord { path, size, sha256 });
                    progress(PatchProgress {
                        phase: "verifying output".into(),
                        completed: outputs.len() as u64,
                        total: None,
                    });
                }
                _ => {
                    return Err(PatchError::new(
                        PatchErrorKind::Path,
                        "output contains a link or nonregular entry",
                    ))
                }
            }
        }
    }
    outputs.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let receipt = Receipt {
        schema: "astra.emu.file_patch.v1",
        recipe: Recipe {
            id: &execution.id,
            version: execution.version,
        },
        scripts: &execution.scripts,
        inputs: &execution.inputs,
        outputs,
        profile: &execution.profile,
    };
    check_cancelled(&cancelled)?;
    let mut writer = CancelledWriter {
        file: root.create(RECEIPT_FILE)?,
        cancelled: &cancelled,
    };
    let result = serde_json::to_writer_pretty(&mut writer, &receipt);
    check_cancelled(&cancelled)?;
    result.map_err(|cause| {
        PatchError::with_source(PatchErrorKind::Io, "writing output receipt failed", cause)
    })?;
    writer.file.sync_all()?;
    check_cancelled(&cancelled)
}

struct CancelledReader<'a> {
    file: File,
    cancelled: &'a AtomicBool,
}

impl Read for CancelledReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::other("receipt read cancelled"));
        }
        let length = bytes.len().min(astra_byte_source::AUDIT_CHUNK_BYTES);
        self.file.read(&mut bytes[..length])
    }
}

/// Read import metadata only; unknown receipt data is streamed past without
/// retaining resource manifests or loading any of the referenced scripts.
pub fn read_output_profile(
    output: &std::path::Path,
    cancelled: &AtomicBool,
) -> Result<Option<OutputProfile>, PatchError> {
    check_cancelled(cancelled)?;
    if crate::is_staging_path(output) {
        return Err(PatchError::new(
            PatchErrorKind::InvalidRequest,
            "staging is not a published output",
        ));
    }
    let root = AuthorizedRoot::open(output)?;
    if !root.exists(RECEIPT_FILE)? {
        return Ok(None);
    }
    #[derive(Deserialize)]
    struct ImportMetadata {
        schema: String,
        profile: OutputProfile,
    }
    let reader = BufReader::new(CancelledReader {
        file: root.open_read(RECEIPT_FILE)?,
        cancelled,
    });
    let metadata = serde_json::from_reader::<_, ImportMetadata>(reader);
    check_cancelled(cancelled)?;
    let metadata = metadata.map_err(|cause| {
        let detail = io::Error::new(
            io::ErrorKind::InvalidData,
            format!("JSON line {}, column {}", cause.line(), cause.column()),
        );
        PatchError::with_source(
            PatchErrorKind::InvalidRequest,
            "invalid output receipt",
            detail,
        )
    })?;
    if metadata.schema != "astra.emu.file_patch.v1" {
        return Err(PatchError::new(
            PatchErrorKind::UnsupportedInput,
            "unsupported output receipt schema",
        ));
    }
    Ok(Some(metadata.profile))
}
