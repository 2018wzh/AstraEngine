use crate::{mount_cmvs, session::CmvsSession, CMVS_PROFILE_FILE};
use abi_stable::{
    prefix_type::PrefixTypeTrait, sabi_types::Constructor, std_types::ROption,
    type_level::downcasting::TD_Opaque,
};
use astra_emu_family_api::*;
use std::{
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
pub(crate) fn error(code: &str, message: &str) -> FamilyError {
    FamilyError::invalid(code, message)
}
pub(crate) fn core(error: astra_emu_sdk::CoreError) -> FamilyError {
    FamilyError::invalid(error.code(), error.message())
}
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) struct Lease;
impl Lease {
    fn acquire() -> FamilyResult<Self> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_CMVS_SESSION_ACTIVE",
                    "one CMVS session may be active",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
#[derive(Default)]
pub struct CmvsProvider {
    next: u64,
}
pub fn cmvs_descriptor() -> FamilyDescriptor {
    let field = |id: &str, label: &str, default: &str| ConfigField {
        id: id.into(),
        label: label.into(),
        group: "Files".into(),
        kind: ConfigKind::String { max_bytes: 256 },
        default: ConfigValue::String(default.into()),
    };
    FamilyDescriptor {
        family_id: "cmvs".into(),
        plugin_id: "astra.emu.cmvs".into(),
        abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![FamilyCapability::CpuFrame].into(),
        supported_formats: vec!["cmvs.cpz5".into()].into(),
        configuration: vec![
            field(
                "profile_file",
                "Private CPZ mount profile",
                CMVS_PROFILE_FILE,
            ),
            field("entry_script", "Entry PS2A script", "start.ps3"),
        ]
        .into(),
    }
}
impl FamilyProvider for CmvsProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let d = cmvs_descriptor();
        d.validate()?;
        Ok(d)
    }
    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate()?;
        let root = Path::new(request.game_path.as_str());
        if !root.is_dir() {
            return Ok(None);
        }
        let found = [root.to_path_buf(), root.join("data/pack")]
            .into_iter()
            .filter_map(|directory| std::fs::read_dir(directory).ok())
            .flatten()
            .take(4096)
            .filter_map(Result::ok)
            .any(|entry| {
                let path = entry.path();
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("cpz"))
                {
                    return false;
                }
                let mut magic = [0; 4];
                std::fs::File::open(path)
                    .and_then(|mut f| f.read_exact(&mut magic))
                    .is_ok()
                    && &magic == b"CPZ5"
            });
        if !found {
            return Ok(None);
        }
        let game_id = root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| error("ASTRA_EMU_CMVS_GAME_ID", "game directory name is invalid"))?;
        Ok(Some(ProbeReport {
            family_id: "cmvs".into(),
            game_id: game_id.into(),
            format: "cmvs.cpz5".into(),
            confidence_permyriad: 9500,
        }))
    }
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let d = self.descriptor()?;
        request.validate_for_descriptor(&d)?;
        let values = resolve_config(&d.configuration, &request.configuration)?;
        let string = |index: usize| match &values[index].value {
            ConfigValue::String(v) => Ok(v.as_str()),
            _ => Err(error("ASTRA_EMU_CMVS_CONFIG", "expected filename")),
        };
        let profile = string(0)?;
        let entry = string(1)?;
        let lease = Lease::acquire()?;
        let archive = Arc::new(
            mount_cmvs(Path::new(request.game_path.as_str()), Path::new(profile)).map_err(core)?,
        );
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_CMVS_SESSION_ID", "session ID exhausted"))?;
        let session = CmvsSession::new(archive, entry, request.initial_window, lease)?;
        Ok(FamilyOpen {
            response: OpenResponse {
                session_id: format!("cmvs-{id}").into(),
                frame: session.frame_info()?,
                audio_format: ROption::RNone,
            },
            session: Box::new(session),
        })
    }
}
extern "C" fn construct_module() -> FamilyModuleBox {
    FamilyModule_TO::from_value(ProviderModule::<CmvsProvider>::default(), TD_Opaque)
}
#[abi_stable::export_root_module]
pub fn astra_cmvs_family_root_module() -> AstraFamilyModuleRef {
    AstraFamilyModule {
        service: Constructor(construct_module),
    }
    .leak_into_prefix()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_uses_native_magic_without_game_specific_names() {
        let root = tempfile::tempdir().unwrap();
        let p = CmvsProvider::default();
        let request = || ProbeRequest {
            game_path: root.path().to_string_lossy().to_string().into(),
        };
        assert!(p.probe(request()).unwrap().is_none());
        std::fs::write(root.path().join("any.cpz"), b"CPZ6").unwrap();
        assert!(p.probe(request()).unwrap().is_none());
        std::fs::write(root.path().join("any.cpz"), b"CPZ5").unwrap();
        assert!(p.probe(request()).unwrap().is_some());
        std::fs::remove_file(root.path().join("any.cpz")).unwrap();
        std::fs::create_dir_all(root.path().join("data/pack")).unwrap();
        std::fs::write(root.path().join("data/pack/any.cpz"), b"CPZ5").unwrap();
        assert!(p.probe(request()).unwrap().is_some());
    }
    #[test]
    fn missing_private_profile_releases_lease_and_preserves_files() {
        let root = tempfile::tempdir().unwrap();
        let mut p = CmvsProvider::default();
        for _ in 0..2 {
            let result = p.open(OpenRequest {
                game_path: root.path().to_string_lossy().to_string().into(),
                configuration: Vec::new().into(),
                initial_window: WindowState {
                    width: 640,
                    height: 480,
                    focused: true,
                    visible: true,
                },
                host: FamilyHostServices {
                    audio_sink: ROption::RNone,
                    text_replacement: ROption::RNone,
                },
            });
            assert!(result.is_err());
            assert!(!ACTIVE.load(Ordering::Acquire));
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
}
