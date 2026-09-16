use crate::CoreError;
use astra_core::Hash256;
use serde::de::DeserializeOwned;
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

/// Read a private typed profile without exposing input bytes or paths in errors.
pub fn read_game_profile<T: DeserializeOwned>(
    game_root: &Path,
    profile_path: &Path,
    max_bytes: u64,
) -> Result<(T, Hash256, PathBuf), CoreError> {
    let fail = |code| CoreError::invalid(code, "private game profile could not be read");
    let limit = max_bytes
        .checked_add(1)
        .filter(|_| max_bytes > 0)
        .ok_or_else(|| fail("ASTRA_EMU_PROFILE_BOUND"))?;
    let root = game_root
        .canonicalize()
        .map_err(|_| fail("ASTRA_EMU_PROFILE_ROOT"))?;
    let path = if profile_path.is_absolute() {
        profile_path.to_path_buf()
    } else {
        root.join(profile_path)
    };
    let path = path
        .canonicalize()
        .map_err(|_| fail("ASTRA_EMU_PROFILE_OPEN"))?;
    if !path.starts_with(&root) {
        return Err(fail("ASTRA_EMU_PROFILE_PATH"));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| fail("ASTRA_EMU_PROFILE_OPEN"))?
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| fail("ASTRA_EMU_PROFILE_READ"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(fail("ASTRA_EMU_PROFILE_BOUND"));
    }
    let profile = serde_json::from_slice(&bytes).map_err(|_| fail("ASTRA_EMU_PROFILE_FORMAT"))?;
    Ok((profile, Hash256::from_sha256(&bytes), root))
}

/// Resolve a profile's explicit resource path within its canonical game root.
pub fn resolve_game_file(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let fail = || CoreError::invalid("ASTRA_EMU_RESOURCE_PATH", "game resource path is invalid");
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains(['\\', ':', '\0'])
        || path.is_absolute()
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(fail());
    }
    let root = root.canonicalize().map_err(|_| fail())?;
    let path = root.join(path).canonicalize().map_err(|_| fail())?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(fail());
    }
    Ok(path)
}
