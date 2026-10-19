//! Explicit trusted Luau file conversion, separate from runtime text extensions.
mod buffer;
mod catalog;
mod codecs;
mod data;
mod error;
mod paths;
mod receipt;
mod storage;
mod transaction;
mod vm;
pub use catalog::{list_recipes, RecipeListing};
pub use error::{PatchError, PatchErrorKind};
pub use receipt::read_output_profile;

pub(crate) const STAGING_PREFIX: &str = ".astra-patch-staging-";

/// The private transaction namespace is never a discoverable game/output root.
pub fn is_staging_path(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .and_then(|name| name.get(..STAGING_PREFIX.len()))
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(STAGING_PREFIX))
    })
}

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub struct PatchRequest {
    pub script: PathBuf,
    pub source: PathBuf,
    pub patch: PathBuf,
    pub output: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputProfile {
    pub plugin_id: String,
    pub configuration: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct PatchProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
}

pub struct PatchResult {
    pub output: PathBuf,
    pub profile: OutputProfile,
}

pub fn apply(
    request: PatchRequest,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: impl Fn(PatchProgress) + Send + Sync,
) -> Result<PatchResult, PatchError> {
    error::check_cancelled(&cancelled)?;
    let transaction = transaction::Transaction::new(&request)?;
    progress(PatchProgress {
        phase: "applying recipe".into(),
        completed: 0,
        total: None,
    });
    let execution = vm::execute(&request, &transaction, cancelled.clone(), &progress)?;
    let root = paths::AuthorizedRoot::from_dir(transaction.open_staging()?);
    receipt::write(&root, &execution, cancelled.clone(), &progress)?;
    drop(root);
    let output = transaction.publish(&cancelled)?;
    progress(PatchProgress {
        phase: "complete".into(),
        completed: 1,
        total: Some(1),
    });
    Ok(PatchResult {
        output,
        profile: execution.profile,
    })
}
