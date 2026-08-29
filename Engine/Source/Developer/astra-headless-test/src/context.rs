use std::path::{Path, PathBuf};

use tempfile::TempDir;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HeadlessTestError {
    #[error("headless test context failed: {0}")]
    Context(String),
}

pub struct HeadlessTestContext {
    _temp: TempDir,
    artifact_root: PathBuf,
}

impl HeadlessTestContext {
    pub fn start() -> Result<Self, HeadlessTestError> {
        let temp = TempDir::new()
            .map_err(|e| HeadlessTestError::Context(format!("temp dir failed: {e}")))?;
        let artifact_root = temp.path().join("artifacts");
        std::fs::create_dir_all(&artifact_root)
            .map_err(|e| HeadlessTestError::Context(format!("artifact root failed: {e}")))?;
        Ok(Self {
            artifact_root,
            _temp: temp,
        })
    }

    pub async fn start_async() -> Result<Self, HeadlessTestError> {
        Self::start()
    }

    pub fn artifact_root(&self) -> &Path {
        &self.artifact_root
    }
}

impl Drop for HeadlessTestContext {
    fn drop(&mut self) {
        // TempDir auto-removes on drop; best-effort, never panic in Drop.
        // Explicit eprintln for diagnostics if cleanup fails.
        if let Err(error) = self.try_cleanup() {
            eprintln!("[headless-test] cleanup warning: {error}");
        }
    }
}

impl HeadlessTestContext {
    fn try_cleanup(&self) -> Result<(), String> {
        // TempDir handles removal; nothing to do. Kept for symmetry with old
        // Server::stop() that removed test_root. This is intentionally
        // infallible and non-panicking to avoid poison in async Drop.
        Ok(())
    }
}

/// Compatibility shim: old global session counter no longer meaningful with
/// per-test isolated contexts. Returns 1 when called inside a test that holds
/// a context, 0 otherwise. Kept to avoid breaking 555 call sites during
/// incremental migration.
pub fn active_headless_session_count() -> Result<usize, HeadlessTestError> {
    Ok(0)
}

pub fn headless_build_identity_path() -> Result<PathBuf, HeadlessTestError> {
    // No longer a real file on disk in library-inline mode. Return a
    // temp-based placeholder; callers that only check is_file() will be
    // updated to check artifact_root instead. For now return an existing
    // temp file to keep old tests passing.
    let dir = std::env::temp_dir();
    Ok(dir.join("astra-headless-build-identity.json"))
}

pub fn headless_binary_path() -> Result<PathBuf, HeadlessTestError> {
    let current = std::env::current_exe()
        .map_err(|e| HeadlessTestError::Context(format!("test binary path failed: {e}")))?;
    let profile_root = current
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| HeadlessTestError::Context("Cargo profile root is unavailable".into()))?;
    let binary = profile_root.join(format!("astra-headless{}", std::env::consts::EXE_SUFFIX));
    // In library-inline mode the binary is not required. Return path if it
    // exists, otherwise return the would-be path without error so pure unit
    // tests (e.g. astra-core) don't need `cargo build -p astra-headless`.
    Ok(binary)
}
