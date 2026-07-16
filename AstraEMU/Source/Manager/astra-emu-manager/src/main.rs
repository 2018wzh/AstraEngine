#[cfg(target_os = "android")]
mod android_platform;
#[cfg(target_os = "android")]
mod android_source;
mod audio_executor;
mod platform_secret;
mod platform_source;
mod stage_renderer;
mod translation_runtime;
#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
mod unsupported_source;
mod video_executor;

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    env,
    io::Cursor,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use astra_core::{Hash256, SchemaVersion};
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAwaitResult, LegacyEffect, LegacyEphemeralText, LegacyInputEdge,
    LegacyProbeRequest, LegacyRenderFrameV1, LegacyRuntimeHostCtx, LegacyStepBudget,
    LegacyVfsReader, LegacyVideoCommandV1, LegacyWaitRequest,
};
use astra_emu_manager::family_host::FamilyHostConfig;
use astra_emu_manager::{run_manager_with_initial_state, ManagerController};
use astra_emu_manager_core::CoverCacheRecord;
use astra_emu_manager_core::{
    AstraEmuRuntimeProvider, CancellationToken, CaseRuntimeProfileRecord, EmuCaseProfile,
    EmuStepPayload, GrantedSourceReader, Library, LibraryScanner, PatchContext, PatchDiagnostic,
    PatchHostAction, PatchVfsReader, ScanLimits, SourceGrant, TranslationCacheRecord,
    TranslationConsent, TranslationProfileRecord, TrustedPatchRuntime,
};
use astra_emu_manager_ui_slint::{GameCardViewModel, ManagerViewModel};
use astra_emu_translation_openai_compatible::{
    SecretResolver, TranslationEndpointKind, TranslationProfile, TranslationProtocol,
};
use astra_plugin::ProductRuntimeProvider;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProviderInstanceId, RuntimeOpenRequest, RuntimeOutputDomain,
    RuntimeSectionCodec, RuntimeSectionPayload, RuntimeStepInput, RuntimeStepMode,
};
use image::GenericImageView;
use platform_secret::ManagerSecretStore;
use platform_source::{GrantedSource, VfsRegistry};
use stage_renderer::ManagerStageRenderer;
use translation_runtime::{
    translation_profile_from_record, TranslationLaunchConfig, TranslationOverlayState,
    TranslationRuntime,
};
use video_executor::{HostVideoExecutor, HostVideoFrame};

#[cfg(not(target_os = "android"))]
fn platform_data_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("dev", "AstraEngine", "AstraEMU")
        .map(|directories| directories.data_dir().to_path_buf())
        .ok_or_else(|| "ASTRA_EMU_PLATFORM_DATA_DIRECTORY_UNAVAILABLE".into())
}

#[cfg(target_os = "android")]
fn platform_data_dir() -> Result<PathBuf, String> {
    android_platform::package_identity().map(|identity| PathBuf::from(identity.data_directory))
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn platform_grant_kind() -> &'static str {
    "desktop-directory-v1"
}

#[cfg(target_os = "android")]
fn platform_grant_kind() -> &'static str {
    "android-saf-tree-v1"
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
fn platform_grant_kind() -> &'static str {
    "platform-source-unavailable-v1"
}

struct ActiveRuntimeSession {
    session_id: GameRuntimeSessionId,
    fixed_step: u64,
    fixed_delta_ns: u64,
    seed: u64,
    pending_waits: BTreeMap<String, PendingWait>,
    await_sequence: u64,
    input_sequence: u64,
    pending_inputs: Vec<LegacyInputEdge>,
    next_tick: Instant,
}

enum PendingWait {
    DueStep(u64),
    Input(u64),
    PresentationFence,
    MediaFence(String),
    ProviderCompletion,
    FamilyOpaque,
}

struct RuntimeBridge {
    provider: AstraEmuRuntimeProvider,
    active: Option<ActiveRuntimeSession>,
    terminal: bool,
    render_frames: VecDeque<LegacyRenderFrameV1>,
    audio: Option<HostAudioExecutor>,
    video: HostVideoExecutor,
    text_captures: VecDeque<LegacyEphemeralText>,
    translation: Option<TranslationRuntime>,
    text_hooks: BTreeMap<String, String>,
    media_hooks: BTreeMap<String, String>,
    filter_preset: String,
    suspended: bool,
}

impl RuntimeBridge {
    fn new(vfs: Arc<VfsRegistry>) -> Result<Self, String> {
        let family = FamilyHostConfig::from_process()?.create_provider(vfs.clone())?;
        let mut provider = AstraEmuRuntimeProvider::new(family)?;
        provider.create_instance(ProviderInstanceId("astra.emu.manager.instance".into()))?;
        Ok(Self {
            provider,
            active: None,
            terminal: false,
            render_frames: VecDeque::new(),
            audio: None,
            video: HostVideoExecutor::default(),
            text_captures: VecDeque::new(),
            translation: None,
            text_hooks: BTreeMap::new(),
            media_hooks: BTreeMap::new(),
            filter_preset: "none".into(),
            suspended: false,
        })
    }

