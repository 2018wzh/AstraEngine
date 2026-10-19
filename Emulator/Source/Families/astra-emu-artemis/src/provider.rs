//! Native descriptor/configuration and original PFS probing.
use crate::{
    error::{self, NativeResult},
    session,
};
use art3m1s_core::archive as pfs_upk;
use astra_emu_abi::*;
use encoding_rs::{Encoding, GB18030, SHIFT_JIS, UTF_8};
use std::path::{Path, PathBuf};
pub(crate) const FIELD_COUNT: usize = 2;
pub(crate) fn fields() -> [ConfigField; FIELD_COUNT] {
    let archive = ConfigField {
        id: Bytes::new(b"archive"),
        label: Bytes::new("基础 PFS 归档".as_bytes()),
        group: Bytes::new("文件".as_bytes()),
        default_value: ConfigValue {
            kind: VALUE_STRING,
            reserved: 0,
            integer: 0,
            number: 0.0,
            text: Bytes::EMPTY,
        },
        integer_min: 0,
        integer_max: 0,
        number_min: 0.0,
        number_max: 0.0,
        max_bytes: 255,
        flags: CONFIG_FLAG_FILE_PATH | CONFIG_FLAG_GAME_RELATIVE,
        choices: std::ptr::null(),
        choice_count: 0,
        description: Bytes::new(
            "作为基础资源的 PFS 文件，须直接位于游戏目录下；留空时自动选用目录中唯一的 PFS 文件。"
                .as_bytes(),
        ),
        choice_labels: std::ptr::null(),
        step: 0.0,
        unit: Bytes::EMPTY,
    };
    [
        archive,
        ConfigField {
            id: Bytes::new(b"extra_archive"),
            label: Bytes::new("额外 PFS 补丁".as_bytes()),
            max_bytes: 4096,
            description: Bytes::new(
                "叠加在基础归档之上的补丁文件，路径相对游戏目录；留空则不加载额外补丁。".as_bytes(),
            ),
            ..archive
        },
    ]
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Configuration {
    pub archive: String,
    pub extra_archive: String,
}

/// Parse validated named entries; field order is not an ABI contract.
///
/// SAFETY: every nested entry span must be readable for this call.
pub(crate) unsafe fn parse_configuration(entries: &[ConfigEntry]) -> NativeResult<Configuration> {
    unsafe { validate_config(&fields(), entries) }.map_err(|e| {
        error::invalid(
            "ASTRA_EMU_ARTEMIS_CONFIG",
            format!("configuration entry {}: {}", e.index, e.reason),
        )
    })?;
    let mut config = Configuration::default();
    for entry in entries {
        let id = unsafe { entry.id.text(128) }
            .map_err(|_| error::invalid("ASTRA_EMU_ARTEMIS_CONFIG", "invalid field ID"))?;
        let (target, maximum) = match id {
            "archive" => (&mut config.archive, 255),
            "extra_archive" => (&mut config.extra_archive, 4096),
            _ => unreachable!("validated configuration field"),
        };
        let value = unsafe { entry.value.text.text(maximum) }
            .map_err(|_| error::invalid("ASTRA_EMU_ARTEMIS_CONFIG", "invalid field text"))?;
        target.push_str(value);
    }
    Ok(config)
}
pub(crate) fn probe(root: &Path) -> NativeResult<Option<String>> {
    if !root.is_dir() {
        return Ok(None);
    }
    let entries = std::fs::read_dir(root)
        .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_PROBE_PATH", e.to_string()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_PROBE_PATH", e.to_string()))?
            .path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pfs"))
            && ENTRY_ENCODINGS.iter().any(|encoding| {
                pfs_upk::PfsArchive::open_with_encoding(&path, encoding)
                    .ok()
                    .is_some_and(|archive| archive.find(session::PROJECT_INI).is_some())
            })
        {
            return root
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| Some(n.to_owned()))
                .ok_or_else(|| {
                    error::invalid("ASTRA_EMU_ARTEMIS_GAME_ID", "game ID is not valid UTF-8")
                });
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
        let entry = ConfigEntry {
            id: Bytes::new(b"unknown"),
            value: fields[0].default_value,
        };
        assert!(unsafe { parse_configuration(&[entry]) }.is_err());
        let entry = ConfigEntry {
            id: fields[0].id,
            value: ConfigValue {
                kind: VALUE_BOOL,
                ..fields[0].default_value
            },
        };
        assert!(unsafe { parse_configuration(&[entry]) }.is_err());
    }

    fn string_entry<'a>(id: &'a [u8], value: &'a [u8]) -> ConfigEntry {
        ConfigEntry {
            id: Bytes::new(id),
            value: ConfigValue {
                kind: VALUE_STRING,
                reserved: 0,
                integer: 0,
                number: 0.0,
                text: Bytes::new(value),
            },
        }
    }

    #[test]
    fn named_configuration_is_order_independent_and_defaults_are_unchanged() {
        assert_eq!(
            unsafe { parse_configuration(&[]) }.unwrap(),
            Configuration::default()
        );
        let archive = string_entry(b"archive", b"base.pfs");
        let extra = string_entry(b"extra_archive", "patches/中文.bak".as_bytes());
        let expected = Configuration {
            archive: "base.pfs".into(),
            extra_archive: "patches/中文.bak".into(),
        };
        assert_eq!(
            unsafe { parse_configuration(&[extra, archive]) }.unwrap(),
            expected
        );
        assert_eq!(
            unsafe { parse_configuration(&[archive, extra]) }.unwrap(),
            expected
        );
        assert_eq!(
            unsafe { parse_configuration(&[extra]) }.unwrap(),
            Configuration {
                archive: String::new(),
                extra_archive: expected.extra_archive
            }
        );
        assert!(unsafe { parse_configuration(&[archive, archive]) }.is_err());
        let oversized = vec![b'a'; 4097];
        assert!(
            unsafe { parse_configuration(&[string_entry(b"extra_archive", &oversized)]) }.is_err()
        );
        let schema = fields();
        assert_eq!(schema.len(), FIELD_COUNT);
        assert_eq!(unsafe { schema[0].id.text(128) }.unwrap(), "archive");
        assert_eq!(unsafe { schema[1].id.text(128) }.unwrap(), "extra_archive");
        assert!(unsafe { schema[1].default_value.text.text(4096) }
            .unwrap()
            .is_empty());
    }

    /// Public standalone PF6 with exactly one system.ini resource.
    fn ini_pfs(ini: &[u8]) -> Vec<u8> {
        let name = b"system.ini";
        let entry_size = 4 + name.len() + 12;
        let index_size = 4 + entry_size + 4 + 16 + 4;
        let mut bytes = b"pf6".to_vec();
        bytes.extend_from_slice(&(index_size as u32).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&((7 + index_size) as u32).to_le_bytes());
        bytes.extend_from_slice(&(ini.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&((11 + entry_size) as u32).to_le_bytes());
        bytes.extend_from_slice(ini);
        bytes
    }

    #[test]
    fn parsed_configuration_reaches_native_project_reader_without_source_writes() {
        let root = tempfile::tempdir().unwrap();
        let base_ini = b"[WINDOWS]\nWIDTH=800\nHEIGHT=600\nBOOT=boot.iet\nCHARSET=UTF-8\n";
        let active_ini = b"[WINDOWS]\nWIDTH=960\nHEIGHT=540\nBOOT=boot.iet\nCHARSET=UTF-8\n";
        let extra_ini = b"[WINDOWS]\nWIDTH=1280\nHEIGHT=720\nBOOT=boot.iet\nCHARSET=UTF-8\n";
        let base = ini_pfs(base_ini);
        let active = ini_pfs(active_ini);
        let extra = ini_pfs(extra_ini);
        std::fs::write(root.path().join("base.pfs"), &base).unwrap();
        std::fs::write(root.path().join("base.pfs.999"), &active).unwrap();
        std::fs::write(root.path().join("selection.bak"), &extra).unwrap();
        std::fs::create_dir(root.path().join("savedata")).unwrap();
        std::fs::write(root.path().join("savedata/system.dat"), b"save sentinel").unwrap();
        let config = unsafe {
            parse_configuration(&[
                string_entry(b"extra_archive", b"selection.bak"),
                string_entry(b"archive", b"base.pfs"),
            ])
        }
        .unwrap();
        let archive = find_base_archive(root.path(), &config.archive).unwrap();
        let (resources, ini) =
            session::project_resources(&archive, &config.extra_archive, || Ok(())).unwrap();
        assert_eq!(ini, extra_ini);
        let project = art3m1s_core::ProjectConfig::from_system_ini_bytes(&ini, "WINDOWS").unwrap();
        assert_eq!((project.stage_width, project.stage_height), (1280, 720));
        assert_eq!(project.charset, "UTF-8");
        resources.clear();
        let (resources, ini) = session::project_resources(&archive, "", || Ok(())).unwrap();
        assert_eq!(ini, active_ini);
        resources.clear();
        for (name, expected) in [
            ("base.pfs", base),
            ("base.pfs.999", active),
            ("selection.bak", extra),
        ] {
            assert_eq!(std::fs::read(root.path().join(name)).unwrap(), expected);
        }
        assert_eq!(
            std::fs::read(root.path().join("savedata/system.dat")).unwrap(),
            b"save sentinel"
        );
        assert!(session::project_resources(&archive, "../selection.bak", || Ok(())).is_err());
        assert!(session::project_resources(&archive, "", || {
            Err(error::invalid("TEST_CANCELLED", "cancelled"))
        })
        .is_err());
    }
}
