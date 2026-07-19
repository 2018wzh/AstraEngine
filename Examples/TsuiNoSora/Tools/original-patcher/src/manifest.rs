use std::{collections::BTreeSet, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    director::DirectorPatchRecord,
    error::{PatchError, PatchResult},
    filesystem::{join_relative, FileDigest},
    fingerprint::{SourceInspection, EDITION_ID},
    locale_emulator::{self, LocaleEmulatorRecord},
    projectorrays::HelperIdentity,
    window_policy::{self, WindowPolicyRecord},
};

pub const PATCH_MANIFEST_NAME: &str = "patch-manifest.json";
const PATCH_MANIFEST_SCHEMA: &str = "tsuinosora.original_patch_manifest.v1";
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchManifest {
    pub schema: String,
    pub status: String,
    pub patcher_version: String,
    pub edition_id: String,
    pub projectorrays: HelperIdentity,
    pub locale_emulator: LocaleEmulatorRecord,
    pub director_patch: DirectorPatchRecord,
    pub window_policy: WindowPolicyRecord,
    pub source_critical_files: Vec<FileDigest>,
    pub source_files: Vec<FileDigest>,
    pub output_files: Vec<FileDigest>,
}

impl PatchManifest {
    pub fn new(
        patcher_version: &str,
        inspection: SourceInspection,
        projectorrays: HelperIdentity,
        director_patch: DirectorPatchRecord,
        source_files: Vec<FileDigest>,
        output_files: Vec<FileDigest>,
    ) -> Self {
        Self {
            schema: PATCH_MANIFEST_SCHEMA.to_owned(),
            status: "pass".to_owned(),
            patcher_version: patcher_version.to_owned(),
            edition_id: inspection.edition_id,
            projectorrays,
            locale_emulator: locale_emulator::record(),
            director_patch,
            window_policy: window_policy::record(),
            source_critical_files: inspection.critical_files,
            source_files,
            output_files,
        }
    }

    pub fn write(&self, root: &Path) -> PatchResult<()> {
        self.validate_contract()?;
        let path = join_relative(root, PATCH_MANIFEST_NAME)?;
        let payload = serde_json::to_vec_pretty(self)?;
        fs::write(path, payload).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_MANIFEST_WRITE_FAILED",
                "write patch manifest",
                error,
            )
        })
    }

    pub fn read(root: &Path) -> PatchResult<Self> {
        let path = join_relative(root, PATCH_MANIFEST_NAME)?;
        let metadata = fs::metadata(&path).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_MANIFEST_MISSING",
                "inspect patch manifest",
                error,
            )
        })?;
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_SIZE_INVALID",
                "patch manifest is missing or exceeds its byte limit",
            ));
        }
        let payload = fs::read(path).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_MANIFEST_READ_FAILED",
                "read patch manifest",
                error,
            )
        })?;
        serde_json::from_slice(&payload).map_err(|_| {
            PatchError::validation(
                "TSUI_PATCH_MANIFEST_INVALID",
                "patch manifest is not valid JSON",
            )
        })
    }

    pub fn validate_contract(&self) -> PatchResult<()> {
        if self.schema != PATCH_MANIFEST_SCHEMA
            || self.status != "pass"
            || self.edition_id != EDITION_ID
            || self.patcher_version.is_empty()
        {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_CONTRACT_MISMATCH",
                "patch manifest contract is unsupported",
            ));
        }
        if self.projectorrays.id != "ProjectorRays"
            || self.projectorrays.version != "0.2.0"
            || self.projectorrays.sha256
                != "e9814428ee503cf129b6f5cff54524177b7bdd63201a9095d8d19433535c70db"
        {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_HELPER_MISMATCH",
                "patch manifest ProjectorRays identity is unsupported",
            ));
        }
        if self.locale_emulator != locale_emulator::record() {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_LOCALE_MISMATCH",
                "patch manifest Locale Emulator identity is unsupported",
            ));
        }
        validate_file_records(&self.source_files, "source")?;
        validate_file_records(&self.output_files, "output")?;
        validate_file_records(&self.source_critical_files, "critical")?;
        if self.director_patch.relative_path != "DATA/MENU.dxr"
            || self.director_patch.exit_resource_id != 88
            || self.director_patch.debug_resource_id != 381
            || self.director_patch.exit_member_id != 42
            || self.director_patch.debug_member_id != 63
            || self.director_patch.old_script_id != 9
            || self.director_patch.new_script_id != 44
            || self.window_policy != window_policy::record()
        {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_TRANSFORM_MISMATCH",
                "patch manifest transformations do not match the supported patch",
            ));
        }
        Ok(())
    }
}

fn validate_file_records(records: &[FileDigest], role: &'static str) -> PatchResult<()> {
    let mut seen = BTreeSet::new();
    let mut previous = None;
    for record in records {
        let path = &record.relative_path;
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.contains(':')
            || path.contains("../")
            || path.contains(['\r', '\n'])
            || record.sha256.len() != 64
            || !record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_FILE_INVALID",
                format!("patch manifest contains an invalid {role} file record"),
            ));
        }
        if !seen.insert(path) || previous.is_some_and(|value: &String| value > path) {
            return Err(PatchError::validation(
                "TSUI_PATCH_MANIFEST_FILE_DUPLICATE",
                format!("patch manifest {role} files are duplicated or unsorted"),
            ));
        }
        previous = Some(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_absolute_or_unsorted_paths() {
        let invalid = vec![FileDigest {
            relative_path: "C:/private/game.bin".to_owned(),
            byte_size: 1,
            sha256: "0".repeat(64),
        }];
        let error = validate_file_records(&invalid, "fixture").expect_err("absolute path");
        assert_eq!(error.code, "TSUI_PATCH_MANIFEST_FILE_INVALID");
    }
}