    fn launch(
        &mut self,
        case: &astra_emu_manager_core::CaseRecord,
        profile: CaseRuntimeProfileRecord,
        mount_set_id: String,
        translation_config: TranslationLaunchConfig,
        patch_actions: Vec<PatchHostAction>,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_RUNTIME_SESSION_ALREADY_ACTIVE".into());
        }
        if profile.family_id != "fvp" || profile.case_identity != case.case_identity {
            return Err("ASTRA_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        let (text_hooks, media_hooks, deterministic_effects) =
            validate_patch_actions(patch_actions)?;
        let case_fingerprint: Hash256 = case
            .content_hash
            .parse()
            .map_err(|_| "ASTRA_EMU_CASE_FINGERPRINT_INVALID".to_owned())?;
        let emu_profile = EmuCaseProfile {
            schema: "astra.emu.case_profile.v1".into(),
            family_id: "fvp".into(),
            case_fingerprint,
            script_uri: case.relative_path.clone(),
            fixed_delta_ns: profile.fixed_delta_ns,
            compatibility_profile: profile.compatibility_profile,
            mount_set_id: mount_set_id.clone(),
            permission_policy_id: "astra.emu.desktop.user_grant.v1".into(),
            family_options: profile.family_options,
        };
        let bytes = postcard::to_allocvec(&emu_profile).map_err(|error| error.to_string())?;
        let section = RuntimeSectionPayload {
            section_id: "emu.case_profile".into(),
            schema: "astra.emu.case_profile.v1".into(),
            version: SchemaVersion::new(1, 0, 0),
            codec: RuntimeSectionCodec::Postcard,
            hash: Hash256::from_sha256(&bytes),
            bytes,
        };
        let seed = u64::from_le_bytes(case_fingerprint.as_bytes()[..8].try_into().unwrap());
        let audio = HostAudioExecutor::open()?;
        let translation = TranslationRuntime::open(
            translation_config,
            Arc::new(ManagerSecretStore::open().map_err(|error| error.to_string())?),
        )?;
        let open = self.provider.open(RuntimeOpenRequest {
            target_id: "astra-emu-case".into(),
            profile: "fvp-v1".into(),
            locale: "und".into(),
            seed,
            package_hash: case.content_hash.clone(),
            sections: vec![section],
        })?;
        for effect in deterministic_effects {
            if let Err(error) = self.provider.queue_patch_effect(&open.session_id, effect) {
                let cleanup = self.provider.shutdown(open.session_id.clone());
                return match cleanup {
                    Ok(_) => Err(error),
                    Err(cleanup_error) => Err(format!(
                        "ASTRA_EMU_PATCH_QUEUE_AND_CLEANUP_FAILED:{error};{cleanup_error}"
                    )),
                };
            }
        }
        self.active = Some(ActiveRuntimeSession {
            session_id: open.session_id,
            fixed_step: 0,
            fixed_delta_ns: emu_profile.fixed_delta_ns,
            seed,
            pending_waits: BTreeMap::new(),
            await_sequence: 0,
            input_sequence: 0,
            pending_inputs: Vec::new(),
            next_tick: Instant::now(),
        });
        self.audio = Some(audio);
        self.translation = Some(translation);
        self.text_hooks = text_hooks;
        self.media_hooks = media_hooks;
        self.terminal = false;
        self.render_frames.clear();
        self.text_captures.clear();
        Ok(())
    }

    fn probe_fvp_profile(
        &self,
        case: &astra_emu_manager_core::CaseRecord,
        mount_set_id: &str,
    ) -> Result<CaseRuntimeProfileRecord, String> {
        let package_hash: Hash256 = case
            .content_hash
            .parse()
            .map_err(|_| "ASTRA_EMU_CASE_FINGERPRINT_INVALID".to_owned())?;
        let report = self.provider.probe_family(
            &LegacyRuntimeHostCtx {
                case_id: case.case_identity.clone(),
                package_id: "astra-emu-case".into(),
                package_hash,
                mount_set_id: mount_set_id.into(),
                media_service_ids: vec!["astra.media.host".into()],
                permission_policy_id: "astra.emu.desktop.user_grant.v1".into(),
                report_sink_id: "astra.emu.manager.report".into(),
                target: "game".into(),
                profile: "fvp-v1".into(),
            },
            LegacyProbeRequest {
                root_mount_id: mount_set_id.into(),
                candidate_uris: vec![case.relative_path.clone()],
                marker_hashes: vec![package_hash],
                max_entries: 1,
                max_metadata_bytes: 512 * 1024 * 1024,
            },
        )?;
        if report.confidence_permyriad != 10_000 || !report.blockers.is_empty() {
            return Err("ASTRA_EMU_FVP_PROBE_BLOCKED".into());
        }
        let nls = report
            .markers
            .iter()
            .filter_map(|marker| marker.strip_prefix("fvp.nls."))
            .collect::<Vec<_>>();
        if nls.len() != 1 || !matches!(nls[0], "shift_jis" | "gbk" | "utf8") {
            return Err("ASTRA_EMU_FVP_PROBE_NLS_AMBIGUOUS".into());
        }
        let marker_number = |prefix: &str| -> Result<u32, String> {
            let values = report
                .markers
                .iter()
                .filter_map(|marker| marker.strip_prefix(prefix))
                .collect::<Vec<_>>();
            if values.len() != 1 {
                return Err("ASTRA_EMU_FVP_PROBE_STAGE_AMBIGUOUS".into());
            }
            values[0]
                .parse()
                .map_err(|_| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID".into())
        };
        let stage_width = marker_number("fvp.stage_width.")?;
        let stage_height = marker_number("fvp.stage_height.")?;
        Ok(CaseRuntimeProfileRecord {
            case_identity: case.case_identity.clone(),
            family_id: "fvp".into(),
            fixed_delta_ns: 16_666_667,
            compatibility_profile: "rfvp-v1".into(),
            family_options: [
                ("fvp.nls".into(), nls[0].into()),
                ("fvp.stage_width".into(), stage_width.to_string()),
                ("fvp.stage_height".into(), stage_height.to_string()),
                ("patch.mode".into(), "no_patch".into()),
            ]
            .into_iter()
            .collect(),
        })
    }

    fn step_if_due(&mut self) -> Result<bool, String> {
        if self.suspended {
            return Ok(false);
        }
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_RUNTIME_SESSION_NOT_ACTIVE".to_owned())?;
        if self.terminal {
            return Ok(false);
        }
        if Instant::now() < active.next_tick {
            if let Some(audio) = self.audio.as_mut() {
                audio.pump()?;
            }
            return Ok(false);
        }
        let next_step = active.fixed_step.saturating_add(1);
        let completed_media = self.video.take_completed();
        for media_id in completed_media {
            let mut matched = false;
            for wait in active.pending_waits.values_mut() {
                if matches!(wait, PendingWait::MediaFence(wait_media_id) if *wait_media_id == media_id)
                {
                    *wait = PendingWait::DueStep(next_step);
                    matched = true;
                }
            }
            if !matched {
                return Err("ASTRA_EMU_VIDEO_COMPLETION_UNSOLICITED".into());
            }
        }
        let input_mask = active
            .pending_inputs
            .iter()
            .fold(0_u64, |mask, edge| mask | input_control_mask(&edge.control));
        let ready = active
            .pending_waits
            .iter()
            .filter(|(_, wait)| match wait {
                PendingWait::DueStep(due) => *due <= next_step,
                PendingWait::Input(mask) => input_mask & *mask != 0,
                PendingWait::PresentationFence
                | PendingWait::MediaFence(_)
                | PendingWait::ProviderCompletion
                | PendingWait::FamilyOpaque => false,
            })
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        let mut await_results = Vec::new();
        for token_id in ready {
            active.pending_waits.remove(&token_id);
            active.await_sequence = active.await_sequence.saturating_add(1);
            await_results.push(LegacyAwaitResult {
                token_id,
                status: "completed".into(),
                payload_hash: Hash256::from_sha256(&[]),
                sequence: active.await_sequence,
            });
        }
        let output = self.provider.step(RuntimeStepInput {
            session_id: active.session_id.clone(),
            fixed_step: next_step,
            delta_ns: active.fixed_delta_ns,
            session_seed: active.seed,
            mode: RuntimeStepMode::Live,
            action: "emu.step".into(),
            payload: serde_json::to_value(EmuStepPayload {
                input_edges: std::mem::take(&mut active.pending_inputs),
                await_results,
                provider_results: Vec::new(),
                budget: LegacyStepBudget {
                    max_instructions: 100_000,
                    max_effects: 4096,
                    max_trace_entries: 65_536,
                },
            })
            .map_err(|error| error.to_string())?,
        })?;
        active.fixed_step = next_step;
        active.next_tick += Duration::from_nanos(active.fixed_delta_ns);
        let mut audio_commands = Vec::new();
        let mut video_commands = Vec::new();
        for envelope in &output.outputs {
            if envelope.domain != RuntimeOutputDomain::Effect
                || envelope.schema != "astra.emu.legacy_step_output.v1"
            {
                continue;
            }
            let family_output = envelope
                .decode_postcard::<astra_emu_family_api::LegacyStepOutput>(
                    RuntimeOutputDomain::Effect,
                    "astra.emu.legacy_step_output.v1",
                    SchemaVersion::new(1, 0, 0),
                )
                .map_err(|error| error.to_string())?;
            for effect in &family_output.effects {
                match effect {
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.render_frame.v1" => {
                        let frame = postcard::from_bytes::<LegacyRenderFrameV1>(payload)
                            .map_err(|_| "ASTRA_EMU_RENDER_FRAME_DECODE".to_owned())?;
                        frame.validate().map_err(|error| error.to_string())?;
                        if self.render_frames.len() >= 3 {
                            return Err("ASTRA_EMU_RENDER_FRAME_QUEUE_OVERFLOW".into());
                        }
                        self.render_frames.push_back(frame);
                    }
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.video_command.v1" => {
                        let command = postcard::from_bytes::<LegacyVideoCommandV1>(payload)
                            .map_err(|_| "ASTRA_EMU_VIDEO_COMMAND_DECODE".to_owned())?;
                        command.validate().map_err(|error| error.to_string())?;
                        video_commands.push(command);
                    }
                    LegacyEffect::Audio {
                        command, payload, ..
                    } if command == "astra.emu.audio_command.v1" => {
                        let command = postcard::from_bytes::<LegacyAudioCommandV1>(payload)
                            .map_err(|_| "ASTRA_EMU_AUDIO_COMMAND_DECODE".to_owned())?;
                        command.validate().map_err(|error| error.to_string())?;
                        audio_commands.push(command);
                    }
                    LegacyEffect::Audio { .. } => {
                        return Err("ASTRA_EMU_AUDIO_COMMAND_UNSUPPORTED".into());
                    }
                    LegacyEffect::TextCapture {
                        lease_id,
                        text_hash,
                        byte_len,
                        speaker_hash,
                        ..
                    } => {
                        let mut text = self
                            .provider
                            .take_ephemeral_text(&active.session_id, lease_id)?
                            .ok_or_else(|| "ASTRA_EMU_TEXT_LEASE_MISSING".to_owned())?;
                        if text.lease_id != *lease_id
                            || text.text.len() != *byte_len as usize
                            || Hash256::from_sha256(text.text.as_bytes()) != *text_hash
                            || text
                                .speaker
                                .as_ref()
                                .map(|speaker| Hash256::from_sha256(speaker.as_bytes()))
                                != *speaker_hash
                        {
                            return Err("ASTRA_EMU_TEXT_LEASE_IDENTITY".into());
                        }
                        let hook_key = Hash256::from_sha256(text.text.as_bytes()).to_string();
                        if let Some(replacement) = self
                            .text_hooks
                            .get(&hook_key)
                            .or_else(|| self.text_hooks.get("all"))
                        {
                            text.text.clone_from(replacement);
                        }
                        if self.text_captures.len() >= 256 {
                            self.text_captures.pop_front();
                        }
                        self.text_captures.push_back(text);
                        self.translation
                            .as_mut()
                            .ok_or_else(|| "ASTRA_EMU_TRANSLATION_RUNTIME_MISSING".to_owned())?
                            .capture(
                                self.text_captures
                                    .back()
                                    .ok_or_else(|| "ASTRA_EMU_TEXT_CAPTURE_STATE".to_owned())?
                                    .text
                                    .clone(),
                            )?;
                    }
                    _ => {}
                }
            }
            for wait in family_output.waits {
                let (token, condition) = wait_condition(&wait, next_step, active.fixed_delta_ns);
                if active.pending_waits.insert(token, condition).is_some() {
                    return Err("ASTRA_EMU_AWAIT_TOKEN_DUPLICATE".into());
                }
            }
        }
        let audio = self
            .audio
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_EXECUTOR_MISSING".to_owned())?;
        for mut command in audio_commands {
            apply_audio_media_hook(&mut command, &self.media_hooks)?;
            let resource = match &command {
                LegacyAudioCommandV1::LoadResource { resource_uri, .. } => {
                    Some(self.provider.read_session_resource(
                        &active.session_id,
                        resource_uri,
                        audio_executor::MAX_RESOURCE_BYTES,
                    )?)
                }
                _ => None,
            };
            audio.execute(command, resource)?;
        }
        for mut command in video_commands {
            apply_video_media_hook(&mut command, &self.media_hooks)?;
            let resource = match &command {
                LegacyVideoCommandV1::Play { resource_uri, .. } => {
                    Some(self.provider.read_session_resource(
                        &active.session_id,
                        resource_uri,
                        video_executor::MAX_ENCODED_BYTES,
                    )?)
                }
                LegacyVideoCommandV1::Stop { .. } => None,
            };
            self.video.execute(command, resource, audio)?;
        }
        self.video.advance(active.fixed_delta_ns, audio)?;
        audio.pump()?;
        self.translation
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_TRANSLATION_RUNTIME_MISSING".to_owned())?
            .poll()?;
        self.terminal = output.status == "terminal";
        Ok(true)
    }

