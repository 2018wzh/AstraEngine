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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeListing {
    pub id: String,
    pub title: String,
    pub script: PathBuf,
    /// VNDB work ids (`v<digits>`) the recipe converts.
    pub vn_ids: Vec<String>,
    /// Emulator plugin ids that consume the converted output.
    pub emulators: Vec<String>,
}

fn is_vn_id(value: &str) -> bool {
    value.strip_prefix('v').is_some_and(|digits| {
        (1..=12).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn unique_nonempty(values: &[String], valid: impl Fn(&str) -> bool) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    !values.is_empty()
        && values
            .iter()
            .all(|value| valid(value) && seen.insert(value.as_str()))
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
        if !unique_nonempty(&recipe.vn_ids, is_vn_id)
            || !unique_nonempty(&recipe.emulators, |id| {
                !id.trim().is_empty() && !id.contains('\0')
            })
        {
            return Err(PatchError::new(
                PatchErrorKind::InvalidRequest,
                "catalog recipe needs unique VNDB work ids and emulator plugin ids",
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
