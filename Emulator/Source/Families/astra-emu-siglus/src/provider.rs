//! Descriptor, probe, and open for the Siglus family.

use std::path::Path;

use abi_stable::std_types::ROption;
use astra_emu_family_api::{
    FamilyCapability, FamilyDescriptor, FamilyOpen, FamilyProvider, FamilyResult, OpenRequest,
    OpenResponse, ProbeReport, ProbeRequest,
};
use siglus_scene_vm::resource;

use crate::{error, session};

pub fn siglus_descriptor() -> FamilyDescriptor {
    FamilyDescriptor {
        family_id: "siglus".into(),
        plugin_id: "astra.emu.siglus".into(),
        abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
        ]
        .into(),
        supported_formats: vec!["siglus.scene_pck".into()].into(),
        configuration: Vec::new().into(),
    }
}

#[derive(Default)]
pub struct SiglusProvider {
    next_session_id: u64,
}

impl FamilyProvider for SiglusProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let descriptor = siglus_descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    }

    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate().map_err(|_| {
            error::invalid(
                "ASTRA_EMU_SIGLUS_PROBE_PATH",
                "the game directory is not readable",
            )
        })?;
        let root = Path::new(request.game_path.as_str());
        if !root.is_dir() {
            return Ok(None);
        }
        // Use the engine's own discovery so a probe hit means the engine can
        // actually boot the directory.
        let has_gameexe = resource::find_initial_gameexe_path(root).is_ok();
        let has_scene_pck = resource::find_scene_pck_path(root).is_ok();
        if !has_gameexe || !has_scene_pck {
            return Ok(None);
        }
        let game_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                error::invalid(
                    "ASTRA_EMU_SIGLUS_GAME_ID",
                    "the Siglus game ID is not valid UTF-8",
                )
            })?;
        let report = ProbeReport {
            family_id: "siglus".into(),
            game_id: game_id.into(),
            format: "siglus.scene_pck".into(),
            // Gameexe.dat + Scene.pck is the complete Siglus layout signature.
            confidence_permyriad: 10_000,
        };
        report.validate().map_err(|_| {
            error::invalid(
                "ASTRA_EMU_SIGLUS_PROBE_REPORT",
                "the Siglus probe produced an invalid report",
            )
        })?;
        Ok(Some(report))
    }

    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let _span = tracing::info_span!(
            "siglus_session_open",
            event = "astra.emu.siglus.session.opening"
        )
        .entered();
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let sink = match request.host.audio_sink {
            ROption::RSome(sink) => sink,
            ROption::RNone => {
                return Err(error::invalid(
                    "ASTRA_EMU_SIGLUS_AUDIO_SINK",
                    "the Siglus family requires the host audio sink",
                ))
            }
        };
        let game_path = Path::new(request.game_path.as_str()).to_owned();
        let open = session::boot(&game_path, request.initial_window, sink)?;
        let id = self.next_session_id;
        self.next_session_id = id
            .checked_add(1)
            .ok_or_else(|| error::invalid("ASTRA_EMU_SIGLUS_SESSION_ID", "session ID exhausted"))?;
        let frame_info = open.frame_info;
        tracing::info!(
            event = "astra.emu.siglus.session.open",
            width = frame_info.width,
            height = frame_info.height
        );
        Ok(FamilyOpen {
            response: OpenResponse {
                session_id: format!("siglus-{id}").into(),
                frame: frame_info,
                audio_format: ROption::RSome(crate::audio::OUTPUT_FORMAT),
            },
            session: Box::new(open.session),
        })
    }
}