    fn shutdown(&mut self) -> Result<(), String> {
        let active = self
            .active
            .take()
            .ok_or_else(|| "ASTRA_EMU_RUNTIME_SESSION_NOT_ACTIVE".to_owned())?;
        self.provider.shutdown(active.session_id)?;
        if let Some(mut audio) = self.audio.take() {
            self.video.reset(&mut audio)?;
            audio.reset()?;
        }
        self.terminal = false;
        self.render_frames.clear();
        self.text_captures.clear();
        self.translation = None;
        self.text_hooks.clear();
        self.media_hooks.clear();
        self.suspended = false;
        Ok(())
    }

    #[cfg(target_os = "android")]
    fn set_suspended(&mut self, suspended: bool) -> Result<(), String> {
        if self.active.is_none() || self.suspended == suspended {
            return Ok(());
        }
        if let Some(audio) = self.audio.as_ref() {
            audio.set_suspended(suspended)?;
        }
        self.suspended = suspended;
        if !suspended {
            let active = self
                .active
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_RUNTIME_SESSION_NOT_ACTIVE".to_owned())?;
            active.next_tick = Instant::now() + Duration::from_nanos(active.fixed_delta_ns);
        }
        Ok(())
    }

    fn queue_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String> {
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_RUNTIME_SESSION_NOT_ACTIVE".to_owned())?;
        if active.pending_inputs.len() >= 4096 {
            return Err("ASTRA_EMU_INPUT_QUEUE_BOUNDS".into());
        }
        if !matches!(
            control,
            "confirm"
                | "cancel"
                | "up"
                | "down"
                | "left"
                | "right"
                | "space"
                | "pointer.x"
                | "pointer.y"
                | "pointer.primary"
                | "pointer.secondary"
                | "wheel"
        ) || !value.is_finite()
        {
            return Err("ASTRA_EMU_INPUT_INVALID".into());
        }
        active.input_sequence = active.input_sequence.saturating_add(1);
        let edge = LegacyInputEdge {
            control: control.to_owned(),
            pressed,
            value,
            sequence: active.input_sequence,
        };
        active.pending_inputs.push(edge);
        Ok(())
    }

    fn take_latest_render_frame(&mut self) -> Option<LegacyRenderFrameV1> {
        self.render_frames.pop_front()
    }

    fn current_video_frame(&self) -> Option<HostVideoFrame> {
        self.video.current_frame()
    }

    fn acknowledge_presentation(&mut self) {
        if let Some(active) = self.active.as_mut() {
            for wait in active.pending_waits.values_mut() {
                if matches!(wait, PendingWait::PresentationFence) {
                    *wait = PendingWait::DueStep(active.fixed_step.saturating_add(1));
                }
            }
        }
    }

    fn translation_overlay(&self) -> Option<TranslationOverlayState> {
        self.translation
            .as_ref()
            .map(|translation| translation.overlay().clone())
    }

    fn take_translation_writes(&mut self) -> Vec<TranslationCacheRecord> {
        self.translation
            .as_mut()
            .map(TranslationRuntime::take_pending_writes)
            .unwrap_or_default()
    }

    fn reset_translation(&mut self) -> Result<(), String> {
        self.translation
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_TRANSLATION_RUNTIME_NOT_ACTIVE".to_owned())?
            .reset_circuit()
    }

    fn set_filter_preset(&mut self, preset_id: &str) -> Result<(), String> {
        if !matches!(preset_id, "none" | "grayscale" | "crt-soft" | "warm") {
            return Err("ASTRA_EMU_FILTER_PRESET_UNSUPPORTED".into());
        }
        self.filter_preset = preset_id.to_owned();
        Ok(())
    }

    fn filter_preset(&self) -> &str {
        &self.filter_preset
    }

    fn diagnostics_summary(&self) -> String {
        let state = if self.active.is_some() {
            if self.terminal {
                "terminal"
            } else {
                "active"
            }
        } else {
            "idle"
        };
        format!(
            "runtime={state}; pending_frames={}; video_active={}; translation_active={}; filter={}",
            self.render_frames.len(),
            self.video.is_active(),
            self.translation.is_some(),
            self.filter_preset
        )
    }
}

impl Drop for RuntimeBridge {
    fn drop(&mut self) {
        if let Some(active) = self.active.take() {
            if self.provider.shutdown(active.session_id).is_err() {
                tracing::error!(
                    event = "astra.emu.runtime.drop_shutdown_failed",
                    diagnostic_code = "ASTRA_EMU_RUNTIME_DROP_SHUTDOWN_FAILED"
                );
            }
        }
    }
}

type PatchBindings = (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    Vec<LegacyEffect>,
);

fn validate_patch_actions(actions: Vec<PatchHostAction>) -> Result<PatchBindings, String> {
    let mut text_hooks = BTreeMap::new();
    let mut media_hooks = BTreeMap::new();
    let mut effects = Vec::new();
    for action in actions {
        match action {
            PatchHostAction::DecodeTransform { .. } => {
                return Err("ASTRA_EMU_PATCH_DECODE_TRANSFORM_NOT_INSTALLED".into());
            }
            PatchHostAction::TextHook {
                target_hash,
                replacement,
            } => {
                if text_hooks.insert(target_hash, replacement).is_some() {
                    return Err("ASTRA_EMU_PATCH_TEXT_HOOK_DUPLICATE".into());
                }
            }
            PatchHostAction::MediaHook {
                target_hash,
                replacement_uri,
            } => {
                if media_hooks.insert(target_hash, replacement_uri).is_some() {
                    return Err("ASTRA_EMU_PATCH_MEDIA_HOOK_DUPLICATE".into());
                }
            }
            PatchHostAction::DeterministicEffect { target, payload } => {
                let effect = if target.starts_with("event.") {
                    LegacyEffect::RuntimeEvent {
                        sequence: 0,
                        event: target,
                        payload,
                    }
                } else if target.starts_with("blackboard.") {
                    LegacyEffect::SetBlackboard {
                        sequence: 0,
                        key: target,
                        value: payload,
                    }
                } else {
                    return Err("ASTRA_EMU_PATCH_EFFECT_TARGET".into());
                };
                effects.push(effect);
            }
        }
    }
    Ok((text_hooks, media_hooks, effects))
}

