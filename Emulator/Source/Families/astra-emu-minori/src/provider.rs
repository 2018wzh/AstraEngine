use crate::{
    audio::Audio,
    mount_minori, parse_sc,
    scene::{core_error, error, read_asset, Scene},
    session::MinoriSession,
    storage::Storage,
    MinoriVm, ScOpcodeCatalog, MINORI_PROFILE_FILE,
};
use abi_stable::std_types::ROption;
use astra_core::Hash256;
use astra_emu_family_api::*;
use std::{
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) struct SessionLease;
impl SessionLease {
    fn acquire() -> FamilyResult<Self> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_ACTIVE",
                    "a Minori session is already active",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
#[derive(Default)]
pub struct MinoriProvider {
    next: u64,
}
pub fn create_minori_provider() -> Box<dyn FamilyProvider> {
    Box::new(MinoriProvider::default())
}
pub fn minori_descriptor() -> FamilyDescriptor {
    let field = |id: &str, label: &str, default: &str| ConfigField {
        id: id.into(),
        label: label.into(),
        group: "Files".into(),
        kind: ConfigKind::String { max_bytes: 256 },
        default: ConfigValue::String(default.into()),
    };
    FamilyDescriptor {
        family_id: "minori".into(),
        plugin_id: "astra.emu.minori".into(),
        abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
            FamilyCapability::TextReplacement,
        ]
        .into(),
        supported_formats: vec!["minori.paz".into(), "minori.sc".into()].into(),
        configuration: vec![
            field("profile_file", "Private PAZ profile", MINORI_PROFILE_FILE),
            field("entry_script", "Entry script in scr archive", "test.sc"),
        ]
        .into(),
    }
}
impl FamilyProvider for MinoriProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let d = minori_descriptor();
        d.validate()?;
        Ok(d)
    }
    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request.validate()?;
        let root = Path::new(request.game_path.as_str());
        if !crate::REQUIRED_ARCHIVE_ROLES
            .iter()
            .all(|r| root.join(format!("{r}.paz")).is_file())
        {
            return Ok(None);
        }
        let report = ProbeReport {
            family_id: "minori".into(),
            game_id: root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("minori")
                .into(),
            format: "minori.paz".into(),
            confidence_permyriad: 8000,
        };
        report.validate()?;
        Ok(Some(report))
    }
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let (response, session) = self.open_session(request)?;
        Ok(FamilyOpen {
            response,
            session: Box::new(session),
        })
    }
}
impl MinoriProvider {
    pub(crate) fn open_session(
        &mut self,
        request: OpenRequest,
    ) -> FamilyResult<(OpenResponse, MinoriSession)> {
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let config = resolve_config(&descriptor.configuration, &request.configuration)?;
        let get = |key: &str| -> FamilyResult<String> {
            match &config
                .iter()
                .find(|e| e.id == key)
                .ok_or_else(|| error("ASTRA_EMU_MINORI_CONFIG", "configuration is missing"))?
                .value
            {
                ConfigValue::String(s) => Ok(s.to_string()),
                _ => Err(error(
                    "ASTRA_EMU_MINORI_CONFIG",
                    "configuration type is invalid",
                )),
            }
        };
        let profile = get("profile_file")?;
        let entry = get("entry_script")?;
        relative(&profile)?;
        relative(&entry)?;
        if entry.contains('/') || entry.contains('\\') || !entry.ends_with(".sc") {
            return Err(error(
                "ASTRA_EMU_MINORI_ENTRY",
                "entry script must name one .sc file in the scr archive",
            ));
        }
        let lease = SessionLease::acquire()?;
        let root = Path::new(request.game_path.as_str());
        let archive = Arc::new(mount_minori(root, &root.join(profile)).map_err(core_error)?);
        let uri = format!("minori:/scr/{entry}");
        let bytes = read_asset(&archive, &uri, 16 * 1024 * 1024)?;
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori())
            .map_err(|_| error("ASTRA_EMU_MINORI_SCRIPT", "entry script cannot be parsed"))?;
        let vm = MinoriVm::new(uri, Hash256::from_sha256(&bytes), script, 0)
            .map_err(|_| error("ASTRA_EMU_MINORI_VM", "entry script cannot be initialized"))?;
        let game = Hash256::from_sha256(&serde_json::to_vec(archive.manifest()).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_IDENTITY",
                "archive identity cannot be encoded",
            )
        })?);
        let mut scene = Scene::new(archive.clone(), 1280, 720)?;
        scene.render(vm.state(), None)?;
        let storage = Storage::new(root)?;
        let replacement = request.host.text_replacement.into_option();
        if let Some(service) = &replacement {
            service.reset(TextResetReason::NewGame).into_result()?;
        }
        let sink = request
            .host
            .audio_sink
            .into_option()
            .ok_or_else(|| error("ASTRA_EMU_MINORI_AUDIO_SINK", "PCM sink is required"))?;
        let audio = Audio::start(archive.clone(), sink)?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MINORI_SESSION_ID", "session counter overflowed"))?;
        let id = format!("minori.{}", self.next);
        let info = FrameInfo {
            width: 1280,
            height: 720,
            stride: 1280 * 4,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Opaque,
            },
        };
        let response = OpenResponse {
            session_id: id.clone().into(),
            frame: info,
            audio_format: ROption::RSome(crate::audio::FORMAT),
        };
        response.validate_for_descriptor(&descriptor)?;
        let session = MinoriSession::new(
            id,
            info,
            archive,
            vm,
            scene,
            audio,
            replacement,
            storage,
            game,
            lease,
        );
        tracing::info!(event = "astra.emu.minori.session.open");
        Ok((response, session))
    }
}
fn relative(value: &str) -> FamilyResult<()> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || Path::new(value)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        Err(error(
            "ASTRA_EMU_MINORI_CONFIG_PATH",
            "configuration path must be a safe relative file",
        ))
    } else {
        Ok(())
    }
}
