//! Descriptor, probe, and open for the Kirikiri family.

use std::path::Path;

use abi_stable::std_types::ROption;
use astra_emu_family_api::{
    FamilyCapability, FamilyDescriptor, FamilyError, FamilyOpen, FamilyProvider, FamilyResult,
    OpenRequest, ProbeReport, ProbeRequest,
};

#[cfg(feature = "engine")]
use crate::session::{self, KrkrOpen};

pub fn krkr_descriptor() -> FamilyDescriptor {
    FamilyDescriptor {
        family_id: "krkr".into(),
        plugin_id: "astra.emu.krkr".into(),
        abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
        ]
        .into(),
        supported_formats: vec!["krkr.xp3".into()].into(),
    }
}

#[derive(Default)]
pub struct KrkrProvider {
    #[cfg_attr(not(feature = "engine"), allow(dead_code))]
    next_session_id: u64,
}

impl FamilyProvider for KrkrProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let descriptor = krkr_descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    }

    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate().map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_KRKR_PROBE_PATH",
                "the game directory is not readable",
            )
        })?;
        let root = Path::new(request.game_path.as_str());
        if !root.is_dir() {
            return Ok(None);
        }
        let mut has_xp3 = false;
        let entries = std::fs::read_dir(root).map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_KRKR_PROBE_PATH",
                "the game directory is not readable",
            )
        })?;
        for entry in entries.flatten() {
            if entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("xp3"))
            {
                has_xp3 = true;
                break;
            }
        }
        if !has_xp3 {
            return Ok(None);
        }
        let game_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                FamilyError::invalid(
                    "ASTRA_EMU_KRKR_GAME_ID",
                    "the Kirikiri game ID is not valid UTF-8",
                )
            })?;
        let report = ProbeReport {
            family_id: "krkr".into(),
            game_id: game_id.into(),
            format: "krkr.xp3".into(),
            // An XP3 archive is strong but not conclusive; other families may
            // also mount them.
            confidence_permyriad: 9_000,
        };
        report.validate().map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_KRKR_PROBE_REPORT",
                "the Kirikiri probe produced an invalid report",
            )
        })?;
        Ok(Some(report))
    }

    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let _span = tracing::info_span!(
            "krkr_session_open",
            event = "astra.emu.krkr.session.opening"
        )
        .entered();
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let _sink = match request.host.audio_sink {
            ROption::RSome(sink) => sink,
            ROption::RNone => {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_KRKR_AUDIO_SINK",
                    "the Kirikiri family requires the host audio sink",
                ))
            }
        };
        let game_path = Path::new(request.game_path.as_str()).to_owned();
        #[cfg(not(feature = "engine"))]
        {
            let _ = game_path;
            Err(FamilyError::invalid(
                "ASTRA_EMU_KRKR_ENGINE_DISABLED",
                "the vendored Kirikiri engine is not compiled into this plugin",
            ))
        }
        #[cfg(feature = "engine")]
        {
            let KrkrOpen {
                session,
                frame_info,
                audio_format,
            } = session::boot(&game_path, sink)?;
            let id = self.next_session_id;
            self.next_session_id = id.checked_add(1).ok_or_else(|| {
                FamilyError::invalid("ASTRA_EMU_KRKR_SESSION_ID", "session ID exhausted")
            })?;
            tracing::info!(
                event = "astra.emu.krkr.session.open",
                width = frame_info.width,
                height = frame_info.height
            );
            Ok(FamilyOpen {
                response: OpenResponse {
                    session_id: format!("krkr-{id}").into(),
                    frame: frame_info,
                    audio_format: ROption::RSome(audio_format),
                },
                session: Box::new(session),
            })
        }
    }
}