fn apply_audio_media_hook(
    command: &mut LegacyAudioCommandV1,
    hooks: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let LegacyAudioCommandV1::LoadResource { resource_uri, .. } = command {
        if let Some(replacement) =
            hooks.get(&Hash256::from_sha256(resource_uri.as_bytes()).to_string())
        {
            resource_uri.clone_from(replacement);
        }
    }
    command.validate().map_err(|error| error.to_string())
}

fn apply_video_media_hook(
    command: &mut LegacyVideoCommandV1,
    hooks: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let LegacyVideoCommandV1::Play { resource_uri, .. } = command {
        if let Some(replacement) =
            hooks.get(&Hash256::from_sha256(resource_uri.as_bytes()).to_string())
        {
            resource_uri.clone_from(replacement);
        }
    }
    command.validate().map_err(|error| error.to_string())
}

fn wait_condition(wait: &LegacyWaitRequest, step: u64, delta_ns: u64) -> (String, PendingWait) {
    match wait {
        LegacyWaitRequest::Time {
            token_id,
            milliseconds,
        } => {
            let delay_ns = u64::from(*milliseconds).saturating_mul(1_000_000);
            let ticks = delay_ns.div_ceil(delta_ns).max(1);
            (
                token_id.clone(),
                PendingWait::DueStep(step.saturating_add(ticks)),
            )
        }
        LegacyWaitRequest::Frame { token_id, frames } => (
            token_id.clone(),
            PendingWait::DueStep(step.saturating_add(u64::from(*frames).max(1))),
        ),
        LegacyWaitRequest::Input { token_id, mask } => {
            (token_id.clone(), PendingWait::Input(*mask))
        }
        LegacyWaitRequest::MediaFence { token_id, media_id } => {
            (token_id.clone(), PendingWait::MediaFence(media_id.clone()))
        }
        LegacyWaitRequest::PresentationFence { token_id, .. } => {
            (token_id.clone(), PendingWait::PresentationFence)
        }
        LegacyWaitRequest::ProviderCompletion { token_id, .. } => {
            (token_id.clone(), PendingWait::ProviderCompletion)
        }
        LegacyWaitRequest::FamilyOpaque { token_id, .. } => {
            (token_id.clone(), PendingWait::FamilyOpaque)
        }
    }
}

fn input_control_mask(control: &str) -> u64 {
    match control {
        "confirm" => 1 << 0,
        "cancel" => 1 << 1,
        "up" => 1 << 2,
        "down" => 1 << 3,
        "left" => 1 << 4,
        "right" => 1 << 5,
        "space" => 1 << 6,
        "pointer.primary" => 1 << 7,
        "pointer.secondary" => 1 << 8,
        "wheel" => 1 << 9,
        _ => 0,
    }
}

fn parse_glossary(input: &str) -> Result<Vec<(String, String)>, String> {
    let mut entries = Vec::new();
    let mut sources = std::collections::BTreeSet::new();
    for (line_index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if entries.len() >= 1024 {
            return Err("ASTRA_EMU_TRANSLATION_GLOSSARY_LIMIT".into());
        }
        let (source, target) = line.split_once('=').ok_or_else(|| {
            format!(
                "ASTRA_EMU_TRANSLATION_GLOSSARY_SYNTAX_LINE_{}",
                line_index + 1
            )
        })?;
        let source = source.trim();
        let target = target.trim();
        if source.is_empty()
            || target.is_empty()
            || source.len() > 512
            || target.len() > 512
            || !sources.insert(source.to_owned())
        {
            return Err(format!(
                "ASTRA_EMU_TRANSLATION_GLOSSARY_INVALID_LINE_{}",
                line_index + 1
            ));
        }
        entries.push((source.to_owned(), target.to_owned()));
    }
    Ok(entries)
}

struct AstraEmuManagerController {
    library: Library,
    selected_case_id: Option<String>,
    search_query: String,
    diagnostic: String,
    vfs: Arc<VfsRegistry>,
    runtime: Rc<RefCell<RuntimeBridge>>,
    active_mount_set_id: Option<String>,
    data_dir: PathBuf,
    patch_summary: String,
    pending_patch_actions: Vec<PatchHostAction>,
}

struct MountedPatchReader {
    vfs: Arc<VfsRegistry>,
    mount_set_id: String,
}

