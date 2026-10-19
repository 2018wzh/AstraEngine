use crate::{
    paths::{relative, AuthorizedRoot},
    storage::{scratch, TaskFile},
    PatchError, PatchErrorKind,
};
use astra_byte_source::{ByteRange, DEFAULT_MAX_RANGE_BYTES};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeListing {
    pub id: String,
    pub title: String,
    pub script: PathBuf,
}

/// Read catalog data only. Listing never opens, compiles, or executes a recipe.
pub fn list_recipes(scripts: &Path) -> Result<Vec<RecipeListing>, PatchError> {
    let root = AuthorizedRoot::open(scripts)?;
    let mut file = TaskFile::new(
        root.open_read("catalog.json")?,
        false,
        Arc::new(AtomicBool::new(false)),
    )?;
    let length = file.size()?;
    ByteRange {
        offset: 0,
        len: length,
    }
    .validate(length, DEFAULT_MAX_RANGE_BYTES)
    .map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::Bounds,
            "catalog exceeds bounded file range",
            cause,
        )
    })?;
    let mut bytes = scratch(length as usize)?;
    file.read(0, &mut bytes)?;
    let recipes: Vec<RecipeListing> = serde_json::from_slice(&bytes).map_err(|cause| {
        // Only safe parser coordinates survive: an invalid field may itself
        // contain private local text in a user-authored catalog.
        let detail = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JSON line {}, column {}", cause.line(), cause.column()),
        );
        PatchError::with_source(
            PatchErrorKind::InvalidRequest,
            "invalid recipe catalog",
            detail,
        )
    })?;
    let mut ids = std::collections::BTreeSet::new();
    for recipe in &recipes {
        if recipe.id.trim().is_empty()
            || recipe.id.contains('\0')
            || recipe.title.trim().is_empty()
            || !ids.insert(&recipe.id)
        {
            return Err(PatchError::new(
                PatchErrorKind::InvalidRequest,
                "invalid or duplicate catalog identity",
            ));
        }
        relative(
            recipe.script.to_str().ok_or_else(|| {
                PatchError::new(PatchErrorKind::Path, "catalog script name must be Unicode")
            })?,
            false,
        )?;
    }
    Ok(recipes)
}
