use crate::{
    audio::Audio,
    mount_musica,
    scene::{core_error, error, Scene},
    session::MusicaSession,
    storage::Storage,
    MusicaVm, ScriptEncoding, MUSICA_PROFILE_FILE,
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
                    "ASTRA_EMU_MUSICA_ACTIVE",
                    "a Musica session is already active",
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
pub struct MusicaProvider {
    next: u64,
}
pub fn create_musica_provider() -> Box<dyn FamilyProvider> {
    Box::new(MusicaProvider::default())
}
pub fn musica_descriptor() -> FamilyDescriptor {
    let field = |id: &str, label: &str, default: &str| ConfigField {
        id: id.into(),
        label: label.into(),
        group: "Files".into(),
        kind: ConfigKind::String { max_bytes: 256 },
        default: ConfigValue::String(default.into()),
    };
    let mut configuration = vec![
        field("profile_file", "Private PAZ profile", MUSICA_PROFILE_FILE),
        field("entry_script", "Entry script in scr archive", "test.sc"),
    ];
    configuration.push(ConfigField {
        id: "launch_mode".into(),
        label: "Launch mode".into(),
        group: "Playback".into(),
        kind: ConfigKind::Enum {
            choices: vec!["direct".into(), "title".into()].into(),
        },
        default: ConfigValue::Enum("direct".into()),
    });
    configuration.push(ConfigField {
        id: "message_speed_auto_play".into(),
        label: "Auto message delay (10 ms units)".into(),
        group: "Playback".into(),
        kind: ConfigKind::Integer { min: 0, max: 100 },
        default: ConfigValue::Integer(50),
    });
    configuration.push(ConfigField {
        id: "progress_in_background".into(),
        label: "Continue playback in background".into(),
        group: "Playback".into(),
        kind: ConfigKind::Bool,
        default: ConfigValue::Bool(false),
    });
    configuration.push(ConfigField {
        id: "text_shadow".into(),
        label: "Text shadow".into(),
        group: "Presentation".into(),
        kind: ConfigKind::Bool,
        default: ConfigValue::Bool(true),
    });
    configuration.push(ConfigField {
        id: "script_encoding".into(),
        label: "Script encoding".into(),
        group: "Script".into(),
        kind: ConfigKind::Enum {
            choices: vec!["shift_jis".into(), "gbk".into()].into(),
        },
        default: ConfigValue::Enum("shift_jis".into()),
    });
    configuration.extend(crate::voice_preferences::VoicePreferences::fields());
    configuration.extend(crate::audio::AudioPreferences::fields());
    FamilyDescriptor {
        family_id: "musica".into(),
        plugin_id: "astra.emu.musica".into(),
        abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
            FamilyCapability::TextReplacement,
        ]
        .into(),
        supported_formats: vec!["musica.paz".into(), "musica.sc".into()].into(),
        configuration: configuration.into(),
    }
}
impl FamilyProvider for MusicaProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let d = musica_descriptor();
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
            family_id: "musica".into(),
            game_id: root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("musica")
                .into(),
            format: "musica.paz".into(),
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
impl MusicaProvider {
    pub(crate) fn open_session(
        &mut self,
        request: OpenRequest,
    ) -> FamilyResult<(OpenResponse, MusicaSession)> {
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let config = resolve_config(&descriptor.configuration, &request.configuration)?;
        let get = |key: &str| -> FamilyResult<String> {
            match &config
                .iter()
                .find(|e| e.id == key)
                .ok_or_else(|| error("ASTRA_EMU_MUSICA_CONFIG", "configuration is missing"))?
                .value
            {
                ConfigValue::String(s) => Ok(s.to_string()),
                _ => Err(error(
                    "ASTRA_EMU_MUSICA_CONFIG",
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
                "ASTRA_EMU_MUSICA_ENTRY",
                "entry script must name one .sc file in the scr archive",
            ));
        }
        let audio_preferences = crate::audio::AudioPreferences::resolve(&config)?;
        let progress_in_background = match config
            .iter()
            .find(|entry| entry.id == "progress_in_background")
            .map(|entry| &entry.value)
        {
            Some(ConfigValue::Bool(value)) => *value,
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_CONFIG",
                    "background playback preference is missing or invalid",
                ))
            }
        };
        let text_shadow = match config
            .iter()
            .find(|e| e.id == "text_shadow")
            .map(|e| &e.value)
        {
            Some(ConfigValue::Bool(value)) => *value,
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_CONFIG",
                    "text shadow preference is missing or invalid",
                ))
            }
        };
        let encoding = match config
            .iter()
            .find(|e| e.id == "script_encoding")
            .map(|e| &e.value)
        {
            Some(ConfigValue::Enum(value)) if value == "shift_jis" => ScriptEncoding::ShiftJis,
            Some(ConfigValue::Enum(value)) if value == "gbk" => ScriptEncoding::Gbk,
            _ => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_CONFIG",
                    "unsupported script encoding",
                ))
            }
        };
        let title_launch = match config
            .iter()
            .find(|e| e.id == "launch_mode")
            .map(|e| &e.value)
        {
            Some(ConfigValue::Enum(value)) if value == "title" => true,
            Some(ConfigValue::Enum(value)) if value == "direct" => false,
            _ => return Err(error("ASTRA_EMU_MUSICA_CONFIG", "invalid launch mode")),
        };
        let focused = request.initial_window.focused;
        let lease = SessionLease::acquire()?;
        let root = Path::new(request.game_path.as_str());
        let archive = Arc::new(mount_musica(root, Path::new(&profile)).map_err(core_error)?);
        let uri = format!("musica:/scr/{entry}");
        let primary_encoding = encoding;
        let loaded = crate::script_loader::load_script(&archive, &uri, primary_encoding)?;
        let encoding = loaded.script.encoding;
        let mut vm = MusicaVm::new(uri, loaded.hash, loaded.script, 0)
            .map_err(|_| error("ASTRA_EMU_MUSICA_VM", "entry script cannot be initialized"))?;
        vm.set_voice_preferences(crate::voice_preferences::VoicePreferences::resolve(
            &config,
        )?);
        let auto_delay = config
            .iter()
            .find(|entry| entry.id == "message_speed_auto_play")
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_CONFIG", "auto delay is missing"))?;
        let ConfigValue::Integer(auto_delay) = auto_delay.value else {
            return Err(error(
                "ASTRA_EMU_MUSICA_CONFIG",
                "auto delay type is invalid",
            ));
        };
        vm.set_auto_delay_units(
            u8::try_from(auto_delay)
                .map_err(|_| error("ASTRA_EMU_MUSICA_CONFIG", "auto delay is out of bounds"))?,
        )
        .map_err(|_| error("ASTRA_EMU_MUSICA_CONFIG", "auto delay is out of bounds"))?;
        let game = Hash256::from_sha256(&serde_json::to_vec(archive.manifest()).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_IDENTITY",
                "archive identity cannot be encoded",
            )
        })?);
        let storage = Storage::new(root)?;
        let quick_cursor = storage.quick_cursor(game)?;
        vm.merge_verified_gallery_unlocks(&storage.progress(game)?)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_GLOBAL_PROGRESS",
                    "global progress cannot be applied",
                )
            })?;
        let mut scene = Scene::new(archive.clone(), 1280, 720, encoding)?;
        scene.set_text_shadow(text_shadow);
        if title_launch {
            vm.begin_title_launch()
                .map_err(|_| error("ASTRA_EMU_MUSICA_TITLE", "title session cannot start"))?;
            scene.render_title(vm.title_variant(), None)?;
        } else {
            scene.render(vm.state(), None, None)?;
        }
        let replacement = request.host.text_replacement.into_option();
        if let Some(service) = &replacement {
            service.reset(TextResetReason::NewGame).into_result()?;
        }
        let sink = request
            .host
            .audio_sink
            .into_option()
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_AUDIO_SINK", "PCM sink is required"))?;
        let audio = Audio::start(archive.clone(), sink)?;
        audio.set_preferences(audio_preferences)?;
        audio.suspend(!focused && !progress_in_background)?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_SESSION_ID", "session counter overflowed"))?;
        let id = format!("musica.{}", self.next);
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
        let session = MusicaSession::new(
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
            focused,
            progress_in_background,
            primary_encoding,
            quick_cursor,
        );
        tracing::info!(event = "astra.emu.musica.session.open");
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
            "ASTRA_EMU_MUSICA_CONFIG_PATH",
            "configuration path must be a safe relative file",
        ))
    } else {
        Ok(())
    }
}