impl PatchVfsReader for MountedPatchReader {
    fn read(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, PatchDiagnostic> {
        self.vfs
            .read_file(&self.mount_set_id, path, max_bytes as u64)
            .map_err(|error| PatchDiagnostic {
                code: error.code().into(),
                message: "trusted patch VFS read failed".into(),
            })
    }
}

impl AstraEmuManagerController {
    fn open() -> Result<Self, String> {
        let data_dir = platform_data_dir()?;
        std::fs::create_dir_all(&data_dir)
            .map_err(|_| "ASTRA_EMU_PLATFORM_DATA_DIRECTORY_CREATE".to_owned())?;
        let library =
            Library::open(data_dir.join("library.sqlite3")).map_err(|error| error.to_string())?;
        let vfs = Arc::new(VfsRegistry::default());
        let runtime = Rc::new(RefCell::new(RuntimeBridge::new(vfs.clone())?));
        Ok(Self {
            library,
            selected_case_id: None,
            search_query: String::new(),
            diagnostic: String::new(),
            vfs,
            runtime,
            active_mount_set_id: None,
            data_dir,
            patch_summary: "Explicit no-patch mode is active.".into(),
            pending_patch_actions: Vec::new(),
        })
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    fn apply_quick_launch_from_environment(&mut self) -> Result<bool, String> {
        const ENGINE: &str = "ASTRA_EMU_QUICK_ENGINE";
        const GAME_DIR: &str = "ASTRA_EMU_QUICK_GAME_DIR";
        const ENTRY: &str = "ASTRA_EMU_QUICK_ENTRY";
        let engine = env::var_os(ENGINE);
        let game_dir = env::var_os(GAME_DIR);
        let entry = env::var_os(ENTRY);
        if engine.is_none() && game_dir.is_none() && entry.is_none() {
            return Ok(false);
        }
        let engine = engine
            .and_then(|value| value.into_string().ok())
            .ok_or_else(|| "ASTRA_EMU_QUICK_ENGINE_REQUIRED".to_owned())?;
        if engine != "fvp" {
            return Err("ASTRA_EMU_QUICK_ENGINE_UNSUPPORTED".into());
        }
        let game_dir = game_dir.ok_or_else(|| "ASTRA_EMU_QUICK_GAME_DIR_REQUIRED".to_owned())?;
        let canonical = std::fs::canonicalize(PathBuf::from(game_dir))
            .map_err(|_| "ASTRA_EMU_QUICK_GAME_DIR_INVALID".to_owned())?;
        if !canonical.is_dir() {
            return Err("ASTRA_EMU_QUICK_GAME_DIR_INVALID".into());
        }
        let identity = Hash256::from_sha256(canonical.to_string_lossy().as_bytes()).to_string();
        let grant = SourceGrant {
            source_id: format!("quick-{}", &identity[7..39]),
            alias: "Quick launch".into(),
            platform_token: canonical.to_string_lossy().into_owned(),
            token_kind: platform_grant_kind().into(),
            active: true,
        };
        self.library
            .upsert_grant(&grant)
            .map_err(|error| error.to_string())?;
        self.scan_grant(&grant)?;
        let requested_entry = entry
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| "ASTRA_EMU_QUICK_ENTRY_INVALID".to_owned())
            })
            .transpose()?
            .map(|value| value.replace('\\', "/"));
        if requested_entry.as_ref().is_some_and(|value| {
            value.is_empty()
                || value.starts_with('/')
                || value
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
        }) {
            return Err("ASTRA_EMU_QUICK_ENTRY_INVALID".into());
        }
        let candidates = self
            .library
            .list_cases()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|case| case.source_id == grant.source_id)
            .filter(|case| {
                requested_entry
                    .as_ref()
                    .is_none_or(|entry| case.relative_path.replace('\\', "/") == *entry)
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err("ASTRA_EMU_QUICK_CASE_NOT_FOUND".into());
        }
        if candidates.len() != 1 {
            return Err("ASTRA_EMU_QUICK_CASE_AMBIGUOUS".into());
        }
        ManagerController::launch(self, &candidates[0].case_identity)?;
        Ok(true)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    fn apply_quick_launch_from_environment(&mut self) -> Result<bool, String> {
        Ok(false)
    }

    fn scan_grant(&mut self, grant: &SourceGrant) -> Result<(), String> {
        if grant.token_kind != platform_grant_kind() || !grant.active {
            return Err("ASTRA_EMU_SOURCE_GRANT_INVALID".into());
        }
        let source =
            Arc::new(GrantedSource::new(&grant.platform_token).map_err(|error| error.to_string())?);
        let scanner =
            LibraryScanner::new(ScanLimits::default()).map_err(|error| error.to_string())?;
        scanner
            .scan(
                &mut self.library,
                &grant.source_id,
                source.clone(),
                &CancellationToken::default(),
            )
            .map_err(|error| error.to_string())?;
        refresh_cover_cache(&mut self.library, &grant.source_id, source, &self.data_dir)?;
        Ok(())
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    fn choose_and_add_grant(&mut self) -> Result<(), String> {
        let root = rfd::FileDialog::new()
            .set_title("Authorize a visual novel directory")
            .pick_folder()
            .ok_or_else(|| "ASTRA_EMU_SOURCE_GRANT_CANCELLED".to_owned())?;
        let canonical =
            std::fs::canonicalize(root).map_err(|_| "ASTRA_EMU_SOURCE_GRANT_INVALID".to_owned())?;
        let identity = Hash256::from_sha256(canonical.to_string_lossy().as_bytes()).to_string();
        let grant = SourceGrant {
            source_id: format!("root-{}", &identity[..32]),
            alias: canonical
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Games")
                .to_owned(),
            platform_token: canonical.to_string_lossy().into_owned(),
            token_kind: "desktop-directory-v1".into(),
            active: true,
        };
        self.library
            .upsert_grant(&grant)
            .map_err(|error| error.to_string())?;
        self.scan_grant(&grant)
    }

    #[cfg(target_os = "android")]
    fn choose_and_add_grant(&mut self) -> Result<(), String> {
        android_platform::request_document_tree()
    }

    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "android"
    )))]
    fn choose_and_add_grant(&mut self) -> Result<(), String> {
        Err("PLATFORM_NOT_IMPLEMENTED: source grant provider is unavailable".into())
    }

    #[cfg(target_os = "android")]
    fn accept_android_grant(&mut self, platform_token: String) -> Result<(), String> {
        let identity = Hash256::from_sha256(platform_token.as_bytes()).to_string();
        let grant = SourceGrant {
            source_id: format!("root-{}", &identity[7..39]),
            alias: format!("SAF {}", &identity[7..15]),
            platform_token,
            token_kind: platform_grant_kind().into(),
            active: true,
        };
        self.library
            .upsert_grant(&grant)
            .map_err(|error| error.to_string())?;
        self.scan_grant(&grant)
    }

    fn apply_trusted_patch(
        &mut self,
        profile: &CaseRuntimeProfileRecord,
        mount_set_id: &str,
    ) -> Result<(), String> {
        match profile
            .family_options
            .get("patch.mode")
            .map(String::as_str)
            .unwrap_or("no_patch")
        {
            "no_patch" => {
                self.patch_summary = "Explicit no-patch mode is active.".into();
                self.pending_patch_actions.clear();
                Ok(())
            }
            "trusted" => {
                self.pending_patch_actions.clear();
                let source = self
                    .vfs
                    .read_file(mount_set_id, "astraemu.patch.luau", 256 * 1024)
                    .map_err(|error| format!("ASTRA_EMU_PATCH_SOURCE_READ:{}", error.code()))?;
                let source = std::str::from_utf8(&source)
                    .map_err(|_| "ASTRA_EMU_PATCH_SOURCE_UTF8".to_owned())?;
                let runtime =
                    TrustedPatchRuntime::new(1_000_000).map_err(|diagnostic| diagnostic.code)?;
                let execution = runtime
                    .evaluate(
                        source,
                        &PatchContext {
                            files: BTreeMap::new(),
                            reader: Some(Arc::new(MountedPatchReader {
                                vfs: self.vfs.clone(),
                                mount_set_id: mount_set_id.into(),
                            })),
                        },
                    )
                    .map_err(|diagnostic| diagnostic.code)?;
                let mut overlays = execution.overlays;
                let mut pending_actions = Vec::new();
                for action in execution.host_actions {
                    match action {
                        PatchHostAction::DecodeTransform { path, bytes } => {
                            if overlays.insert(path, bytes).is_some() {
                                return Err("ASTRA_EMU_PATCH_DECODE_OVERLAY_CONFLICT".into());
                            }
                        }
                        action => pending_actions.push(action),
                    }
                }
                let overlay_count = overlays.len();
                let intent_count = execution.intents.len();
                self.vfs.install_overlays(mount_set_id, overlays)?;
                self.pending_patch_actions = pending_actions;
                self.patch_summary = format!(
                    "Trusted patch isolated successfully; intents={intent_count}; overlays={overlay_count}. Reports retain hashes and counts only."
                );
                Ok(())
            }
            _ => Err("ASTRA_EMU_PATCH_MODE_INVALID".into()),
        }
    }
}

fn default_case_profile(case_identity: String) -> CaseRuntimeProfileRecord {
    CaseRuntimeProfileRecord {
        case_identity,
        family_id: "fvp".into(),
        fixed_delta_ns: 16_666_667,
        compatibility_profile: "rfvp-v1".into(),
        family_options: [("patch.mode".into(), "no_patch".into())]
            .into_iter()
            .collect(),
    }
}

const MAX_COVER_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_COVER_DIMENSION: u32 = 8_192;
const COVER_WIDTH: u32 = 512;
const COVER_HEIGHT: u32 = 768;

fn refresh_cover_cache(
    library: &mut Library,
    source_id: &str,
    source: Arc<dyn GrantedSourceReader>,
    data_dir: &std::path::Path,
) -> Result<(), String> {
    let entries = source
        .enumerate(&CancellationToken::default())
        .map_err(|error| error.to_string())?;
    let available = entries
        .into_iter()
        .filter(|entry| entry.is_file && entry.byte_size <= MAX_COVER_SOURCE_BYTES)
        .map(|entry| (entry.relative_path.replace('\\', "/").to_lowercase(), entry))
        .collect::<BTreeMap<_, _>>();
    let cache_root = data_dir.join("covers");
    std::fs::create_dir_all(&cache_root)
        .map_err(|_| "ASTRA_EMU_COVER_CACHE_DIRECTORY_CREATE".to_owned())?;
    for case in library
        .list_cases()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|case| case.source_id == source_id)
    {
        let parent = case
            .relative_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        let mut selected = None;
        'candidate: for stem in ["cover", "title", "package", "icon"] {
            for extension in ["png", "jpg", "jpeg", "webp"] {
                let path = if parent.is_empty() {
                    format!("{stem}.{extension}")
                } else {
                    format!("{parent}/{stem}.{extension}")
                };
                if let Some(entry) = available.get(&path.to_lowercase()) {
                    selected = Some(entry);
                    break 'candidate;
                }
            }
        }
        let Some(entry) = selected else {
            continue;
        };
        let bytes = source
            .read_file(&entry.relative_path, MAX_COVER_SOURCE_BYTES)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 != entry.byte_size {
            return Err("ASTRA_EMU_COVER_SOURCE_CHANGED".into());
        }
        let source_hash = Hash256::from_sha256(&bytes).to_string();
        let existing = library
            .cover_cache(&case.case_identity)
            .map_err(|error| error.to_string())?;
        if existing.as_ref().is_some_and(|cover| {
            cover.source_hash == source_hash && data_dir.join(&cover.cache_relative_path).is_file()
        }) {
            continue;
        }
        let image = image::load_from_memory(&bytes)
            .map_err(|_| "ASTRA_EMU_COVER_IMAGE_DECODE".to_owned())?;
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 || width > MAX_COVER_DIMENSION || height > MAX_COVER_DIMENSION
        {
            return Err("ASTRA_EMU_COVER_IMAGE_BOUNDS".into());
        }
        let thumbnail = if width > COVER_WIDTH || height > COVER_HEIGHT {
            image.thumbnail(COVER_WIDTH, COVER_HEIGHT)
        } else {
            image
        };
        let (cache_width, cache_height) = thumbnail.dimensions();
        let mut encoded = Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut encoded, image::ImageFormat::Png)
            .map_err(|_| "ASTRA_EMU_COVER_IMAGE_ENCODE".to_owned())?;
        let encoded = encoded.into_inner();
        let image_hash = Hash256::from_sha256(&encoded).to_string();
        let file_name = format!("{}-{}.png", case.case_identity, &source_hash[7..23]);
        let relative_path = format!("covers/{file_name}");
        let final_path = cache_root.join(&file_name);
        let temporary_path = cache_root.join(format!("{file_name}.tmp"));
        std::fs::write(&temporary_path, &encoded)
            .map_err(|_| "ASTRA_EMU_COVER_CACHE_WRITE".to_owned())?;
        std::fs::rename(&temporary_path, &final_path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary_path);
            format!("ASTRA_EMU_COVER_CACHE_COMMIT:{:?}", error.kind())
        })?;
        library
            .upsert_cover_cache(&CoverCacheRecord {
                case_identity: case.case_identity,
                source_hash,
                cache_relative_path: relative_path.clone(),
                image_hash,
                width: cache_width,
                height: cache_height,
                byte_size: i64::try_from(encoded.len())
                    .map_err(|_| "ASTRA_EMU_COVER_IMAGE_BOUNDS".to_owned())?,
            })
            .map_err(|error| error.to_string())?;
        if let Some(old) = existing.filter(|old| old.cache_relative_path != relative_path) {
            let old_path = data_dir.join(old.cache_relative_path);
            if old_path.starts_with(&cache_root) {
                let _ = std::fs::remove_file(old_path);
            }
        }
    }
    Ok(())
}

