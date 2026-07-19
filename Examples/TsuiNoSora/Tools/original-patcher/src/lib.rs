mod director;
mod error;
mod filesystem;
mod fingerprint;
mod locale_emulator;
mod manifest;
mod projectorrays;
mod window_launcher;
mod window_policy;

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use error::{ErrorClass, PatchError, PatchResult};
pub use fingerprint::{inspect_source, SourceInspection};
pub use manifest::{PatchManifest, PATCH_MANIFEST_NAME};
use projectorrays::{decompile_menu, resolve_helper, HelperIdentity, ReleaseHelperPolicy};
use tempfile::Builder;

const PATCHER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PATCHED_MENU_PATH: &str = "DATA/MENU.dxr";
pub const WINDOW_LAUNCHER_NAME: &str = "TsuiNoSoraWindowed.exe";
pub const WINDOWED_PROJECTOR_NAME: &str = "TsuiNoSoraProjector.exe";

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub source: PathBuf,
    pub output: PathBuf,
    pub projectorrays: Option<PathBuf>,
    pub locale_emulator: Option<PathBuf>,
}

pub fn apply(options: &ApplyOptions) -> PatchResult<PatchManifest> {
    apply_with_policy(options, &ReleaseHelperPolicy)
}

fn apply_with_policy(
    options: &ApplyOptions,
    helper_policy: &dyn projectorrays::HelperPolicy,
) -> PatchResult<PatchManifest> {
    let source = filesystem::canonical_source(&options.source)?;
    let output = filesystem::resolve_new_output(&source, &options.output)?;
    let inspection = inspect_source(&source)?;
    let source_files = filesystem::collect_file_digests(&source, None)?;
    let helper_path = resolve_helper(options.projectorrays.as_deref())?;
    let helper = helper_policy.validate(&helper_path)?;

    let output_parent = output.parent().ok_or_else(|| {
        PatchError::validation(
            "TSUI_PATCH_OUTPUT_PARENT_MISSING",
            "output must have an existing parent directory",
        )
    })?;
    let staging = Builder::new()
        .prefix(".tsuinosora-patch-")
        .tempdir_in(output_parent)
        .map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_STAGE_CREATE_FAILED",
                "create staging directory",
                error,
            )
        })?;
    filesystem::copy_tree(&source, staging.path())?;

    let staged_menu = filesystem::join_relative(staging.path(), PATCHED_MENU_PATH)?;
    fs::remove_file(&staged_menu).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_STAGED_MENU_REMOVE_FAILED",
            "replace staged menu movie",
            error,
        )
    })?;
    let source_menu = filesystem::join_relative(&source, PATCHED_MENU_PATH)?;
    decompile_menu(&helper_path, &source_menu, &staged_menu)?;
    let director_patch = director::patch_exit_to_debug(&staged_menu)?;
    window_policy::write(staging.path())?;
    locale_emulator::install(options.locale_emulator.as_deref(), staging.path())?;
    window_launcher::install(staging.path())?;

    let output_files = filesystem::collect_file_digests(staging.path(), Some(PATCH_MANIFEST_NAME))?;
    let manifest = PatchManifest::new(
        PATCHER_VERSION,
        inspection,
        HelperIdentity::from_validated(helper),
        director_patch,
        source_files,
        output_files,
    );
    manifest.write(staging.path())?;
    verify_root(staging.path())?;

    let staging_path = staging.keep();
    fs::rename(&staging_path, &output).map_err(|error| {
        let _ = fs::remove_dir_all(&staging_path);
        PatchError::io(
            "TSUI_PATCH_OUTPUT_COMMIT_FAILED",
            "commit verified output directory",
            error,
        )
    })?;
    Ok(manifest)
}

pub fn verify(output: &Path) -> PatchResult<PatchManifest> {
    let root = filesystem::canonical_existing_directory(output, "patched output")?;
    verify_root(&root)
}

fn verify_root(root: &Path) -> PatchResult<PatchManifest> {
    let manifest = PatchManifest::read(root)?;
    manifest.validate_contract()?;
    let actual_files = filesystem::collect_file_digests(root, Some(PATCH_MANIFEST_NAME))?;
    if actual_files != manifest.output_files {
        return Err(PatchError::validation(
            "TSUI_PATCH_OUTPUT_FILESET_MISMATCH",
            "patched output files do not match patch-manifest.json",
        ));
    }
    let menu = filesystem::join_relative(root, PATCHED_MENU_PATH)?;
    director::verify_exit_to_debug(&menu)?;
    window_policy::verify(root)?;
    locale_emulator::verify_installed(root)?;
    window_launcher::verify_installed(root)?;
    Ok(manifest)
}

pub fn launch_windowed(game_root: &Path) -> PatchResult<()> {
    window_launcher::launch(game_root)
}

#[cfg(test)]
mod tests;
