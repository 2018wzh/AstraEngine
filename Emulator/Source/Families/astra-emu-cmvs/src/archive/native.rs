use super::*;
use crate::{CmvsPs2aVmState, CmvsResourceSearchBank};
use std::path::Path;

const NATIVE_PREFIX: &str = "cmvs:/native-files/";

pub(crate) fn validate_search_path(path: &str) -> Result<(), CoreError> {
    let (bytes, _, invalid_encoding) = encoding_rs::SHIFT_JIS.encode(path);
    if invalid_encoding || bytes.len() >= 2048 || path.contains(['\0', ':']) {
        return Err(path_error());
    }
    let normalized = path.replace('\\', "/");
    let relative = normalized.strip_suffix('/').unwrap_or(&normalized);
    if !relative.is_empty()
        && relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(path_error());
    }
    if normalized.starts_with('/') {
        return Err(path_error());
    }
    Ok(())
}

fn path_error() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_RESOURCE_PATH",
        "native resource path is outside the bounded game-relative domain",
    )
}

impl CmvsArchive {
    pub(crate) fn bind_game_root(&mut self, root: PathBuf) -> Result<(), CoreError> {
        if self
            .entries
            .keys()
            .any(|uri| uri.starts_with(NATIVE_PREFIX))
        {
            return Err(path_error());
        }
        self.game_root = Some(root);
        Ok(())
    }

    /// Resolve only the selected native table, in slot order. The initial
    /// boot script is selected separately, before any table has been configured.
    pub fn resolve_native_resource(
        &self,
        vm: &CmvsPs2aVmState,
        bank: CmvsResourceSearchBank,
        name: &str,
    ) -> Result<String, CoreError> {
        validate_search_path(name)?;
        if name.is_empty() || name.ends_with(['/', '\\']) {
            return Err(path_error());
        }
        let root = self.game_root.as_deref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_RESOURCE_ROOT",
                "native source lookup requires a mounted game root",
            )
        })?;
        let paths = vm.resource_search_paths.get(&bank).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_RESOURCE_UNBOUND",
                "native resource search table is not configured",
            )
        })?;
        let search_count = match bank {
            CmvsResourceSearchBank::Script | CmvsResourceSearchBank::CrossfadeAudio => 4,
            CmvsResourceSearchBank::Texture => 10,
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_RESOURCE_UNBOUND",
                    "native resource reader is not implemented",
                ))
            }
        };
        for (_, path) in paths.range(..search_count) {
            validate_search_path(path)?;
            if path.is_empty() || path.ends_with(['/', '\\']) {
                let relative = format!("{}{name}", path.replace('\\', "/")).replace('\\', "/");
                if game_file(root, &relative)?.is_some() {
                    return Ok(format!("{NATIVE_PREFIX}{relative}"));
                }
                continue;
            }
            let Some(source) = game_file(root, &path.replace('\\', "/"))? else {
                continue;
            };
            let index = self
                .archives
                .iter()
                .position(|archive| archive.path == source)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_RESOURCE_ARCHIVE",
                        "native search names an archive absent from the private profile",
                    )
                })?;
            // The native CPZ reader scans directories and files in index order
            // when no explicit directory selector is supplied.
            if let Some(entry) = self
                .entries
                .values()
                .filter(|entry| {
                    entry.archive == index
                        && entry
                            .descriptor
                            .relative_path
                            .rsplit('/')
                            .next()
                            .is_some_and(|leaf| leaf.eq_ignore_ascii_case(name))
                })
                .min_by_key(|entry| entry.ordinal)
            {
                return Ok(entry.uri.clone());
            }
        }
        Err(invalid(
            "ASTRA_EMU_CMVS_RESOURCE_NOT_FOUND",
            "native resource was not found in its configured search table",
        ))
    }

    pub(super) fn native_file(&self, uri: &str) -> Result<Option<PathBuf>, CoreError> {
        let Some(relative) = uri.strip_prefix(NATIVE_PREFIX) else {
            return Ok(None);
        };
        let root = self.game_root.as_deref().ok_or_else(path_error)?;
        validate_search_path(relative)?;
        game_file(root, relative)?
            .map(Some)
            .ok_or_else(|| invalid("ASTRA_EMU_VFS_NOT_FOUND", "native game file is unavailable"))
    }

    pub(super) fn resource_bytes(&self, uri: &str) -> Result<(OwnedByteBuffer, bool), CoreError> {
        if let Some(path) = self.native_file(uri)? {
            return Ok((
                read_native_range(&path, 0, native_size(&path)?)?.into(),
                false,
            ));
        }
        if let Some(role) = self.loose_role_from_uri(uri) {
            let source = &self.loose[&role];
            return Ok((
                self.read_loose_range(source, 0, source.byte_size)?.into(),
                false,
            ));
        }
        self.decode_entry(self.entry(uri)?)
    }
}

fn game_file(root: &Path, relative: &str) -> Result<Option<PathBuf>, CoreError> {
    validate_search_path(relative)?;
    match std::fs::metadata(root.join(relative)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(invalid(
            "ASTRA_EMU_CMVS_SOURCE_IO",
            "native source metadata could not be read",
        )),
        Ok(metadata) if metadata.is_file() => {
            astra_emu_sdk::resolve_game_file(root, relative).map(Some)
        }
        Ok(_) => Err(path_error()),
    }
}

pub(super) fn native_size(path: &Path) -> Result<u64, CoreError> {
    let size = std::fs::metadata(path).map_err(|_| path_error())?.len();
    if size > MAX_ENTRY_BYTES {
        return Err(path_error());
    }
    Ok(size)
}

pub(super) fn read_native_range(
    path: &Path,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>, CoreError> {
    let mut file = File::open(path).map_err(|_| path_error())?;
    let before = file.metadata().map_err(|_| path_error())?;
    if before.len() > MAX_ENTRY_BYTES
        || offset
            .checked_add(length)
            .is_none_or(|end| end > before.len())
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_READ_RANGE",
            "native file range exceeds its bounds",
        ));
    }
    let mut bytes = vec![0; usize::try_from(length).map_err(|_| path_error())?];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|_| path_error())?;
    if source_stamp(&before)? != source_stamp(&file.metadata().map_err(|_| path_error())?)? {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SOURCE_CHANGED",
            "native file changed during read",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