impl ManagerController for AstraEmuManagerController {
    fn model(&self) -> Result<ManagerViewModel, String> {
        let translation_profile = self
            .library
            .translation_profile()
            .map_err(|error| error.to_string())?;
        let query = self.search_query.to_lowercase();
        let games = self
            .library
            .list_cases()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|case| {
                query.is_empty()
                    || case.title.to_lowercase().contains(&query)
                    || case.case_identity.to_lowercase().contains(&query)
                    || case
                        .family_override
                        .as_deref()
                        .is_some_and(|family| family.to_lowercase().contains(&query))
            })
            .map(|case| {
                let cover_uri = self
                    .library
                    .cover_cache(&case.case_identity)
                    .map_err(|error| error.to_string())?
                    .map(|cover| {
                        self.data_dir
                            .join(cover.cache_relative_path)
                            .to_string_lossy()
                            .into_owned()
                    })
                    .unwrap_or_default();
                Ok(GameCardViewModel {
                    case_id: case.case_identity,
                    title: case.title,
                    family: case.family_override.unwrap_or_else(|| "Auto probe".into()),
                    cover_uri,
                    diagnostic: String::new(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let selected_nls = self
            .selected_case_id
            .as_deref()
            .map(|case_id| self.library.case_runtime_profile(case_id))
            .transpose()
            .map_err(|error| error.to_string())?
            .flatten()
            .and_then(|profile| profile.family_options.get("fvp.nls").cloned())
            .unwrap_or_else(|| "Not configured".into());
        let persistent_cache = self
            .selected_case_id
            .as_deref()
            .map(|case_id| self.library.persistent_translation_cache_enabled(case_id))
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let consent_present = self
            .library
            .translation_consent()
            .map_err(|error| error.to_string())?
            .is_some();
        let endpoint_kind = translation_profile
            .as_ref()
            .map(|profile| profile.endpoint_kind.clone())
            .unwrap_or_else(|| "ecnu".into());
        let profile_endpoint = translation_profile
            .as_ref()
            .map(|profile| profile.endpoint.clone())
            .unwrap_or_else(|| astra_emu_translation_openai_compatible::ECNU_BASE_URL.into());
        let protocol = translation_profile
            .as_ref()
            .map(|profile| profile.protocol.clone())
            .unwrap_or_else(|| "responses".into());
        Ok(ManagerViewModel {
            games,
            selected_case_id: self.selected_case_id.clone(),
            search_query: self.search_query.clone(),
            endpoint_identity: translation_profile
                .as_ref()
                .map(|profile| profile.endpoint.clone())
                .unwrap_or_else(|| "Not configured".into()),
            model_identity: translation_profile
                .as_ref()
                .map(|profile| profile.model.clone())
                .unwrap_or_else(|| "Not configured".into()),
            global_diagnostic: self.diagnostic.clone(),
            selected_nls,
            translation_endpoint_kind: endpoint_kind,
            translation_endpoint: profile_endpoint,
            translation_protocol: protocol,
            translation_model: translation_profile
                .as_ref()
                .map(|profile| profile.model.clone())
                .unwrap_or_default(),
            translation_target_language: translation_profile
                .as_ref()
                .map(|profile| profile.target_language.clone())
                .unwrap_or_else(|| "zh-CN".into()),
            translation_context_sentences: translation_profile
                .as_ref()
                .map(|profile| i32::from(profile.context_sentences))
                .unwrap_or(10),
            translation_body_limit_bytes: translation_profile
                .as_ref()
                .and_then(|profile| i32::try_from(profile.body_limit_bytes).ok())
                .unwrap_or(16 * 1024),
            translation_timeout_ms: translation_profile
                .as_ref()
                .and_then(|profile| i32::try_from(profile.timeout_ms).ok())
                .unwrap_or(30_000),
            translation_background: translation_profile
                .as_ref()
                .and_then(|profile| profile.background.clone())
                .unwrap_or_default(),
            translation_glossary: translation_profile
                .as_ref()
                .map(|profile| {
                    profile
                        .glossary
                        .iter()
                        .map(|(source, target)| format!("{source} = {target}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
            translation_consent_present: consent_present,
            translation_persistent_cache: persistent_cache,
            filter_preset: self
                .runtime
                .try_borrow()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .filter_preset()
                .to_owned(),
            diagnostics_summary: self
                .runtime
                .try_borrow()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .diagnostics_summary(),
            patches_summary: self.patch_summary.clone(),
        })
    }

    fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        if self
            .library
            .case(case_id)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("ASTRA_EMU_CASE_SELECTION_MISSING".into());
        }
        self.selected_case_id = Some(case_id.to_owned());
        self.diagnostic.clear();
        self.model()
    }

    fn search(&mut self, query: &str) -> Result<ManagerViewModel, String> {
        let query = query.trim();
        if query.len() > 256 || query.chars().any(char::is_control) {
            return Err("ASTRA_EMU_LIBRARY_SEARCH_INVALID".into());
        }
        self.search_query = query.to_owned();
        self.diagnostic.clear();
        self.model()
    }

    fn configure_nls(&mut self, nls: &str) -> Result<ManagerViewModel, String> {
        if !matches!(nls, "shift_jis" | "gbk" | "utf8") {
            return Err("ASTRA_EMU_FVP_NLS_INVALID".into());
        }
        let case_identity = self
            .selected_case_id
            .clone()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        let mut profile = self
            .library
            .case_runtime_profile(&case_identity)
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| default_case_profile(case_identity));
        profile.family_options.insert("fvp.nls".into(), nls.into());
        self.library
            .set_case_runtime_profile(&profile)
            .map_err(|error| error.to_string())?;
        self.diagnostic.clear();
        self.model()
    }

    fn save_translation_profile(
        &mut self,
        endpoint_kind: &str,
        endpoint: &str,
        protocol: &str,
        model: &str,
        target_language: &str,
        context_sentences: i32,
        body_limit_bytes: i32,
        timeout_ms: i32,
        background: &str,
        glossary: &str,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        let (endpoint_kind, profile_id) = match endpoint_kind {
            "ecnu" => (TranslationEndpointKind::Ecnu, "ecnu.default"),
            "openai" => (TranslationEndpointKind::OpenAi, "openai.default"),
            "third_party" => (TranslationEndpointKind::ThirdParty, "third-party.default"),
            _ => return Err("ASTRA_EMU_TRANSLATION_ENDPOINT_KIND_INVALID".into()),
        };
        let protocol = match protocol {
            "responses" => TranslationProtocol::Responses,
            "chat_completions" => TranslationProtocol::ChatCompletions,
            _ => return Err("ASTRA_EMU_TRANSLATION_PROTOCOL_INVALID".into()),
        };
        let context_sentences = u8::try_from(context_sentences)
            .map_err(|_| "ASTRA_EMU_TRANSLATION_CONTEXT_INVALID".to_owned())?;
        let body_limit_bytes = u32::try_from(body_limit_bytes)
            .map_err(|_| "ASTRA_EMU_TRANSLATION_BODY_LIMIT_INVALID".to_owned())?;
        let timeout_ms = u64::try_from(timeout_ms)
            .map_err(|_| "ASTRA_EMU_TRANSLATION_TIMEOUT_INVALID".to_owned())?;
        let secret_reference = format!("astraemu.translation.{profile_id}");
        let profile = TranslationProfile {
            profile_id: profile_id.into(),
            endpoint_kind,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            protocol,
            model: model.trim().to_owned(),
            target_language: target_language.trim().to_owned(),
            context_sentences,
            body_limit_bytes,
            timeout_ms,
            secret_reference: secret_reference.clone(),
        };
        profile.validate().map_err(|error| error.to_string())?;
        let glossary = parse_glossary(glossary)?;
        let record = TranslationProfileRecord {
            profile_id: profile.profile_id.clone(),
            endpoint_kind: match profile.endpoint_kind {
                TranslationEndpointKind::Ecnu => "ecnu",
                TranslationEndpointKind::OpenAi => "openai",
                TranslationEndpointKind::ThirdParty => "third_party",
            }
            .into(),
            endpoint: profile.endpoint.clone(),
            protocol: match profile.protocol {
                TranslationProtocol::Responses => "responses",
                TranslationProtocol::ChatCompletions => "chat_completions",
            }
            .into(),
            model: profile.model.clone(),
            target_language: profile.target_language.clone(),
            context_sentences: profile.context_sentences,
            body_limit_bytes: profile.body_limit_bytes,
            timeout_ms: profile.timeout_ms,
            secret_reference: secret_reference.clone(),
            background: (!background.trim().is_empty()).then(|| background.trim().to_owned()),
            glossary,
        };
        let secret_store = ManagerSecretStore::open().map_err(|error| error.to_string())?;
        if secret.is_empty() {
            secret_store
                .resolve(&secret_reference)
                .map_err(|_| "ASTRA_EMU_TRANSLATION_SECRET_REQUIRED".to_owned())?;
        } else {
            secret_store
                .store(&secret_reference, secret)
                .map_err(|error| error.to_string())?;
        }
        self.library
            .set_translation_profile(&record)
            .map_err(|error| error.to_string())?;
        self.diagnostic.clear();
        self.model()
    }

    fn grant_translation_consent(&mut self) -> Result<ManagerViewModel, String> {
        if self
            .library
            .translation_consent()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return self.model();
        }
        let record = self
            .library
            .translation_profile()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED".to_owned())?;
        let profile = translation_profile_from_record(&record)?;
        let granted_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "ASTRA_EMU_SYSTEM_CLOCK_INVALID".to_owned())?
            .as_millis()
            .try_into()
            .map_err(|_| "ASTRA_EMU_SYSTEM_CLOCK_OVERFLOW".to_owned())?;
        self.library
            .grant_translation_consent(&TranslationConsent {
                provider_identity: profile.provider_identity(),
                endpoint: profile.endpoint,
                model: profile.model,
                granted_at_unix_ms,
            })
            .map_err(|error| error.to_string())?;
        self.diagnostic.clear();
        self.model()
    }

    fn set_translation_cache(&mut self, enabled: bool) -> Result<ManagerViewModel, String> {
        let case_identity = self
            .selected_case_id
            .clone()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        self.library
            .set_persistent_translation_cache(&case_identity, enabled)
            .map_err(|error| error.to_string())?;
        self.diagnostic.clear();
        self.model()
    }

    fn set_filter_preset(&mut self, preset_id: &str) -> Result<ManagerViewModel, String> {
        self.runtime
            .try_borrow_mut()
            .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
            .set_filter_preset(preset_id)?;
        self.diagnostic.clear();
        self.model()
    }

    fn set_patch_mode(&mut self, mode: &str) -> Result<ManagerViewModel, String> {
        if !matches!(mode, "no_patch" | "trusted") {
            return Err("ASTRA_EMU_PATCH_MODE_INVALID".into());
        }
        let case_identity = self
            .selected_case_id
            .clone()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        let mut profile = self
            .library
            .case_runtime_profile(&case_identity)
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| default_case_profile(case_identity));
        profile
            .family_options
            .insert("patch.mode".into(), mode.into());
        self.library
            .set_case_runtime_profile(&profile)
            .map_err(|error| error.to_string())?;
        self.patch_summary = if mode == "trusted" {
            "Trusted mode selected. Launch requires a valid UTF-8 astraemu.patch.luau in the authorized case root; any violation blocks launch.".into()
        } else {
            "Explicit no-patch mode is active.".into()
        };
        self.diagnostic.clear();
        self.model()
    }

    fn rescan(&mut self) -> Result<ManagerViewModel, String> {
        let grants = self
            .library
            .list_grants()
            .map_err(|error| error.to_string())?;
        if grants.is_empty() {
            self.choose_and_add_grant()?;
        } else {
            for grant in grants.iter().filter(|grant| grant.active) {
                self.scan_grant(grant)?;
            }
        }
        self.diagnostic.clear();
        self.model()
    }

    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        self.select_case(case_id)?;
        let case = self
            .library
            .case(case_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        let mut profile = self
            .library
            .case_runtime_profile(case_id)
            .map_err(|error| error.to_string())?;
        let grant = self
            .library
            .list_grants()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|grant| grant.source_id == case.source_id && grant.active)
            .ok_or_else(|| "ASTRA_EMU_SOURCE_GRANT_INACTIVE".to_owned())?;
        if grant.token_kind != platform_grant_kind() {
            return Err("ASTRA_EMU_SOURCE_GRANT_PLATFORM_MISMATCH".into());
        }
        let mount_identity = Hash256::from_sha256(case.content_hash.as_bytes()).to_string();
        let mount_set_id = format!("mount-{}", &mount_identity[..32]);
        let translation_config = TranslationLaunchConfig {
            case_identity: case.case_identity.clone(),
            profile: self
                .library
                .translation_profile()
                .map_err(|error| error.to_string())?,
            consent_present: self
                .library
                .translation_consent()
                .map_err(|error| error.to_string())?
                .is_some(),
            persistent_cache_enabled: self
                .library
                .persistent_translation_cache_enabled(&case.case_identity)
                .map_err(|error| error.to_string())?,
            cached: self
                .library
                .translations_for_case(&case.case_identity)
                .map_err(|error| error.to_string())?,
        };
        self.vfs.bind(&mount_set_id, &grant.platform_token)?;
        let detected = self
            .runtime
            .try_borrow()
            .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
            .probe_fvp_profile(&case, &mount_set_id);
        match detected {
            Ok(mut detected) => {
                if let Some(explicit) = profile.take() {
                    if explicit.case_identity != case.case_identity || explicit.family_id != "fvp" {
                        self.vfs.unbind(&mount_set_id);
                        return Err("ASTRA_EMU_EXPLICIT_PROFILE_BINDING_MISMATCH".into());
                    }
                    detected.fixed_delta_ns = explicit.fixed_delta_ns;
                    detected.compatibility_profile = explicit.compatibility_profile;
                    detected.family_options.extend(explicit.family_options);
                }
                if let Err(error) = self.library.set_case_runtime_profile(&detected) {
                    self.vfs.unbind(&mount_set_id);
                    return Err(error.to_string());
                }
                profile = Some(detected);
            }
            Err(error) => {
                self.vfs.unbind(&mount_set_id);
                return Err(error);
            }
        }
        let profile = profile.ok_or_else(|| "ASTRA_EMU_CASE_PROFILE_NOT_CONFIGURED".to_owned())?;
        if let Err(error) = self.apply_trusted_patch(&profile, &mount_set_id) {
            self.vfs.unbind(&mount_set_id);
            return Err(error);
        }
        let patch_actions = std::mem::take(&mut self.pending_patch_actions);
        let launch = self
            .runtime
            .try_borrow_mut()
            .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
            .launch(
                &case,
                profile,
                mount_set_id.clone(),
                translation_config,
                patch_actions,
            );
        if let Err(error) = launch {
            self.vfs.unbind(&mount_set_id);
            return Err(error);
        }
        self.active_mount_set_id = Some(mount_set_id);
        #[cfg(target_os = "android")]
        if let Err(error) = android_platform::set_game_mode(true) {
            if let Ok(mut runtime) = self.runtime.try_borrow_mut() {
                let _ = runtime.shutdown();
            }
            if let Some(mount_set_id) = self.active_mount_set_id.take() {
                self.vfs.unbind(&mount_set_id);
            }
            return Err(error);
        }
        self.diagnostic.clear();
        self.model()
    }

    fn leave_game(&mut self) -> Result<ManagerViewModel, String> {
        let writes = {
            let mut runtime = self
                .runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?;
            let writes = runtime.take_translation_writes();
            runtime.shutdown()?;
            writes
        };
        if let Some(mount_set_id) = self.active_mount_set_id.take() {
            self.vfs.unbind(&mount_set_id);
        }
        for record in writes {
            self.library
                .store_translation(&record)
                .map_err(|error| error.to_string())?;
        }
        #[cfg(target_os = "android")]
        android_platform::set_game_mode(false)?;
        self.diagnostic.clear();
        self.model()
    }

    #[cfg(target_os = "android")]
    fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String> {
        for state in android_platform::take_pending_lifecycle()? {
            let suspended = !matches!(state, android_platform::AndroidLifecycleState::Resumed);
            self.runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .set_suspended(suspended)?;
        }
        let grants = android_platform::take_pending_tree_grants()?;
        if grants.is_empty() {
            return Ok(None);
        }
        for grant in grants {
            self.accept_android_grant(grant)?;
        }
        self.diagnostic.clear();
        self.model().map(Some)
    }

    fn game_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String> {
        self.runtime
            .try_borrow_mut()
            .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
            .queue_input(control, pressed, value)
    }

    fn reset_translation(&mut self) -> Result<(), String> {
        self.runtime
            .try_borrow_mut()
            .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
            .reset_translation()
    }
}

impl Drop for AstraEmuManagerController {
    fn drop(&mut self) {
        if self.active_mount_set_id.is_some() {
            if let Ok(mut runtime) = self.runtime.try_borrow_mut() {
                for record in runtime.take_translation_writes() {
                    if let Err(error) = self.library.store_translation(&record) {
                        tracing::error!(
                            event = "astra.emu.translation.drop_persist_failed",
                            diagnostic_code = "ASTRA_EMU_TRANSLATION_DROP_PERSIST_FAILED",
                            error_kind = %error
                        );
                    }
                }
                let _ = runtime.shutdown();
            }
            if let Some(mount_set_id) = self.active_mount_set_id.take() {
                self.vfs.unbind(&mount_set_id);
            }
        }
    }
}

fn run_application() -> Result<(), Box<dyn std::error::Error>> {
    let diagnostics_dir = platform_data_dir()?.join("diagnostics");
    std::fs::create_dir_all(&diagnostics_dir)?;
    let mut observability = astra_observability::HostObservabilityConfig::for_cli("info");
    observability.role = astra_observability::HostRole::Manager;
    observability.console = false;
    observability.log_dir = Some(diagnostics_dir);
    let _observability = astra_observability::init_host(observability)?;
    tracing::info!(event = "astra.emu.manager.start");
    let mut controller = AstraEmuManagerController::open()?;
    let quick_launch = controller.apply_quick_launch_from_environment()?;
    let runtime = controller.runtime.clone();
    run_manager_with_initial_state(
        controller,
        ManagerStageRenderer {
            texture: None,
            scene_texture: None,
            runtime,
            gpu: None,
            stage_width: 1024,
            stage_height: 768,
            texture_dirty: false,
            scene_initialized: false,
        },
        quick_launch,
    )?;
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_application()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: slint::android::AndroidApp) {
    android_platform::initialize(app.clone()).expect("ASTRA_EMU_ANDROID_CONTEXT_INIT");
    slint::android::init(app).expect("ASTRA_EMU_ANDROID_BACKEND_INIT");
    if let Err(error) = run_application() {
        tracing::error!(
            event = "astra.emu.android.fatal",
            diagnostic_code = "ASTRA_EMU_ANDROID_FATAL",
            error_kind = %error
        );
        panic!("ASTRA_EMU_ANDROID_FATAL");
    }
}
use audio_executor::HostAudioExecutor;

#[cfg(test)]
mod manager_tests {
    use std::{collections::BTreeMap, io::Cursor, sync::Arc};

