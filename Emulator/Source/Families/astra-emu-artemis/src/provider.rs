//! Descriptor, probe, and open for the Artemis family.

use std::path::{Path, PathBuf};

use abi_stable::std_types::ROption;
use art3m1s_core::archive as pfs_upk;
use astra_emu_family_api::{
    FamilyCapability, FamilyDescriptor, FamilyOpen, FamilyProvider, FamilyResult, OpenRequest,
    OpenResponse, ProbeReport, ProbeRequest,
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
        let Some(base_archive) = find_base_archive(root) else {
            return Ok(None);
        };
        let has_project_ini = ENTRY_ENCODINGS.iter().any(|encoding| {
            pfs_upk::PfsArchive::open_with_encoding(&base_archive, encoding)
                .ok()
                .is_some_and(|archive| archive.find(session::PROJECT_INI).is_some())
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
        let open = session::boot(&game_path, request.initial_window, sink)?;
        let id = self.next_session_id;
        self.next_session_id = id.checked_add(1).ok_or_else(|| {
            error::invalid("ASTRA_EMU_ARTEMIS_SESSION_ID", "session ID exhausted")
        })?;
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

/// Finds the base PFS archive of a game directory: a `*.pfs` file that is
/// not a split-volume part (`*.pfs.000`). Ties break on the lexicographically
/// first file name.
pub(crate) fn find_base_archive(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return false;
            }
            let lower = entry.file_name().to_string_lossy().to_ascii_lowercase();
            lower.ends_with(".pfs") && !lower.starts_with("._")
        })
        .map(|entry| entry.path())
        .min_by_key(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase()
        })
}
