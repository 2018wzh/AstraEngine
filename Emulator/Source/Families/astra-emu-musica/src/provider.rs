use crate::{
    MUSICA_KEY_FILE, MusicaVm, ScriptEncoding,
    audio::Audio,
    profile::mount_musica_with_texture_overrides,
    scene::{Scene, core_error, error},
    session::MusicaSession,
    storage::Storage,
};
use abi_stable::std_types::ROption;
use astra_core::Hash256;
use astra_emu_family_api::*;
use std::{
    path::{Component, Path},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
mod eden;
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
pub(crate) struct MusicaProvider {
    next: u64,
    gpu_instance: Option<wgpu::Instance>,
}
pub(crate) fn musica_descriptor() -> FamilyDescriptor {
    let field = |id: &str, label: &str, default: &str| ConfigField {
        id: id.into(),
        label: label.into(),
        group: "Files".into(),
        kind: ConfigKind::String { max_bytes: 256 },
        default: ConfigValue::String(default.into()),
    };
    let render_extent = |id: &str, label: &str, default: i64| ConfigField {
        id: id.into(),
        label: label.into(),
        group: "Presentation".into(),
        kind: ConfigKind::Integer {
            min: 1,
            max: i64::from(u32::MAX),
        },
        default: ConfigValue::Integer(default),
    };
    let mut configuration = vec![
        field(
            "profile_file",
            "Private PAZ key.toml (absolute or game-relative path)",
            MUSICA_KEY_FILE,
        ),
        field("entry_script", "Entry script in scr archive", "test.sc"),
        field(
            "eden_import_file",
            "Native eden save to import (read only)",
            "",
        ),
    ];
    configuration.push(eden::edition_field());
    configuration.push(ConfigField {
        id: "launch_mode".into(),
        label: "Launch mode".into(),
        group: "Playback".into(),
        kind: ConfigKind::Enum {
            choices: vec!["direct".into(), "title".into()].into(),
        },
        default: ConfigValue::Enum("title".into()),
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
    configuration.push(render_extent("render_width", "GPU render width", 1280));
    configuration.push(render_extent("render_height", "GPU render height", 720));
    configuration.push(render_extent("logical_width", "Game logical width", 1280));
    configuration.push(render_extent("logical_height", "Game logical height", 720));
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
    for field in &mut configuration {
        if !matches!(
            field.id.as_str(),
            "profile_file"
                | "entry_script"
                | "launch_mode"
                | "script_encoding"
                | "eden_import_file"
                | "eden_import_edition"
        ) {
            field.group = format!("Initial {} defaults", field.group).into();
        }
    }
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
#[cfg(test)]
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
    #[cfg(test)]
    pub(crate) fn with_gpu_instance(instance: wgpu::Instance) -> Self {
        Self {
            gpu_instance: Some(instance),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn open_session(
        &mut self,
        request: OpenRequest,
    ) -> FamilyResult<(OpenResponse, MusicaSession)> {
        self.open_native_session(request, None)
    }

    pub(crate) fn open_native_session(
        &mut self,
        request: OpenRequest,
        replacement: Option<crate::native::text::TextBridge>,
    ) -> FamilyResult<(OpenResponse, MusicaSession)> {
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let descriptor = musica_descriptor();
        descriptor.validate()?;
        request.validate()?;
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
                ));
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
                ));
            }
        };
        let render_extent = |key: &str| -> FamilyResult<u32> {
            let value = config
                .iter()
                .find(|entry| entry.id == key)
                .map(|entry| &entry.value)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_RENDER_DIMENSIONS",
                        "render dimension is missing",
                    )
                })?;
            let ConfigValue::Integer(value) = value else {
                return Err(error(
                    "ASTRA_EMU_MUSICA_RENDER_DIMENSIONS",
                    "render dimension must be a positive integer",
                ));
            };
            u32::try_from(*value).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RENDER_DIMENSIONS",
                    "render dimension is outside the supported integer range",
                )
            })
        };
        let raster_width = render_extent("render_width")?;
        let raster_height = render_extent("render_height")?;
        let logical_width = render_extent("logical_width")?;
        let logical_height = render_extent("logical_height")?;
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
                ));
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
        let visible = request.initial_window.visible;
        let lease = SessionLease::acquire()?;
        let root = Path::new(request.game_path.as_str());
        let (archive, texture_overrides) =
            mount_musica_with_texture_overrides(root, Path::new(&profile)).map_err(core_error)?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let archive = Arc::new(archive);
        let primary_encoding = encoding;
        let imported =
            eden::read_import(root, &get("eden_import_file")?, &config, primary_encoding)?;
        if imported.is_some() && [logical_width, logical_height] != [1024, 640] {
            return Err(error(
                "ASTRA_EMU_EDEN_SAVE_CANVAS",
                "eden native saves require a 1024 by 640 logical canvas",
            ));
        }
        if imported.is_some() && title_launch {
            return Err(error(
                "ASTRA_EMU_EDEN_SAVE_LAUNCH",
                "native eden import requires direct launch",
            ));
        }
        let script_name = imported
            .as_ref()
            .map(|save| {
                save.variable("script_Filename")
                    .expect("read_import validated the checkpoint")
            })
            .unwrap_or(&entry);
        let uri = format!("musica:/scr/{script_name}");
        let loaded = crate::script_loader::load_script(&archive, &uri, primary_encoding)?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
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
        let mut settings = vm.config().clone();
        [
            settings.bgm_volume,
            settings.voice_volume,
            settings.se_volume,
        ] = audio_preferences.volume;
        [settings.bgm_muted, settings.voice_muted, settings.se_muted] = audio_preferences.muted;
        settings.progress_in_background = progress_in_background;
        settings.text_shadow = text_shadow;
        let settings = storage.configuration(game)?.unwrap_or(settings);
        let progress_in_background = settings.progress_in_background;
        let text_shadow = settings.text_shadow;
        let audio_preferences = settings.audio_preferences();
        vm.set_config(settings)
            .map_err(|_| error("ASTRA_EMU_MUSICA_CONFIG", "native configuration is invalid"))?;
        let quick_cursor = storage.quick_cursor(game)?;
        let restored_message = imported
            .as_ref()
            .map(|save| eden::restore_vm(&mut vm, save, &archive, primary_encoding))
            .transpose()?;
        vm.merge_verified_gallery_unlocks(&storage.progress(game)?)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_GLOBAL_PROGRESS",
                    "global progress cannot be applied",
                )
            })?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        if self.gpu_instance.is_none() {
            self.gpu_instance = Some(astra_platform_common::native_wgpu_instance().map_err(
                |cause| {
                    error(
                        "ASTRA_EMU_MUSICA_RENDERER",
                        &format!(
                            "GPU instance creation failed ({:?} during {})",
                            cause.code, cause.operation
                        ),
                    )
                },
            )?);
        }
        let mut scene = Scene::new_scaled(
            self.gpu_instance
                .as_ref()
                .expect("GPU instance initialized"),
            archive.clone(),
            logical_width,
            logical_height,
            raster_width,
            raster_height,
            texture_overrides,
            encoding,
        )?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        scene.set_text_shadow(text_shadow);
        if title_launch {
            vm.begin_title_launch()
                .map_err(|_| error("ASTRA_EMU_MUSICA_TITLE", "title session cannot start"))?;
            scene.render_title(vm.title_variant(), None)?;
        } else {
            scene.render(vm.state(), restored_message.as_ref(), None)?;
        }
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let audio = Audio::start(archive.clone(), request.host.audio_sink.into_option())?;
        audio.set_preferences(audio_preferences)?;
        audio.suspend(!visible || (!focused && !progress_in_background))?;
        if imported.is_some() {
            eden::restore_audio(&audio, vm.state())?;
        }
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_SESSION_ID", "session counter overflowed"))?;
        let id = format!("musica.{}", self.next);
        let info = FrameInfo {
            width: raster_width,
            height: raster_height,
            logical_width,
            logical_height,
            stride: raster_width
                .checked_mul(4)
                .ok_or_else(|| error("ASTRA_EMU_MUSICA_FRAME_SIZE", "frame stride overflows"))?,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Ignored,
            },
            rows: FrameRowOrder::TopToBottom,
        };
        let response = OpenResponse {
            session_id: id.clone().into(),
            frame: info,
            audio_format: ROption::RSome(crate::audio::FORMAT),
        };
        response.validate()?;
        let mut session = MusicaSession::new(
            id,
            info,
            FramePublisher::new(request.session_epoch)?,
            archive,
            vm,
            scene,
            audio,
            replacement,
            storage,
            game,
            lease,
            focused,
            visible,
            progress_in_background,
            primary_encoding,
            quick_cursor,
        );
        if let Some((text, speaker)) = restored_message {
            session.install_imported_message(text, speaker)?;
        }
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
