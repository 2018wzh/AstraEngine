//! Native descriptor/configuration and original PFS probing.
use std::path::{Path, PathBuf};
use art3m1s_core::archive as pfs_upk;
use astra_emu_abi::*;
use encoding_rs::{Encoding, GB18030, SHIFT_JIS, UTF_8};
use crate::{error::{self, NativeResult}, session};
pub(crate) fn fields() -> [ConfigField; 1] {
    [ConfigField {
        id: Bytes::new(b"archive"), label: Bytes::new(b"Base PFS archive (empty selects the only archive)"),
        group: Bytes::new(b"Files"), default_value: ConfigValue { kind: VALUE_STRING, reserved: 0, integer: 0, number: 0.0, text: Bytes::EMPTY },
        integer_min: 0, integer_max: 0, number_min: 0.0, number_max: 0.0, max_bytes: 255, reserved: 0,
        choices: std::ptr::null(), choice_count: 0,
    }]
}
pub(crate) fn probe(root: &Path) -> NativeResult<Option<String>> {
    if !root.is_dir() { return Ok(None); }
    let entries = std::fs::read_dir(root).map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_PROBE_PATH", e.to_string()))?;
    for entry in entries {
        let path = entry.map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_PROBE_PATH", e.to_string()))?.path();
        if path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pfs"))
            && ENTRY_ENCODINGS.iter().any(|encoding| pfs_upk::PfsArchive::open_with_encoding(&path, encoding).ok().is_some_and(|archive| archive.find(session::PROJECT_INI).is_some())) {
            return root.file_name().and_then(|n| n.to_str()).map(|n| Some(n.to_owned()))
                .ok_or_else(|| error::invalid("ASTRA_EMU_ARTEMIS_GAME_ID", "game ID is not valid UTF-8"));
        }
    }
    Ok(None)
}
/// Entry-name encodings tried in order when opening a PFS archive. Artemis
/// titles store entry names either as UTF-8 (all recent games) or in a
/// legacy Windows code page.
pub(crate) const ENTRY_ENCODINGS: [&Encoding; 3] = [UTF_8, SHIFT_JIS, GB18030];

pub(crate) fn find_base_archive(root: &Path, selected: &str) -> NativeResult<PathBuf> {
    if !selected.is_empty() {
        if selected.contains(['/', '\\', ':']) || !selected.to_ascii_lowercase().ends_with(".pfs") {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_ARCHIVE",
                "base archive must be a direct PFS filename",
            ));
        }
        let path = root.join(selected);
        if !path.is_file() {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_ARCHIVE",
                "selected archive is unavailable",
            ));
        }
        return Ok(path);
    }
    let mut candidates = std::fs::read_dir(root)
        .map_err(|_| error::invalid("ASTRA_EMU_ARTEMIS_ARCHIVE", "game directory is unreadable"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pfs")));
    let first = candidates
        .next()
        .ok_or_else(|| error::invalid("ASTRA_EMU_ARTEMIS_ARCHIVE", "no PFS base archive"))?;
    if candidates.next().is_some() {
        return Err(error::invalid(
            "ASTRA_EMU_ARTEMIS_ARCHIVE_AMBIGUOUS",
            "select the base PFS archive in core configuration",
        ));
    }
    Ok(first)
}
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) struct SessionLease;
impl SessionLease {
    pub(crate) fn acquire() -> NativeResult<Self> {
        ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map_err(|_| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_SESSION_ACTIVE",
                    "one Artemis session may be active",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_selection_requires_unambiguous_safe_filename() {
        let root = tempfile::tempdir().unwrap();
        assert!(find_base_archive(root.path(), "").is_err());
        std::fs::write(root.path().join("data.pfs"), []).unwrap();
        assert!(find_base_archive(root.path(), "").is_ok());
        std::fs::write(root.path().join("patch.pfs"), []).unwrap();
        assert!(find_base_archive(root.path(), "").is_err());
        assert!(find_base_archive(root.path(), "data.pfs").is_ok());
        for name in ["../data.pfs", "x/data.pfs", "x:data.pfs", "missing.pfs"] {
            assert!(find_base_archive(root.path(), name).is_err());
        }
    }
    #[test]
    fn typed_archive_configuration_rejects_unknown_and_wrong_type() {
        let fields = fields();
        let entry = ConfigEntry { id: Bytes::new(b"unknown"), value: fields[0].default_value };
        assert!(unsafe { validate_config(&fields, &[entry]) }.is_err());
        let entry = ConfigEntry { id: fields[0].id, value: ConfigValue { kind: VALUE_BOOL, ..fields[0].default_value } };
        assert!(unsafe { validate_config(&fields, &[entry]) }.is_err());
    }
}
