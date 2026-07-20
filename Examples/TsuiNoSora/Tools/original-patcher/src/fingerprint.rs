use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    error::{PatchError, PatchResult},
    filesystem::{hash_file, join_relative, FileDigest},
};

pub const EDITION_ID: &str = "tsuinosora.windows.1999.verified.v1";

const REQUIRED_FILES: &[(&str, u64, &str)] = &[
    (
        "SETUP.exe",
        4_001_991,
        "1d05513c1c3aa2bfb857d7d5c6ba50dcbc15ec8c49e160553e5a8b66b6c5e4d7",
    ),
    (
        "READY.dxr",
        164_373,
        "25c92bf1a41365051f7500249c47b779367fcd9efd27b32fb9299732b85ba8f7",
    ),
    (
        "DATA/MENU.dxr",
        3_770_400,
        "e060066456a7b239ae5ea5362ed6c338130f6d908af759421b23ff70644457f3",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInspection {
    pub schema: String,
    pub status: String,
    pub edition_id: String,
    pub critical_files: Vec<FileDigest>,
}

pub fn inspect_source(root: &Path) -> PatchResult<SourceInspection> {
    let mut critical_files = Vec::with_capacity(REQUIRED_FILES.len());
    for (relative_path, expected_size, expected_sha256) in REQUIRED_FILES {
        let path = join_relative(root, relative_path)?;
        let metadata = path.metadata().map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_REQUIRED_FILE_MISSING",
                "read required game file",
                error,
            )
        })?;
        if !metadata.is_file() || metadata.len() != *expected_size {
            return Err(PatchError::validation(
                "TSUI_PATCH_EDITION_SIZE_MISMATCH",
                format!("{relative_path} does not match the supported 1999 edition"),
            ));
        }
        let sha256 = hash_file(&path)?;
        if sha256 != *expected_sha256 {
            return Err(PatchError::validation(
                "TSUI_PATCH_EDITION_HASH_MISMATCH",
                format!("{relative_path} does not match the supported 1999 edition"),
            ));
        }
        critical_files.push(FileDigest {
            relative_path: (*relative_path).to_owned(),
            byte_size: metadata.len(),
            sha256,
        });
    }
    critical_files.sort();
    Ok(SourceInspection {
        schema: "tsuinosora.original_patch_inspection.v1".to_owned(),
        status: "pass".to_owned(),
        edition_id: EDITION_ID.to_owned(),
        critical_files,
    })
}
