//! Descriptor, probe, and open for the Artemis family.

use std::path::{Path, PathBuf};

use abi_stable::std_types::ROption;
use art3m1s_core::archive as pfs_upk;
use astra_emu_family_api::{
    resolve_config, ConfigField, ConfigKind, ConfigValue, FamilyCapability, FamilyDescriptor,
    FamilyOpen, FamilyProvider, FamilyResult, OpenRequest, OpenResponse, ProbeReport, ProbeRequest,
};
use encoding_rs::{Encoding, GB18030, SHIFT_JIS, UTF_8};

use crate::{error, session};

pub fn artemis_descriptor() -> FamilyDescriptor {
    FamilyDescriptor {
        family_id: "artemis".into(),
        plugin_id: "astra.emu.artemis".into(),
        abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
        ]
        .into(),
        supported_formats: vec!["artemis.pfs".into()].into(),
        configuration: vec![ConfigField {
            id: "archive".into(),
            label: "Base PFS archive (empty selects the only archive)".into(),
            group: "Files".into(),
            kind: ConfigKind::String { max_bytes: 255 },
            default: ConfigValue::String("".into()),
        }]
        .into(),
    }
}

/// Entry-name encodings tried in order when opening a PFS archive. Artemis
/// titles store entry names either as UTF-8 (all recent games) or in a
/// legacy Windows code page.
pub(crate) const ENTRY_ENCODINGS: [&Encoding; 3] = [UTF_8, SHIFT_JIS, GB18030];

#[derive(Default)]
pub struct ArtemisProvider {
    next_session_id: u64,
}

impl FamilyProvider for ArtemisProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let descriptor = artemis_descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    }

    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate().map_err(|_| {
            error::invalid(
                "ASTRA_EMU_ARTEMIS_PROBE_PATH",
                "the game directory is not readable",
            )
        })?;
        let root = Path::new(request.game_path.as_str());
        if !root.is_dir() {
            return Ok(None);
        }
        // The complete Artemis layout signature is a readable PFS base
        // archive whose index contains the boot configuration entry. Use the
        // engine's own reader so a probe hit means the engine can actually
        // boot the directory.
        let has_project_ini = std::fs::read_dir(root)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                let path = entry.path();
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("pfs"))
                    && ENTRY_ENCODINGS.iter().any(|encoding| {
                        pfs_upk::PfsArchive::open_with_encoding(&path, encoding)
                            .ok()
                            .is_some_and(|archive| archive.find(session::PROJECT_INI).is_some())
                    })
            });
        if !has_project_ini {
            return Ok(None);
        }
        let game_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_GAME_ID",
                    "the Artemis game ID is not valid UTF-8",
                )
            })?;
        let report = ProbeReport {
            family_id: "artemis".into(),
            game_id: game_id.into(),
            format: "artemis.pfs".into(),
            // PFS base archive + system.ini is the complete Artemis layout.
            confidence_permyriad: 10_000,
        };
        report.validate().map_err(|_| {
            error::invalid(
                "ASTRA_EMU_ARTEMIS_PROBE_REPORT",
                "the Artemis probe produced an invalid report",
            )
        })?;
        Ok(Some(report))
    }

    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let _span = tracing::info_span!(
            "artemis_session_open",
            event = "astra.emu.artemis.session.opening"
        )
        .entered();
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let config = resolve_config(&descriptor.configuration, &request.configuration)?;
        let ConfigValue::String(archive) = &config[0].value else {
            unreachable!()
        };
        let archive = find_base_archive(Path::new(request.game_path.as_str()), archive.as_str())?;
        let lease = SessionLease::acquire()?;
        let sink = match request.host.audio_sink {
            ROption::RSome(sink) => sink,
            ROption::RNone => {
                return Err(error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_SINK",
                    "the Artemis family requires the host audio sink",
                ))
            }
        };
        let game_path = Path::new(request.game_path.as_str()).to_owned();
        let id = self.next_session_id;
        self.next_session_id = id.checked_add(1).ok_or_else(|| {
            error::invalid("ASTRA_EMU_ARTEMIS_SESSION_ID", "session ID exhausted")
        })?;
        let open = session::boot(&game_path, &archive, request.initial_window, sink, lease)?;
        let frame_info = open.frame_info;
        tracing::info!(
            event = "astra.emu.artemis.session.open",
            width = frame_info.width,
            height = frame_info.height
        );
        Ok(FamilyOpen {
            response: OpenResponse {
                session_id: format!("artemis-{id}").into(),
                frame: frame_info,
                audio_format: ROption::RSome(crate::audio::OUTPUT_FORMAT),
            },
            session: Box::new(open.session),
        })
    }
}

pub(crate) fn find_base_archive(root: &Path, selected: &str) -> FamilyResult<PathBuf> {
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
    fn acquire() -> FamilyResult<Self> {
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
        let d = artemis_descriptor();
        d.validate().unwrap();
        for entry in [
            astra_emu_family_api::ConfigEntry {
                id: "unknown".into(),
                value: ConfigValue::String("".into()),
            },
            astra_emu_family_api::ConfigEntry {
                id: "archive".into(),
                value: ConfigValue::Bool(true),
            },
        ] {
            assert!(resolve_config(&d.configuration, &[entry]).is_err());
        }
    }
}