    use astra_emu_manager_core::{
        CancellationToken, GrantedSourceEntry, GrantedSourceReader, Library, LibraryScanner,
        PatchHostAction, ScanLimits, SourceGrant, SourceScanError,
    };

    use super::{
        apply_audio_media_hook, parse_glossary, refresh_cover_cache, validate_patch_actions,
    };

    struct MemorySource(BTreeMap<String, Vec<u8>>);

    impl GrantedSourceReader for MemorySource {
        fn enumerate(
            &self,
            _cancellation: &CancellationToken,
        ) -> Result<Vec<GrantedSourceEntry>, SourceScanError> {
            Ok(self
                .0
                .iter()
                .map(|(path, bytes)| GrantedSourceEntry {
                    relative_path: path.clone(),
                    modified_ns: 1,
                    byte_size: bytes.len() as u64,
                    is_file: true,
                })
                .collect())
        }

        fn read_file(&self, path: &str, max_bytes: u64) -> Result<Vec<u8>, SourceScanError> {
            let bytes = self.0.get(path).ok_or(SourceScanError::Read)?;
            if bytes.len() as u64 > max_bytes {
                return Err(SourceScanError::ScriptBounds);
            }
            Ok(bytes.clone())
        }
    }

    #[test]
    fn glossary_parser_is_ordered_bounded_and_rejects_ambiguous_entries() {
        assert_eq!(
            parse_glossary("Alice = 艾丽丝\nSword=剑").unwrap(),
            vec![
                ("Alice".into(), "艾丽丝".into()),
                ("Sword".into(), "剑".into())
            ]
        );
        assert!(parse_glossary("missing separator").is_err());
        assert!(parse_glossary("Alice=A\nAlice=B").is_err());
        assert!(parse_glossary("=empty").is_err());
    }

