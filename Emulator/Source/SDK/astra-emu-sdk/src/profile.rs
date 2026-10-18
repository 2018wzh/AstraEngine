use crate::CoreError;
use astra_core::Hash256;
use serde::de::DeserializeOwned;
use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
};

/// Read a private typed profile confined to its game directory.
pub fn read_game_profile<T: DeserializeOwned>(
    game_root: &Path,
    profile_path: &Path,
    max_bytes: u64,
) -> Result<(T, Hash256, PathBuf), CoreError> {
    let (bytes, hash, root) = read_profile_bytes(game_root, profile_path, max_bytes, true)?;
    let profile = serde_json::from_slice(&bytes).map_err(|_| {
        CoreError::invalid("ASTRA_EMU_PROFILE_FORMAT", "profile is not valid JSON for this core; select the matching profile format or import it first")
    })?;
    Ok((profile, hash, root))
}

/// Read an explicitly configured private source. Absolute paths may point to a
/// private configuration directory; relative paths remain confined to the game.
/// Resource resolution is always relative to the returned canonical game root.
pub fn read_configured_profile_bytes(
    game_root: &Path,
    profile_path: &Path,
    max_bytes: u64,
) -> Result<(Vec<u8>, Hash256, PathBuf), CoreError> {
    read_profile_bytes(game_root, profile_path, max_bytes, false)
}

fn read_profile_bytes(
    game_root: &Path,
    profile_path: &Path,
    max_bytes: u64,
    confined: bool,
) -> Result<(Vec<u8>, Hash256, PathBuf), CoreError> {
    let limit = max_bytes.checked_add(1).filter(|_| max_bytes > 0).ok_or_else(|| {
        CoreError::invalid("ASTRA_EMU_PROFILE_BOUND", "profile size limit is invalid")
    })?;
    let root = game_root.canonicalize().map_err(|_| {
        CoreError::invalid("ASTRA_EMU_PROFILE_ROOT", "game directory is unavailable; select an existing readable game directory")
    })?;
    if profile_path.as_os_str().is_empty()
        || (!profile_path.is_absolute()
            && (profile_path.components().any(|part| !matches!(part, Component::Normal(_)))
                || profile_path.to_str().is_none_or(|text| {
                    text.contains(['\\', ':', '\0'])
                        || text.split('/').any(|part| part.is_empty() || part == "." || part == "..")
                })))
    {
        return Err(CoreError::invalid("ASTRA_EMU_PROFILE_PATH", "profile path must be absolute or a safe path relative to the game directory"));
    }
    let path = if profile_path.is_absolute() { profile_path.to_path_buf() } else { root.join(profile_path) };
    let path = path.canonicalize().map_err(profile_io_error)?;
    if (confined || !profile_path.is_absolute()) && !path.starts_with(&root) {
        return Err(CoreError::invalid("ASTRA_EMU_PROFILE_PATH", "relative profile paths must stay inside the game directory; select an absolute private profile path explicitly"));
    }
    if !std::fs::metadata(&path).map_err(profile_io_error)?.is_file() {
        return Err(CoreError::invalid("ASTRA_EMU_PROFILE_FILE", "profile selection is not a regular file; select the private profile file"));
    }
    let mut file = File::open(path).map_err(profile_io_error)?;
    let mut bytes = Vec::new();
    file.by_ref().take(limit).read_to_end(&mut bytes).map_err(|_| {
        CoreError::invalid("ASTRA_EMU_PROFILE_READ", "profile read failed; check file access and storage availability")
    })?;
    if bytes.len() as u64 > max_bytes {
        return Err(CoreError::invalid("ASTRA_EMU_PROFILE_BOUND", "profile exceeds the core's size limit; select the private profile rather than an archive or scheme database"));
    }
    let hash = Hash256::from_sha256(&bytes);
    Ok((bytes, hash, root))
}

fn profile_io_error(cause: std::io::Error) -> CoreError {
    match cause.kind() {
        ErrorKind::NotFound => CoreError::invalid("ASTRA_EMU_PROFILE_MISSING", "private profile is missing; configure an existing profile or import the selected game's scheme before launching"),
        ErrorKind::PermissionDenied => CoreError::invalid("ASTRA_EMU_PROFILE_PERMISSION", "private profile access was denied; grant this user read access or select a readable private copy"),
        _ => CoreError::invalid("ASTRA_EMU_PROFILE_OPEN", "private profile could not be opened; check the selected file and storage availability"),
    }
}

/// Resolve a profile's explicit resource path within its canonical game root.
pub fn resolve_game_file(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let fail = || CoreError::invalid("ASTRA_EMU_RESOURCE_PATH", "game resource path is invalid");
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains(['\\', ':', '\0'])
        || path.is_absolute()
        || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || path.components().any(|part| !matches!(part, Component::Normal(_)))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn access_errors_distinguish_missing_and_permission_without_echoing_source() {
        for (kind, code) in [
            (ErrorKind::NotFound, "ASTRA_EMU_PROFILE_MISSING"),
            (ErrorKind::PermissionDenied, "ASTRA_EMU_PROFILE_PERMISSION"),
            (ErrorKind::Other, "ASTRA_EMU_PROFILE_OPEN"),
        ] {
            let error = profile_io_error(std::io::Error::new(kind, "private-source-path"));
            assert_eq!(error.code(), code);
            assert!(!error.to_string().contains("private-source-path"));
        }
    }
}