    #[test]
    fn cover_cache_is_content_addressed_and_bounded() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 48)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let source = Arc::new(MemorySource(BTreeMap::from([
            ("game/start.hcb".into(), b"fixture-hcb".to_vec()),
            ("game/cover.png".into(), png.into_inner()),
        ])));
        let mut library = Library::in_memory().unwrap();
        library
            .upsert_grant(&SourceGrant {
                source_id: "root-1".into(),
                alias: "fixture".into(),
                platform_token: "opaque".into(),
                token_kind: "test".into(),
                active: true,
            })
            .unwrap();
        LibraryScanner::new(ScanLimits::default())
            .unwrap()
            .scan(
                &mut library,
                "root-1",
                source.clone(),
                &CancellationToken::default(),
            )
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        refresh_cover_cache(&mut library, "root-1", source, directory.path()).unwrap();
        let case = library.list_cases().unwrap().pop().unwrap();
        let cover = library.cover_cache(&case.case_identity).unwrap().unwrap();
        assert_eq!((cover.width, cover.height), (32, 48));
        assert!(directory.path().join(cover.cache_relative_path).is_file());
    }

    #[test]
    fn patch_host_bindings_are_unique_and_media_rewrites_are_revalidated() {
        let original = "audio/original.ogg";
        let target_hash = astra_core::Hash256::from_sha256(original.as_bytes()).to_string();
        let (text, media, effects) = validate_patch_actions(vec![
            PatchHostAction::TextHook {
                target_hash: "all".into(),
                replacement: "replacement".into(),
            },
            PatchHostAction::MediaHook {
                target_hash: target_hash.clone(),
                replacement_uri: "audio/replacement.ogg".into(),
            },
            PatchHostAction::DeterministicEffect {
                target: "event.patch_ready".into(),
                payload: vec![1, 2, 3],
            },
        ])
        .unwrap();
        assert_eq!(text["all"], "replacement");
        assert_eq!(media[&target_hash], "audio/replacement.ogg");
        assert!(matches!(
            &effects[0],
            astra_emu_family_api::LegacyEffect::RuntimeEvent { event, payload, .. }
                if event == "event.patch_ready" && payload == &[1, 2, 3]
        ));

        let mut command = astra_emu_family_api::LegacyAudioCommandV1::LoadResource {
            stream_id: 1,
            encoding: astra_emu_family_api::LegacyAudioEncoding::Ogg,
            resource_uri: original.into(),
        };
        apply_audio_media_hook(&mut command, &media).unwrap();
        assert!(matches!(
            command,
            astra_emu_family_api::LegacyAudioCommandV1::LoadResource { resource_uri, .. }
                if resource_uri == "audio/replacement.ogg"
        ));

        assert!(validate_patch_actions(vec![
            PatchHostAction::TextHook {
                target_hash: "all".into(),
                replacement: "a".into(),
            },
            PatchHostAction::TextHook {
                target_hash: "all".into(),
                replacement: "b".into(),
            },
        ])
        .is_err());
    }
}
