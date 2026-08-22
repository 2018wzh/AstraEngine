use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    sync::Arc,
};

use astra_byte_source::{ByteRange, OwnedByteBuffer};
use astra_core::{Hash256, SchemaVersion};
use astra_emu_family_api::{
    validate_symbol, FamilyId, LegacyAudioCommandV1, LegacyAudioEncoding, LegacyBlackboardMutation,
    LegacyBlendMode, LegacyControlTransaction, LegacyCoverageDelta, LegacyDrawV1,
    LegacyEphemeralText, LegacyEvent, LegacyFamilyPluginDescriptor, LegacyLiveOutput,
    LegacyOpenRequest, LegacyProbeReport, LegacyProbeRequest, LegacyProviderError,
    LegacyRenderResourceFrameV1, LegacyResourceRead, LegacyRestoreReport, LegacyRuntimeHostCtx,
    LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacyRuntimeStatus, LegacyScissorV1,
    LegacySequenced, LegacyShutdownReport, LegacySnapshotEnvelope, LegacySnapshotSection,
    LegacyStepInput, LegacyStepOutput, LegacyTextHorizontalAlignmentV1, LegacyTextLease,
    LegacyTextOutlineV1, LegacyTextPresentationLeaseV1, LegacyTextPresentationV1,
    LegacyTextRegionV1, LegacyTextureFilter, LegacyTextureFormat, LegacyTextureResourceV1,
    LegacyTraceEntry, LegacyVertexV1, LegacyVfsReader, LegacyVideoCommandV1, LegacyVideoMode,
    LegacyVmTraceRecord, LegacyWaitRequest, LEGACY_FAMILY_ABI_FINGERPRINT,
};
use serde::{Deserialize, Serialize};

use crate::{
    parse_sc, MinoriAudioCommand, MinoriAudioEncoding, MinoriAxisScrollFrame, MinoriCharacterFrame,
    MinoriCharacterState, MinoriChoicePresentation, MinoriConfigAudioBus, MinoriConfigChange,
    MinoriConfigControl, MinoriEffectFrame, MinoriExecutedCommand, MinoriLinearScrollFrame,
    MinoriMovieState, MinoriPlayMode, MinoriRuntimeError, MinoriRuntimeState,
    MinoriScreenShakeFrame, MinoriScrollXfFrame, MinoriSecondaryEffectFrame, MinoriStageCommand,
    MinoriStageLayer, MinoriStandLayer, MinoriSystemPage, MinoriVm, MinoriVmEvent,
    MinoriWScroll2Frame, MinoriWaitState, ScOpcodeCatalog, MINORI_CHOICE_PRESENTATION_SCHEMA,
    MINORI_RUNTIME_STATE_SCHEMA,
};

pub const MINORI_FAMILY_ID: &str = "minori";
pub const MINORI_RUNTIME_PROVIDER_ID: &str = "astra.emu.family.minori";
const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_WSCROLL2_SYNC_BYTES: u64 = 64 * 1024;
const MAX_WSCROLL2_SYNC_VALUES: usize = 4096;
const MAX_EPHEMERAL_TEXT_BYTES: usize = 64 * 1024;
const MINORI_CONTROL_KEY: &str = "control";
const MINORI_POINTER_X: &str = "pointer.x";
const MINORI_POINTER_Y: &str = "pointer.y";
const MINORI_POINTER_PRIMARY: &str = "pointer.primary";
const MINORI_GAME_MENU_PLAY_MODE_LEFT: i32 = 1125;
const MINORI_GAME_MENU_PLAY_MODE_TOP: i32 = 577;
const MINORI_GAME_MENU_PLAY_MODE_RIGHT: i32 = 1177;
const MINORI_GAME_MENU_PLAY_MODE_BOTTOM: i32 = 616;
const MINORI_CHOICE_CONFIRM_CONTROLS: [&str; 2] = ["enter", "space"];
const MINORI_CHOICE_NAVIGATION_CONTROLS: [&str; 2] = ["arrow_up", "arrow_down"];
const MINORI_CHOICE_RESOURCE_URIS: [&str; 3] = [
    "minori:/sys/SelectBLur.png",
    "minori:/sys/SelectFocus.png",
    "minori:/sys/SelectActive.png",
];
const MINORI_CHOICE_TEXTURE_BASE: u32 = 500;
const MINORI_CHARACTER_TEXTURE_BASE: u32 = 10_000;
const MINORI_SYSTEM_TEXTURE_ID: u32 = 20_000;
const MINORI_BACKLOG_GAUGE_TEXTURE_ID: u32 = 20_001;
const MINORI_BACKLOG_BALL_TEXTURE_ID: u32 = 20_002;
const MINORI_CONFIG_KNOB_TEXTURE_ID: u32 = 20_010;
const MINORI_CONFIG_CHECKMARK_TEXTURE_ID: u32 = 20_011;
const MINORI_CONFIG_CIRCLE_TEXTURE_ID: u32 = 20_012;
const MINORI_TITLE_BASE_ITEM_COUNT: u32 = 4;
const MINORI_TITLE_MEMORIES_ITEM_COUNT: u32 = 5;
const MINORI_MEMORIES_ITEM_COUNT: u32 = 5;
const MINORI_PLATFORM_STORAGE_PROVIDER_ID: &str = "astra.platform.storage";
const MINORI_GLOBAL_PROGRESS_OPTION: &str = "astra.provider.storage";
const MINORI_GLOBAL_PROGRESS_SLOT: &str = "minori-global-progress-v1";
const MINORI_GLOBAL_PROGRESS_SCHEMA: &str = "astra.emu.minori.global_progress.v1";
const MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA: &str = "astra.emu.minori.global_progress_snapshot.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinoriGlobalProgressV1 {
    schema: String,
    gallery_unlocks: Vec<Hash256>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinoriGlobalProgressSnapshotV1 {
    schema: String,
    enabled: bool,
    loaded: bool,
    persisted_unlocks: Vec<Hash256>,
}

#[derive(Debug, Clone)]
enum MinoriGlobalProgressRequest {
    Load {
        request_id: String,
    },
    Store {
        request_id: String,
        unlocks: Vec<Hash256>,
    },
}

#[derive(Debug, Clone)]
struct MinoriGlobalProgressSession {
    enabled: bool,
    loaded: bool,
    persisted_unlocks: Vec<Hash256>,
    pending: Option<MinoriGlobalProgressRequest>,
}
fn message_input_keys() -> Vec<String> {
    MINORI_MESSAGE_HOST_AWAIT_CONTROLS
        .iter()
        .map(|key| key.to_string())
        .collect()
}

fn choice_input_keys() -> Vec<String> {
    MINORI_CHOICE_CONFIRM_CONTROLS
        .iter()
        .map(|key| key.to_string())
        .collect()
}

const MINORI_MESSAGE_INPUT_CONTROLS: [&str; 3] = ["enter", "space", "pointer.primary"];
const MINORI_MESSAGE_HOST_AWAIT_CONTROLS: [&str; 2] = ["enter", "space"];

fn minori_message_presentation(
    stage_size: Option<(u32, u32)>,
) -> Result<LegacyTextPresentationV1, LegacyProviderError> {
    if stage_size != Some((1280, 720)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY",
            "the verified Minori message layout requires the 1280x720 reference stage",
        ));
    }
    let presentation = LegacyTextPresentationV1 {
        layout_id: "minori.message".into(),
        language: "ja-JP".into(),
        font_families: vec!["Noto Sans JP".into()],
        body: LegacyTextRegionV1 {
            x: 160,
            y: 568,
            width: 960,
            height: 112,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 3,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
        },
        speaker: Some(LegacyTextRegionV1 {
            x: 160,
            y: 528,
            width: 960,
            height: 32,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 1,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
        }),
        rgba: [255, 255, 255, 255],
        outline: Some(LegacyTextOutlineV1 {
            radius: 2,
            rgba: [0, 0, 0, 192],
        }),
    };
    presentation.validate()?;
    Ok(presentation)
}

struct MinoriSession {
    case_fingerprint: Hash256,
    mount_set_id: String,
    fixed_delta_ns: u64,
    session_seed: u64,
    stage_size: Option<(u32, u32)>,
    vm: MinoriVm,
    ephemeral_text: BTreeMap<String, LegacyEphemeralText>,
    collect_evidence_vm_trace: bool,
    evidence_contexts: BTreeMap<u32, Hash256>,
    evidence_vm_trace: BTreeSet<(u32, u32, u8)>,
    restore_audio_pending: bool,
    restore_presentation_pending: bool,
    reported_system_page: Option<MinoriSystemPage>,
    reported_play_mode: Option<MinoriPlayMode>,
    reported_gallery_unlock_count: Option<usize>,
    reported_choice_active: Option<bool>,
    global_progress: MinoriGlobalProgressSession,
    poisoned: bool,
}

#[derive(Default)]
pub struct MinoriRuntimeProvider {
    vfs: Option<Arc<dyn LegacyVfsReader>>,
    sessions: BTreeMap<String, MinoriSession>,
}

impl MinoriRuntimeProvider {
    pub fn with_vfs(vfs: Arc<dyn LegacyVfsReader>) -> Self {
        Self {
            vfs: Some(vfs),
            sessions: BTreeMap::new(),
        }
    }

    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    fn vfs(&self) -> Result<&Arc<dyn LegacyVfsReader>, LegacyProviderError> {
        self.vfs.as_ref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_RUNTIME_VFS",
                "Minori runtime has no explicitly bound VFS reader",
            )
        })
    }
}

pub fn create_static_minori_provider(
    vfs: Arc<dyn LegacyVfsReader>,
) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError> {
    let provider = MinoriRuntimeProvider::with_vfs(vfs);
    provider.descriptor().validate()?;
    Ok(Box::new(provider))
}

impl LegacyRuntimeProvider for MinoriRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        LegacyFamilyPluginDescriptor {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            plugin_id: "astra.emu.minori".into(),
            provider_id: MINORI_RUNTIME_PROVIDER_ID.into(),
            core_kind: astra_emu_family_api::LegacyFamilyCoreKind::Native,
            presentation_mode: astra_emu_family_api::LegacyFamilyPresentationMode::MultiLayer,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_MINORI_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_MINORI_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            supported_formats: vec![
                "minori.sc".into(),
                "minori.paz".into(),
                "minori.ani".into(),
                "minori.sqz".into(),
            ],
            permissions: vec![
                "vfs.read".into(),
                "media.submit".into(),
                "storage.request".into(),
            ],
            report_redaction: "astra.emu.redaction.v1".into(),
            license: "MPL-2.0".into(),
        }
    }

    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError> {
        ctx.validate()?;
        if request.max_entries == 0 || request.max_metadata_bytes == 0 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PROBE_BUDGET",
                "Minori probe budget is empty",
            ));
        }
        let candidates = request
            .candidate_uris
            .iter()
            .take(request.max_entries as usize)
            .filter(|uri| uri.starts_with("minori:/scr/") && uri.ends_with(".sc"))
            .collect::<Vec<_>>();
        let candidate = candidates
            .iter()
            .find(|uri| uri.eq_ignore_ascii_case("minori:/scr/test.sc"))
            .copied()
            .or_else(|| (candidates.len() == 1).then(|| candidates[0]))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_PROBE_ENTRY",
                    "probe requires one unambiguous Minori entry script",
                )
            })?;
        let bytes = self.vfs()?.read_file(
            &request.root_mount_id,
            candidate,
            request.max_metadata_bytes.min(MAX_SCRIPT_BYTES),
        )?;
        parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        let identity = Hash256::from_sha256(&bytes);
        let marker_match =
            request.marker_hashes.is_empty() || request.marker_hashes.contains(&identity);
        Ok(LegacyProbeReport {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            confidence_permyriad: if marker_match { 10_000 } else { 0 },
            markers: if marker_match {
                vec!["minori.sc.cp932".into(), "minori.sc.command_stream".into()]
            } else {
                Vec::new()
            },
            blockers: Vec::new(),
            content_identity: identity,
        })
    }

    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError> {
        ctx.validate()?;
        validate_symbol("session_id", &request.requested_session_id.0)?;
        validate_symbol("compatibility_profile", &request.compatibility_profile)?;
        if request.fixed_delta_ns == 0 || request.fixed_delta_ns > 1_000_000_000 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FIXED_DELTA",
                "fixed delta is outside 1ns..=1s",
            ));
        }
        if self.sessions.contains_key(&request.requested_session_id.0) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_DUPLICATE",
                "session id is already active",
            ));
        }
        validate_script_uri(&request.script_uri)?;
        let bytes =
            self.vfs()?
                .read_file(&ctx.mount_set_id, &request.script_uri, MAX_SCRIPT_BYTES)?;
        let script_hash = Hash256::from_sha256(&bytes);
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        let title_launch = match request
            .family_options
            .get("astra.launch_entry_explicit")
            .map(String::as_str)
        {
            Some("false") => true,
            Some("true") | None => false,
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_LAUNCH_MODE",
                    "launch entry explicitness must be true or false",
                ));
            }
        };
        let mut vm = MinoriVm::new(
            request.script_uri,
            script_hash,
            script,
            request.session_seed,
        )
        .map_err(runtime_error)?;
        if title_launch {
            vm.begin_title_launch().map_err(runtime_error)?;
        }
        let stage_size = match (
            request.family_options.get("astra.stage_width"),
            request.family_options.get("astra.stage_height"),
        ) {
            (None, None) => None,
            (Some(width), Some(height)) => {
                let width = width.parse::<u32>().map_err(|_| {
                    invalid("ASTRA_EMU_MINORI_STAGE_SIZE", "stage width is invalid")
                })?;
                let height = height.parse::<u32>().map_err(|_| {
                    invalid("ASTRA_EMU_MINORI_STAGE_SIZE", "stage height is invalid")
                })?;
                if !(320..=8192).contains(&width) || !(240..=8192).contains(&height) {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_STAGE_SIZE",
                        "stage dimensions are outside the supported bound",
                    ));
                }
                Some((width, height))
            }
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_STAGE_SIZE",
                    "stage dimensions must be supplied together",
                ));
            }
        };
        let collect_evidence_vm_trace = match request
            .family_options
            .get("astra.hosted_trace_profile")
            .map(String::as_str)
        {
            Some("evidence") => true,
            Some("shipping") | None => false,
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TRACE_PROFILE",
                    "hosted trace profile must be evidence or shipping",
                ));
            }
        };
        let global_progress_enabled = match request
            .family_options
            .get(MINORI_GLOBAL_PROGRESS_OPTION)
            .map(String::as_str)
        {
            None => false,
            Some(MINORI_PLATFORM_STORAGE_PROVIDER_ID) => true,
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_PROVIDER",
                    "global progress requires the explicitly bound platform storage provider",
                ));
            }
        };
        let id = request.requested_session_id;
        self.sessions.insert(
            id.0.clone(),
            MinoriSession {
                case_fingerprint: request.case_fingerprint,
                mount_set_id: ctx.mount_set_id.clone(),
                fixed_delta_ns: request.fixed_delta_ns,
                session_seed: request.session_seed,
                stage_size,
                vm,
                ephemeral_text: BTreeMap::new(),
                collect_evidence_vm_trace,
                evidence_contexts: BTreeMap::new(),
                evidence_vm_trace: BTreeSet::new(),
                restore_audio_pending: false,
                restore_presentation_pending: false,
                reported_system_page: None,
                reported_play_mode: None,
                reported_gallery_unlock_count: None,
                reported_choice_active: None,
                global_progress: MinoriGlobalProgressSession {
                    enabled: global_progress_enabled,
                    loaded: !global_progress_enabled,
                    persisted_unlocks: Vec::new(),
                    pending: None,
                },
                poisoned: false,
            },
        );
        Ok(id)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        mut input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        ctx.validate()?;
        input.validate()?;
        let vfs = Arc::clone(self.vfs()?);
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_POISONED",
                "poisoned session cannot continue",
            ));
        }
        if input.delta_ns != session.fixed_delta_ns || input.session_seed != session.session_seed {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STEP_IDENTITY",
                "step timing or seed does not match the open session",
            ));
        }
        if session
            .vm
            .state()
            .fixed_tick
            .checked_add(1)
            .is_none_or(|expected| input.tick_index != expected)
        {
            session.poisoned = true;
            return Err(runtime_error(MinoriRuntimeError::State));
        }
        for edge in &input.input_edges {
            if edge.control == MINORI_CONTROL_KEY {
                session.vm.set_control_pressed(edge.pressed);
            } else if edge.control == MINORI_POINTER_X {
                session
                    .vm
                    .set_pointer_axis('x', edge.value)
                    .map_err(runtime_error)?;
            } else if edge.control == MINORI_POINTER_Y {
                session
                    .vm
                    .set_pointer_axis('y', edge.value)
                    .map_err(runtime_error)?;
            } else if edge.control == MINORI_POINTER_PRIMARY {
                session.vm.set_pointer_primary_pressed(edge.pressed);
            }
        }
        if session.global_progress.pending.is_some() {
            consume_global_progress_result(session, &input)?;
            input.provider_results.clear();
        } else if session.global_progress.enabled && !session.global_progress.loaded {
            return begin_global_progress_load(session, &input);
        }
        let mut restore_audio = match take_restore_audio_commands(session, &vfs) {
            Ok(commands) => commands,
            Err(error) => {
                session.poisoned = true;
                return Err(error);
            }
        };
        if session.vm.state().system_ui.page == MinoriSystemPage::None
            && backlog_wheel_direction(&input)? == Some(-1)
            && !session.vm.state().backlog.is_empty()
        {
            if !input.await_results.is_empty() || !input.provider_results.is_empty() {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_BACKLOG_RESULT_UNEXPECTED",
                    "opening backlog cannot consume an await or provider result",
                ));
            }
            session.vm.open_backlog().map_err(runtime_error)?;
            session
                .vm
                .advance_system_tick(input.tick_index)
                .map_err(runtime_error)?;
            return system_ui_output(session, &vfs, &input, restore_audio);
        }
        let mut started_game = false;
        if session.vm.state().system_ui.page != MinoriSystemPage::None {
            let backlog_replay_completion = if session.vm.state().system_ui.page
                == MinoriSystemPage::Backlog
                && input.provider_results.is_empty()
                && input.await_results.len() == 1
            {
                let token_id = wait_token(
                    session
                        .vm
                        .state()
                        .wait
                        .as_ref()
                        .ok_or_else(|| runtime_error(MinoriRuntimeError::Backlog))?,
                );
                input.await_results[0].token_id == token_id
                    && input.await_results[0].status == "completed"
                    && input.await_results[0].payload_len == 0
            } else {
                false
            };
            if (!input.await_results.is_empty() && !backlog_replay_completion)
                || !input.provider_results.is_empty()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_RESULT_UNEXPECTED",
                    "system UI cannot consume an await or provider result",
                ));
            }
            if backlog_replay_completion {
                input.await_results.clear();
            }
            let mut action = match apply_system_ui_input(&mut session.vm, &input) {
                Ok(action) => action,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            if backlog_replay_completion {
                action = MinoriSystemUiAction::ReplayBacklogVoice;
            }
            tracing::debug!(
                target: "astra_emu_minori::system_ui",
                event = "astra_emu_minori_system_ui_input_applied",
                page = system_page_name(session.vm.state().system_ui.page),
                focus_index = session.vm.state().system_ui.focus_index,
                action = system_ui_action_name(action),
                input_edge_count = input.input_edges.len(),
                "applied bounded system UI input"
            );
            if action != MinoriSystemUiAction::StartGame {
                let commands = match action {
                    MinoriSystemUiAction::ReplayBacklogVoice => {
                        session.vm.replay_backlog_voice().map_err(runtime_error)?
                    }
                    MinoriSystemUiAction::PresentWithAudioRefresh => session
                        .vm
                        .config_audio_param_commands()
                        .map_err(runtime_error)?,
                    MinoriSystemUiAction::PresentAfterConfigClose => session
                        .vm
                        .close_config_audio_commands()
                        .map_err(runtime_error)?,
                    MinoriSystemUiAction::PresentWithAudioTest(bus) => session
                        .vm
                        .config_test_audio_commands(bus)
                        .map_err(runtime_error)?,
                    _ => Vec::new(),
                };
                if !commands.is_empty() {
                    append_validated_audio_commands(
                        &vfs,
                        &session.mount_set_id,
                        commands.iter(),
                        session.vm.state(),
                        &mut restore_audio,
                    )?;
                }
                session
                    .vm
                    .advance_system_tick(input.tick_index)
                    .map_err(runtime_error)?;
                if action == MinoriSystemUiAction::CloseBacklog {
                    session.vm.close_backlog().map_err(runtime_error)?;
                    return gameplay_resume_output(session, &vfs, &input, restore_audio);
                }
                if action == MinoriSystemUiAction::Exit {
                    session
                        .vm
                        .terminate_system_session()
                        .map_err(runtime_error)?;
                }
                return system_ui_output(session, &vfs, &input, restore_audio);
            }
            started_game = true;
            session.restore_presentation_pending = false;
        }
        let mut play_mode_wait_rebound = session
            .vm
            .rebind_active_message_wait()
            .map_err(runtime_error)?;
        let game_menu_mode_pressed = input.input_edges.iter().any(|edge| {
            edge.control == MINORI_POINTER_PRIMARY
                && edge.pressed
                && (MINORI_GAME_MENU_PLAY_MODE_LEFT..MINORI_GAME_MENU_PLAY_MODE_RIGHT)
                    .contains(&session.vm.state().system_ui.pointer_x)
                && (MINORI_GAME_MENU_PLAY_MODE_TOP..MINORI_GAME_MENU_PLAY_MODE_BOTTOM)
                    .contains(&session.vm.state().system_ui.pointer_y)
        });
        if game_menu_mode_pressed {
            if !input.await_results.is_empty() || !input.provider_results.is_empty() {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_PLAY_MODE_RESULT_UNEXPECTED",
                    "play-mode menu input cannot consume an await or provider result",
                ));
            }
            play_mode_wait_rebound |= session
                .vm
                .toggle_preferred_play_mode()
                .map_err(runtime_error)?;
        }
        // A choice is a modal input wait.  Arrow edges only move the cursor;
        // they never resolve the wait.  Enter/Space is accepted as a direct
        // provider input for embedders that do not translate physical input
        // into an await result first.  The normal host path supplies the
        // await result and therefore rejects a duplicate confirm edge below.
        let mut choice_moved = false;
        let mut choice_committed = false;
        if let Some(MinoriWaitState::Choice { token_id }) = session.vm.state().wait.clone() {
            for edge in input.input_edges.iter().filter(|edge| edge.pressed) {
                let direction = choice_direction(&edge.control);
                if let Some(direction) = direction {
                    session.vm.move_choice(direction).map_err(runtime_error)?;
                    choice_moved = true;
                }
            }
            let confirm_pressed = input.input_edges.iter().any(|edge| {
                edge.pressed
                    && MINORI_CHOICE_CONFIRM_CONTROLS
                        .iter()
                        .any(|control| *control == edge.control)
            });
            if confirm_pressed && !input.await_results.is_empty() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_CHOICE_DUPLICATE_COMPLETION",
                    "choice confirm edge and await result complete the same choice wait",
                ));
            }
            if confirm_pressed && input.await_results.is_empty() {
                session.vm.resolve_wait(&token_id).map_err(runtime_error)?;
                session.vm.commit_choice().map_err(runtime_error)?;
                choice_committed = true;
            }
        }
        // The host normally translates an input edge that satisfies an input
        // wait into an ordered `LegacyAwaitResult`.  Static callers may use the
        // family provider directly, however, so the provider also accepts the
        // canonical key edge and resolves the same wait itself.  Edges that do
        // not satisfy a current input wait are still consumed as physical
        // state notifications; they must never be silently interpreted as a
        // semantic action or make an otherwise valid tick fail.
        if let Some(MinoriWaitState::Input { token_id }) = session
            .vm
            .state()
            .wait
            .clone()
            .filter(|_| !game_menu_mode_pressed)
        {
            if !input.await_results.is_empty()
                && input.input_edges.iter().any(|edge| {
                    edge.pressed
                        && MINORI_MESSAGE_INPUT_CONTROLS
                            .iter()
                            .any(|control| *control == edge.control)
                })
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_INPUT_DUPLICATE_COMPLETION",
                    "input edge and await result complete the same input wait",
                ));
            }
            if input.await_results.is_empty()
                && input.input_edges.iter().any(|edge| {
                    edge.pressed
                        && MINORI_MESSAGE_INPUT_CONTROLS
                            .iter()
                            .any(|control| *control == edge.control)
                })
            {
                session.vm.resolve_wait(&token_id).map_err(runtime_error)?;
            }
        }
        if !input.provider_results.is_empty() {
            let Some(MinoriWaitState::Provider {
                token_id,
                request_id,
            }) = session.vm.state().wait.clone()
            else {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_PROVIDER_RESULT_UNEXPECTED",
                    "provider result was supplied without an active provider wait",
                ));
            };
            if !input.await_results.is_empty()
                || input.provider_results.len() != 1
                || input.provider_results[0].request_id != request_id
                || input.provider_results[0].status != "completed"
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_PROVIDER_RESULT_MISMATCH",
                    "provider result does not match the active provider wait",
                ));
            }
            session.vm.resolve_wait(&token_id).map_err(runtime_error)?;
        }
        let animated_effect = session
            .vm
            .advance_effect_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_firefly = session
            .vm
            .advance_firefly_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_secondary_effect = session
            .vm
            .advance_secondary_effect_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_screen_shake = session
            .vm
            .advance_screen_shake_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_axis_scroll = session
            .vm
            .advance_axis_scroll_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_character = session
            .vm
            .advance_character_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_linear_scroll = session
            .vm
            .advance_linear_scroll_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_scroll_xf = session
            .vm
            .advance_scroll_xf_clock(input.delta_ns)
            .map_err(runtime_error)?;
        let animated_wscroll2 = session
            .vm
            .advance_wscroll2_clock(input.delta_ns)
            .map_err(runtime_error)?;
        if let Some(wait) = session.vm.state().wait.clone() {
            if input.await_results.is_empty() {
                let movie_to_skip = match &wait {
                    MinoriWaitState::Media { media_id, .. }
                        if session.vm.state().system_ui.control_pressed =>
                    {
                        session
                            .vm
                            .state()
                            .movie
                            .as_ref()
                            .filter(|movie| movie.skippable && movie.media_id == media_id.as_str())
                            .map(|movie| movie.media_id.clone())
                    }
                    _ => None,
                };
                session
                    .vm
                    .advance_waiting_tick(input.tick_index)
                    .map_err(runtime_error)?;
                if session.restore_presentation_pending {
                    return gameplay_resume_output(session, &vfs, &input, restore_audio.clone());
                }
                let mut resource_scenes: Vec<LegacySequenced<LegacyRenderResourceFrameV1>> =
                    animated_effect
                        .as_ref()
                        .map(|frame| {
                            effect_presentation(
                                &vfs,
                                &session.mount_set_id,
                                session.stage_size,
                                session.vm.state(),
                                frame,
                            )
                        })
                        .transpose()?
                        .into_iter()
                        .collect();
                if let Some(firefly_event) = &animated_firefly {
                    resource_scenes.push(firefly_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        firefly_event,
                    )?);
                }
                if let Some(axis_scroll) = &animated_axis_scroll {
                    resource_scenes.push(axis_scroll_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        axis_scroll,
                    )?);
                }
                if let Some(character) = &animated_character {
                    resource_scenes.push(character_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        character,
                    )?);
                }
                if let Some(linear_scroll) = &animated_linear_scroll {
                    resource_scenes.push(linear_scroll_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        linear_scroll,
                    )?);
                }
                if let Some(scroll_xf) = &animated_scroll_xf {
                    resource_scenes.push(scroll_xf_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        scroll_xf,
                    )?);
                }
                if let Some(wscroll2) = &animated_wscroll2 {
                    resource_scenes.push(wscroll2_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        wscroll2,
                    )?);
                }
                if let Some(secondary_effect) = &animated_secondary_effect {
                    resource_scenes.push(secondary_effect_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        secondary_effect,
                    )?);
                }
                if let Some(screen_shake) = &animated_screen_shake {
                    resource_scenes.push(screen_shake_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        screen_shake,
                    )?);
                }
                let mut live = LegacyLiveOutput {
                    resource_scenes,
                    audio_commands: restore_audio.clone(),
                    ..LegacyLiveOutput::default()
                };
                if let Some(playback_id) = movie_to_skip {
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    live.video.push(LegacySequenced {
                        sequence,
                        value: LegacyVideoCommandV1::Stop { playback_id },
                    });
                }
                if choice_moved {
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    let choice_event = append_choice_live_output(
                        session,
                        &vfs,
                        input.tick_index,
                        sequence,
                        &mut live,
                    )?;
                    return waiting_output(
                        session,
                        wait,
                        live,
                        Some(choice_event),
                        play_mode_wait_rebound,
                        &input,
                    );
                }
                return waiting_output(session, wait, live, None, play_mode_wait_rebound, &input);
            }
            let expected = wait_token(&wait);
            if input.await_results.len() != 1
                || input.await_results[0].token_id != expected
                || input.await_results[0].status != "completed"
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AWAIT_RESULT",
                    "await result does not match the active wait token",
                ));
            }
            session.vm.resolve_wait(expected).map_err(runtime_error)?;
            if matches!(wait, MinoriWaitState::Choice { .. }) {
                session.vm.commit_choice().map_err(runtime_error)?;
                choice_committed = true;
            }
        } else if !input.await_results.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_AWAIT_UNEXPECTED",
                "step supplied an await result without an active wait",
            ));
        }
        let before = session.vm.state().instruction_count;
        let event = match session
            .vm
            .step(input.tick_index, input.budget.max_instructions)
        {
            Ok(event) => event,
            Err(error) => {
                let command = session.vm.take_executed_commands().last().cloned();
                if let Some(command) = command.as_ref() {
                    tracing::error!(
                        target: "astra_emu_minori::runtime",
                        event = "astra_emu_minori_command_failed",
                        script_hash = %command.script_hash,
                        command_ordinal = command.command_ordinal,
                        opcode_identity = %Hash256::from_sha256(command.opcode.as_bytes()),
                        diagnostic = runtime_error_code(&error),
                        "Minori command execution failed"
                    );
                }
                session.poisoned = true;
                let provider_error = if let Some(command) = command {
                    LegacyProviderError::invalid(
                        runtime_error_code(&error),
                        format!(
                            "{} (script_hash={}, command_ordinal={}, opcode_identity={})",
                            error,
                            command.script_hash,
                            command.command_ordinal,
                            Hash256::from_sha256(command.opcode.as_bytes())
                        ),
                    )
                } else {
                    runtime_error(error)
                };
                return Err(provider_error);
            }
        };
        let executed_commands = session.vm.take_executed_commands();
        if session.collect_evidence_vm_trace {
            if let Err(error) = record_evidence_commands(session, executed_commands) {
                session.poisoned = true;
                return Err(error);
            }
        }
        let chain_target = match &event {
            Some(MinoriVmEvent::Chain { target }) => Some(target.clone()),
            _ => None,
        };
        if let Some(target) = chain_target.as_deref() {
            let switch_result = load_script(&vfs, &session.mount_set_id, target).and_then(
                |(script_uri, script_hash, script)| {
                    session
                        .vm
                        .replace_script(script_uri, script_hash, script)
                        .map_err(runtime_error)
                },
            );
            if let Err(error) = switch_result {
                session.poisoned = true;
                return Err(error);
            }
        }
        let after = session.vm.state().instruction_count;
        let mut live = LegacyLiveOutput {
            clear_text: choice_committed,
            audio_commands: restore_audio,
            ..LegacyLiveOutput::default()
        };
        let returned_to_title =
            matches!(event, Some(MinoriVmEvent::Terminal)) && !session.vm.state().terminal;
        if returned_to_title {
            live.clear_text = true;
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            live.resource_scenes.push(LegacySequenced {
                sequence,
                value: describe_system_page(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    &session.vm,
                )?,
            });
        }
        let mut control_events = Vec::new();
        if let Some(frame) = &animated_effect {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::Stage(_)
                        | MinoriVmEvent::AxisScroll(_)
                        | MinoriVmEvent::Effect(_)
                        | MinoriVmEvent::EffectCleared { .. }
                        | MinoriVmEvent::Panel { .. }
                        | MinoriVmEvent::Character(_)
                )
            ) {
                live.resource_scenes.push(effect_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    frame,
                )?);
            }
        }
        if let Some(firefly_event) = &animated_firefly {
            let is_same_script_event = matches!(
                event,
                Some(MinoriVmEvent::Firefly(_))
                    | Some(MinoriVmEvent::FireflyCleared { .. })
                    | Some(MinoriVmEvent::AxisScroll(_))
                    | Some(MinoriVmEvent::Character(_))
            );
            if !is_same_script_event {
                live.resource_scenes.push(firefly_event_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    firefly_event,
                )?);
            }
        }
        if let Some(axis_scroll) = &animated_axis_scroll {
            if !matches!(event, Some(MinoriVmEvent::AxisScroll(_))) {
                live.resource_scenes.push(axis_scroll_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    axis_scroll,
                )?);
            }
        }
        if let Some(character) = &animated_character {
            if !matches!(event, Some(MinoriVmEvent::Character(_))) {
                live.resource_scenes.push(character_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    character,
                )?);
            }
        }
        if let Some(linear_scroll) = &animated_linear_scroll {
            if !matches!(event, Some(MinoriVmEvent::LinearScroll(_))) {
                live.resource_scenes.push(linear_scroll_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    linear_scroll,
                )?);
            }
        }
        if let Some(scroll_xf) = &animated_scroll_xf {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::ScrollXf(_)
                        | MinoriVmEvent::AxisScroll(_)
                        | MinoriVmEvent::Character(_)
                )
            ) {
                live.resource_scenes.push(scroll_xf_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    scroll_xf,
                )?);
            }
        }
        if let Some(wscroll2) = &animated_wscroll2 {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::Stage(_)
                        | MinoriVmEvent::AxisScroll(_)
                        | MinoriVmEvent::WScroll2(_)
                        | MinoriVmEvent::Panel { .. }
                        | MinoriVmEvent::Effect(_)
                        | MinoriVmEvent::EffectCleared { .. }
                        | MinoriVmEvent::Firefly(_)
                        | MinoriVmEvent::FireflyCleared { .. }
                        | MinoriVmEvent::Chain { .. }
                        | MinoriVmEvent::Character(_)
                )
            ) {
                live.resource_scenes.push(wscroll2_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    wscroll2,
                )?);
            }
        }
        if let Some(secondary_effect) = &animated_secondary_effect {
            if !matches!(
                event,
                Some(MinoriVmEvent::SecondaryEffect(_))
                    | Some(MinoriVmEvent::SecondaryEffectCleared { .. })
            ) {
                live.resource_scenes
                    .push(secondary_effect_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        secondary_effect,
                    )?);
            }
        }
        if let Some(screen_shake) = &animated_screen_shake {
            if !matches!(event, Some(MinoriVmEvent::ScreenShake(_))) {
                live.resource_scenes.push(screen_shake_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    screen_shake,
                )?);
            }
        }
        if let Some(MinoriVmEvent::Message {
            presentation_sequence,
            capture_sequence,
            text,
            speaker,
            audio_commands: _,
            wait: _,
        }) = &event
        {
            if text.len() > MAX_EPHEMERAL_TEXT_BYTES
                || speaker
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                    "message or speaker exceeds the ephemeral text channel bound",
                ));
            }
            let lease_id = format!("minori.text.{}.{}", input.tick_index, capture_sequence);
            let presentation = LegacyTextPresentationLeaseV1 {
                lease_id: lease_id.clone(),
                presentation: match minori_message_presentation(session.stage_size) {
                    Ok(presentation) => presentation,
                    Err(error) => {
                        session.poisoned = true;
                        return Err(error);
                    }
                },
            };
            presentation.validate().inspect_err(|_| {
                session.poisoned = true;
            })?;
            if session
                .ephemeral_text
                .insert(
                    lease_id.clone(),
                    LegacyEphemeralText {
                        lease_id: lease_id.clone(),
                        text: text.clone(),
                        speaker: speaker.clone(),
                    },
                )
                .is_some()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
                    "ephemeral text lease id is duplicated",
                ));
            }
            live.text_presentations.push(LegacySequenced {
                sequence: *presentation_sequence,
                value: presentation,
            });
            live.text.push(LegacyTextLease {
                sequence: *capture_sequence,
                lease_id,
                byte_len: text.len().try_into().map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                        "message length cannot be represented by the ABI",
                    )
                })?,
                source_ref: "minori.sc.message".into(),
            });
        }
        if let Some(MinoriVmEvent::Choice {
            sequence,
            option_hashes: _,
            selected_index: _,
        }) = &event
        {
            control_events.push(append_choice_live_output(
                session,
                &vfs,
                input.tick_index,
                *sequence,
                &mut live,
            )?);
        }
        if let Some(firefly_event) = &event {
            if matches!(
                firefly_event,
                MinoriVmEvent::Firefly(_) | MinoriVmEvent::FireflyCleared { .. }
            ) {
                live.resource_scenes.push(firefly_event_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    firefly_event,
                )?);
            }
        }
        if let Some(secondary_effect) = &event {
            if matches!(
                secondary_effect,
                MinoriVmEvent::SecondaryEffect(_) | MinoriVmEvent::SecondaryEffectCleared { .. }
            ) {
                live.resource_scenes
                    .push(secondary_effect_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        secondary_effect,
                    )?);
            }
        }
        if let Some(MinoriVmEvent::ScreenShake(screen_shake)) = &event {
            live.resource_scenes.push(screen_shake_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                screen_shake,
            )?);
        }
        if let Some(MinoriVmEvent::Stage(stage)) = &event {
            let stage_size = session.stage_size.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_SIZE",
                    "stage presentation requires explicit host dimensions",
                )
            })?;
            let sequence = session.vm.state().effect_sequence;
            let frame = match describe_stage_frame(
                &vfs,
                &session.mount_set_id,
                session.vm.state(),
                stage,
                stage_size,
            )
            .and_then(|mut frame| {
                if let Some(wscroll2) = session.vm.state().wscroll2.as_ref() {
                    apply_wscroll2_to_frame(&vfs, &session.mount_set_id, &mut frame, wscroll2)?;
                }
                append_secondary_effect_to_frame(
                    &vfs,
                    &session.mount_set_id,
                    session.vm.state(),
                    &mut frame,
                )?;
                apply_screen_shake_to_frame(session.vm.state(), &mut frame)?;
                frame.validate()?;
                Ok(frame)
            }) {
                Ok(frame) => frame,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            live.resource_scenes.push(LegacySequenced {
                sequence,
                value: frame,
            });
        }
        if let Some(MinoriVmEvent::AxisScroll(frame)) = &event {
            live.resource_scenes.push(axis_scroll_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::LinearScroll(frame)) = &event {
            live.resource_scenes.push(linear_scroll_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::ScrollXf(frame)) = &event {
            live.resource_scenes.push(scroll_xf_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::WScroll2(frame)) = &event {
            live.resource_scenes.push(wscroll2_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::Character(frame)) = &event {
            live.resource_scenes.push(character_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::Effect(frame)) = &event {
            let effect = match effect_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            live.resource_scenes.push(effect);
        }
        if let Some(MinoriVmEvent::Panel { sequence }) = &event {
            let panel = match panel_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                *sequence,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            live.resource_scenes.push(panel);
        }
        if let Some(MinoriVmEvent::Movie(movie)) = &event {
            tracing::info!(
                target: "astra_emu_minori::media",
                event = "astra_emu_minori_movie_requested",
                skippable = movie.skippable,
                control_pressed = session.vm.state().system_ui.control_pressed,
                control_enabled = session.vm.state().system_ui.control_enabled,
                skip_enabled = session.vm.state().system_ui.skip_enabled,
                "requested bounded modal movie playback"
            );
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            let movie = movie_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                movie,
                sequence,
            )?;
            live.video.push(movie);
        }
        if started_game && live.resource_scenes.is_empty() {
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            let (width, height) = session.stage_size.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_STAGE_SIZE",
                    "leaving the system UI requires explicit host dimensions",
                )
            })?;
            let frame = LegacyRenderResourceFrameV1 {
                width,
                height,
                texture_resources: Vec::new(),
                draws: Vec::new(),
            };
            frame.validate()?;
            live.resource_scenes.push(LegacySequenced {
                sequence,
                value: frame,
            });
        }
        let mut audio_command_count = u64::try_from(live.audio_commands.len()).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_AUDIO_COMMAND_BOUNDS",
                "restored audio command count cannot be represented",
            )
        })?;
        let event_audio_commands = match &event {
            Some(MinoriVmEvent::Audio { commands })
            | Some(MinoriVmEvent::Message {
                audio_commands: commands,
                ..
            }) => Some(commands),
            _ => None,
        };
        if let Some(commands) = event_audio_commands {
            for command in commands {
                let (sequence, command) = map_audio_command(command, session.vm.state())?;
                if let LegacyAudioCommandV1::LoadResource { resource_uri, .. } = &command {
                    let stat = match vfs.stat_file(&session.mount_set_id, resource_uri) {
                        Ok(stat) if stat.len > 0 && stat.len <= MAX_RESOURCE_BYTES => stat,
                        Ok(_) => {
                            session.poisoned = true;
                            return Err(invalid(
                                "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                                "audio resource is empty or exceeds the session bound",
                            ));
                        }
                        Err(error) => {
                            tracing::debug!(
                                target: "astra_emu_minori::resource",
                                event = "astra_emu_minori_audio_resource_stat_failed",
                                resource_identity = %Hash256::from_sha256(resource_uri.as_bytes()),
                                diagnostic = %error.code(),
                                "audio resource stat failed"
                            );
                            session.poisoned = true;
                            return Err(error);
                        }
                    };
                    let _ = stat;
                }
                if let Err(error) = command.validate() {
                    session.poisoned = true;
                    return Err(error);
                }
                live.audio_commands.push(LegacySequenced {
                    sequence,
                    value: command,
                });
                audio_command_count += 1;
            }
        }
        let waits = match &event {
            Some(MinoriVmEvent::Wait(wait)) | Some(MinoriVmEvent::Message { wait, .. }) => {
                vec![legacy_wait(wait)]
            }
            Some(MinoriVmEvent::Choice { .. }) => {
                let wait = session
                    .vm
                    .state()
                    .wait
                    .as_ref()
                    .ok_or_else(|| runtime_error(MinoriRuntimeError::Choice))?;
                vec![legacy_wait(wait)]
            }
            Some(MinoriVmEvent::Movie(movie)) => vec![LegacyWaitRequest::MediaFence {
                token_id: movie.fence_id.clone(),
                media_id: movie.media_id.clone(),
            }],
            _ => Vec::new(),
        };
        let status = match &event {
            Some(MinoriVmEvent::Wait(_))
            | Some(MinoriVmEvent::Message { .. })
            | Some(MinoriVmEvent::Choice { .. }) => LegacyRuntimeStatus::Awaiting,
            Some(MinoriVmEvent::Chain { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Audio { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Stage(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::AxisScroll(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::LinearScroll(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::ScrollXf(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::WScroll2(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Character(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Effect(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::EffectCleared { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Firefly(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::FireflyCleared { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::SecondaryEffect(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::SecondaryEffectCleared { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::ScreenShake(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Panel { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Movie(_)) => LegacyRuntimeStatus::Awaiting,
            Some(MinoriVmEvent::Terminal) if session.vm.state().terminal => {
                LegacyRuntimeStatus::Terminal
            }
            Some(MinoriVmEvent::Terminal) => LegacyRuntimeStatus::Active,
            None => LegacyRuntimeStatus::Active,
        };
        let trace = (after > before)
            .then(|| LegacyTraceEntry {
                sequence: after,
                context_id: 0,
                pc: session.vm.state().pc_line as u64,
                opcode: "minori.sc".into(),
                action: match &event {
                    Some(MinoriVmEvent::Chain { .. }) => Some("chain".into()),
                    Some(MinoriVmEvent::Audio { .. }) => Some("audio".into()),
                    Some(MinoriVmEvent::Stage(_)) => Some("stage".into()),
                    Some(MinoriVmEvent::AxisScroll(_)) => Some("axis_scroll".into()),
                    Some(MinoriVmEvent::LinearScroll(_)) => Some("linear_scroll".into()),
                    Some(MinoriVmEvent::ScrollXf(_)) => Some("scroll_xf".into()),
                    Some(MinoriVmEvent::WScroll2(_)) => Some("wscroll2".into()),
                    Some(MinoriVmEvent::Character(_)) => Some("character".into()),
                    Some(MinoriVmEvent::Effect(_)) => Some("effect".into()),
                    Some(MinoriVmEvent::EffectCleared { .. }) => Some("effect_clear".into()),
                    Some(MinoriVmEvent::Firefly(_)) => Some("firefly".into()),
                    Some(MinoriVmEvent::FireflyCleared { .. }) => Some("firefly_clear".into()),
                    Some(MinoriVmEvent::SecondaryEffect(_)) => Some("secondary_effect".into()),
                    Some(MinoriVmEvent::SecondaryEffectCleared { .. }) => {
                        Some("secondary_effect_clear".into())
                    }
                    Some(MinoriVmEvent::ScreenShake(_)) => Some("screen_shake".into()),
                    Some(MinoriVmEvent::Panel { .. }) => Some("panel".into()),
                    Some(MinoriVmEvent::Choice { .. }) => Some("choice".into()),
                    Some(MinoriVmEvent::Movie(_)) => Some("movie".into()),
                    Some(MinoriVmEvent::Terminal) if !session.vm.state().terminal => {
                        Some("route_complete".into())
                    }
                    _ => None,
                },
                yield_reason: waits.first().map(|_| "wait".into()),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut output = LegacyStepOutput {
            status,
            live,
            control: LegacyControlTransaction {
                events: control_events,
                waits,
                ..LegacyControlTransaction::default()
            },
            trace,
            diagnostics: Vec::new(),
            coverage: LegacyCoverageDelta {
                instructions: after - before,
                contexts: vec![0],
                audio_commands: audio_command_count,
                ..LegacyCoverageDelta::default()
            },
            state_revision: session.vm.state().fixed_tick,
        };
        let reported_system_page = append_system_page_observation(session, &mut output.control)?;
        let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
        let reported_gallery_unlock_count =
            append_gallery_unlock_observation(session, &mut output.control)?;
        let reported_choice_active =
            append_choice_active_observation(session, &mut output.control)?;
        if matches!(event, Some(MinoriVmEvent::Terminal)) && !session.vm.state().terminal {
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            output.control.blackboard.push(LegacyBlackboardMutation {
                sequence,
                key: "minori.route_complete".into(),
                value: "true".into(),
            });
        }
        append_global_progress_store(session, &mut output)?;
        let restored_presentation =
            append_restored_gameplay_scene(session, &vfs, &mut output.live)?;
        output.validate(&input.budget)?;
        if let Some(page) = reported_system_page {
            session.reported_system_page = Some(page);
        }
        if let Some(mode) = reported_play_mode {
            session.reported_play_mode = Some(mode);
        }
        if let Some(count) = reported_gallery_unlock_count {
            session.reported_gallery_unlock_count = Some(count);
        }
        if let Some(active) = reported_choice_active {
            session.reported_choice_active = Some(active);
        }
        if restored_presentation {
            session.restore_presentation_pending = false;
        }
        Ok(output)
    }

    fn save(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacySnapshotEnvelope, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get(session_id.0.as_str())
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_POISONED",
                "poisoned session cannot be saved",
            ));
        }
        if session.global_progress.pending.is_some() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SAVE_PENDING",
                "session cannot be saved while global progress storage is pending",
            ));
        }
        let bytes = session.vm.snapshot_bytes().map_err(runtime_error)?;
        let global_progress = postcard::to_allocvec(&MinoriGlobalProgressSnapshotV1 {
            schema: MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA.into(),
            enabled: session.global_progress.enabled,
            loaded: session.global_progress.loaded,
            persisted_unlocks: session.global_progress.persisted_unlocks.clone(),
        })
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SNAPSHOT_ENCODE",
                "global progress snapshot could not be encoded",
            )
        })?;
        let envelope = LegacySnapshotEnvelope {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            session_id: session_id.clone(),
            schema_version: SchemaVersion::new(6, 0, 0),
            case_fingerprint: session.case_fingerprint,
            fixed_step: session.vm.state().fixed_tick,
            session_seed: session.session_seed,
            runtime_cursor: session.vm.state().instruction_count,
            family_sections: vec![
                LegacySnapshotSection {
                    section_id: "minori.runtime".into(),
                    schema: MINORI_RUNTIME_STATE_SCHEMA.into(),
                    version: SchemaVersion::new(23, 0, 0),
                    bytes,
                },
                LegacySnapshotSection {
                    section_id: "minori.global_progress".into(),
                    schema: MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA.into(),
                    version: SchemaVersion::new(1, 0, 0),
                    bytes: global_progress,
                },
            ],
            redaction_status: "passed".into(),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    fn restore(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        snapshot: &LegacySnapshotEnvelope,
    ) -> Result<LegacyRestoreReport, LegacyProviderError> {
        ctx.validate()?;
        snapshot.validate()?;
        let vfs = Arc::clone(self.vfs()?);
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.global_progress.pending.is_some() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_RESTORE_PENDING",
                "session cannot be restored while global progress storage is pending",
            ));
        }
        if snapshot.family_id.0 != MINORI_FAMILY_ID
            || snapshot.session_id != *session_id
            || snapshot.case_fingerprint != session.case_fingerprint
            || snapshot.session_seed != session.session_seed
            || snapshot.family_sections.len() != 2
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SNAPSHOT_IDENTITY",
                "snapshot identity does not match the open session",
            ));
        }
        let runtime_section = &snapshot.family_sections[0];
        let progress_section = &snapshot.family_sections[1];
        if runtime_section.section_id != "minori.runtime"
            || runtime_section.schema != MINORI_RUNTIME_STATE_SCHEMA
            || runtime_section.version != SchemaVersion::new(23, 0, 0)
            || progress_section.section_id != "minori.global_progress"
            || progress_section.schema != MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA
            || progress_section.version != SchemaVersion::new(1, 0, 0)
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SNAPSHOT_SECTION",
                "snapshot runtime section identity is invalid",
            ));
        }
        let restored = MinoriVm::decode_snapshot(&runtime_section.bytes).map_err(runtime_error)?;
        let restored_progress: MinoriGlobalProgressSnapshotV1 =
            postcard::from_bytes(&progress_section.bytes).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SNAPSHOT_DECODE",
                    "global progress snapshot could not be decoded",
                )
            })?;
        if restored_progress.schema != MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA
            || restored_progress.enabled != session.global_progress.enabled
            || (!restored_progress.loaded && !restored_progress.persisted_unlocks.is_empty())
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SNAPSHOT_IDENTITY",
                "global progress snapshot identity or state is invalid",
            ));
        }
        validate_script_uri(&restored.script_uri)?;
        let bytes = vfs.read_file(&ctx.mount_set_id, &restored.script_uri, MAX_SCRIPT_BYTES)?;
        let script_hash = Hash256::from_sha256(&bytes);
        if script_hash != restored.script_hash {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SNAPSHOT_SCRIPT_IDENTITY",
                "snapshot script hash does not match the mounted VFS",
            ));
        }
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        session
            .vm
            .replace_script(restored.script_uri, script_hash, script)
            .and_then(|_| session.vm.restore_state(&runtime_section.bytes))
            .map_err(runtime_error)?;
        session.global_progress.loaded = restored_progress.loaded;
        session.global_progress.persisted_unlocks = restored_progress.persisted_unlocks;
        session.global_progress.pending = None;
        if session.global_progress.enabled && session.global_progress.loaded {
            session
                .vm
                .merge_verified_gallery_unlocks(&session.global_progress.persisted_unlocks)
                .map_err(runtime_error)?;
        }
        session.ephemeral_text.clear();
        session.restore_audio_pending = true;
        session.restore_presentation_pending = true;
        session.reported_system_page = None;
        session.reported_play_mode = None;
        session.reported_gallery_unlock_count = None;
        session.reported_choice_active = None;
        session.poisoned = false;
        Ok(LegacyRestoreReport {
            restored_fixed_step: session.vm.state().fixed_tick,
            session_seed: session.session_seed,
            state_revision: session.vm.state().fixed_tick,
            diagnostics: Vec::new(),
        })
    }

    fn take_ephemeral_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<astra_emu_family_api::LegacyEphemeralText>, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        Ok(session.ephemeral_text.remove(lease_id))
    }

    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<OwnedByteBuffer, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if max_bytes == 0 || max_bytes > MAX_RESOURCE_BYTES || !resource_uri.starts_with("minori:/")
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_BOUNDS",
                "resource request is outside the session VFS or byte budget",
            ));
        }
        self.vfs()?
            .read_file(&ctx.mount_set_id, resource_uri, max_bytes)
    }

    fn begin_session_resource_read(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<LegacyResourceRead, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if max_bytes == 0 || max_bytes > MAX_RESOURCE_BYTES || !resource_uri.starts_with("minori:/")
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_BOUNDS",
                "resource request is outside the session VFS or byte budget",
            ));
        }
        let vfs = Arc::clone(self.vfs()?);
        let mount_set_id = ctx.mount_set_id.clone();
        let resource_uri = resource_uri.to_owned();
        LegacyResourceRead::spawn(move || vfs.read_file(&mount_set_id, &resource_uri, max_bytes))
    }

    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .remove(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, &session)?;
        Ok(LegacyShutdownReport {
            final_state_revision: session.vm.state().fixed_tick,
            instruction_count: session.vm.state().instruction_count,
            syscall_count: 0,
            evidence_vm_trace: session
                .evidence_vm_trace
                .into_iter()
                .map(
                    |(context_id, program_counter, opcode)| LegacyVmTraceRecord {
                        context_id,
                        program_counter,
                        opcode,
                    },
                )
                .collect(),
            diagnostics: Vec::new(),
        })
    }
}

fn append_restored_gameplay_scene(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    live: &mut LegacyLiveOutput,
) -> Result<bool, LegacyProviderError> {
    if !session.restore_presentation_pending {
        return Ok(false);
    }
    if !live.resource_scenes.is_empty() {
        return Ok(true);
    }
    let Some(stage_size) = session.stage_size else {
        return Ok(false);
    };
    let scene_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let frame = if session.vm.state().firefly.is_some() {
        describe_firefly_frame(vfs, &session.mount_set_id, session.vm.state(), stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            &session.mount_set_id,
            session.vm.state(),
            &visible_effect_frame(session.vm.state(), scene_sequence)?,
            stage_size,
        )?
    };
    live.resource_scenes.push(LegacySequenced {
        sequence: scene_sequence,
        value: frame,
    });
    Ok(true)
}

fn record_evidence_commands(
    session: &mut MinoriSession,
    commands: Vec<MinoriExecutedCommand>,
) -> Result<(), LegacyProviderError> {
    for command in commands {
        let context_id = u32::from_be_bytes(
            command.script_hash.as_bytes()[..4]
                .try_into()
                .expect("a SHA-256 prefix is exactly four bytes"),
        );
        if let Some(existing) = session.evidence_contexts.get(&context_id) {
            if *existing != command.script_hash {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_EVIDENCE_CONTEXT_COLLISION",
                    "two scripts mapped to the same bounded evidence context",
                ));
            }
        } else {
            session
                .evidence_contexts
                .insert(context_id, command.script_hash);
        }
        let opcode = minori_evidence_opcode(&command.opcode).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_EVIDENCE_OPCODE",
                "executed command has no stable evidence opcode identity",
            )
        })?;
        session
            .evidence_vm_trace
            .insert((context_id, command.command_ordinal, opcode));
    }
    Ok(())
}

fn minori_evidence_opcode(opcode: &str) -> Option<u8> {
    Some(match opcode {
        "message" => 1,
        "transition" => 2,
        "stage" => 3,
        "panel" => 4,
        "playbgm" => 5,
        "char" => 6,
        "playse" => 7,
        "wait" => 8,
        "playse2" => 9,
        "pragma" => 10,
        "setglobal" => 11,
        "set" => 12,
        "playse3" => 13,
        "effect" => 14,
        "movie" => 15,
        "playvoice" => 16,
        "shakescreen" => 17,
        "endscroll" => 18,
        "vscroll" => 19,
        "scrollxf" => 20,
        "effect2" => 21,
        "hscroll" => 22,
        "scroll" => 23,
        "label" => 24,
        "goto" => 25,
        "if" => 26,
        "chain" => 27,
        "end" => 28,
        "select" => 29,
        _ => return None,
    })
}

fn map_audio_command(
    command: &MinoriAudioCommand,
    state: &MinoriRuntimeState,
) -> Result<(u64, LegacyAudioCommandV1), LegacyProviderError> {
    match command {
        MinoriAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding,
            resource_uri,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::LoadResource {
                stream_id: *stream_id,
                encoding: map_audio_encoding(*encoding),
                resource_uri: resource_uri.clone(),
            },
        )),
        MinoriAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::Play {
                stream_id: *stream_id,
                volume: effective_audio_volume(state, *stream_id, *volume)?,
                pan: *pan,
                repeat: *repeat,
                fade_in_ms: *fade_in_ms,
            },
        )),
        MinoriAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::Stop {
                stream_id: *stream_id,
                fade_ms: *fade_ms,
            },
        )),
        MinoriAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::SetParams {
                stream_id: *stream_id,
                volume: effective_audio_volume(state, *stream_id, *volume)?,
                pan: *pan,
                repeat: *repeat,
            },
        )),
    }
}

fn map_audio_encoding(encoding: MinoriAudioEncoding) -> LegacyAudioEncoding {
    match encoding {
        MinoriAudioEncoding::Ogg => LegacyAudioEncoding::Ogg,
        MinoriAudioEncoding::Wav => LegacyAudioEncoding::Wav,
    }
}

fn effective_audio_volume(
    state: &MinoriRuntimeState,
    stream_id: u32,
    base_volume: f32,
) -> Result<f32, LegacyProviderError> {
    if !base_volume.is_finite() || !(0.0..=1.0).contains(&base_volume) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_AUDIO_VOLUME",
            "audio volume is outside the verified normalized range",
        ));
    }
    let audio = state.audio.get(&stream_id).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_AUDIO_STREAM_STATE",
            "audio command has no matching runtime stream state",
        )
    })?;
    let config = state
        .system_ui
        .config_draft
        .as_ref()
        .unwrap_or(&state.system_ui.config);
    let (volume, muted) = match audio.bus.as_str() {
        "bgm" => (config.bgm_volume, config.bgm_muted),
        "voice" => (config.voice_volume, config.voice_muted),
        "se" | "se2" | "se3" => (config.se_volume, config.se_muted),
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_AUDIO_BUS",
                "audio stream has an unsupported bus identity",
            ));
        }
    };
    Ok(if muted {
        0.0
    } else {
        base_volume * (f32::from(volume) / 100.0)
    })
}

fn take_restore_audio_commands(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
) -> Result<Vec<LegacySequenced<LegacyAudioCommandV1>>, LegacyProviderError> {
    if !session.restore_audio_pending {
        return Ok(Vec::new());
    }
    let active = session
        .vm
        .state()
        .audio
        .iter()
        .filter(|(_, state)| state.playing)
        .map(|(stream_id, state)| (*stream_id, state.clone()))
        .collect::<Vec<_>>();
    for (_, state) in &active {
        match vfs.stat_file(&session.mount_set_id, &state.resource_uri) {
            Ok(stat) if stat.len > 0 && stat.len <= MAX_RESOURCE_BYTES => {}
            Ok(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                    "restored audio resource is empty or exceeds the session bound",
                ));
            }
            Err(error) => return Err(error),
        }
    }
    let mut commands = Vec::with_capacity(active.len().saturating_mul(2));
    for (stream_id, state) in active {
        let load_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        commands.push(LegacySequenced {
            sequence: load_sequence,
            value: LegacyAudioCommandV1::LoadResource {
                stream_id,
                encoding: map_audio_encoding(state.encoding),
                resource_uri: state.resource_uri,
            },
        });
        let play_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        commands.push(LegacySequenced {
            sequence: play_sequence,
            value: LegacyAudioCommandV1::Play {
                stream_id,
                volume: effective_audio_volume(
                    session.vm.state(),
                    stream_id,
                    f32::from(state.volume_milli) / 1000.0,
                )?,
                pan: f32::from(state.pan_milli) / 1000.0,
                repeat: state.looped,
                fade_in_ms: 0,
            },
        });
    }
    session.restore_audio_pending = false;
    Ok(commands)
}

fn describe_stage_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    stage: &MinoriStageCommand,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    append_stage_contents(
        vfs,
        mount_set_id,
        stage,
        height,
        &mut texture_resources,
        &mut draws,
    )?;
    append_character_contents(
        vfs,
        mount_set_id,
        &state.characters,
        width,
        height,
        &mut texture_resources,
        &mut draws,
    )?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn apply_scroll_xf_to_frame(
    frame: &mut LegacyRenderResourceFrameV1,
    scroll: &crate::MinoriScrollXfState,
) -> Result<(), LegacyProviderError> {
    let width = scroll.visible_extent[0].min(i32::try_from(frame.width).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SCROLL_XF_BOUNDS",
            "stage width cannot be represented for scrollXF",
        )
    })?);
    let height = scroll.visible_extent[1].min(i32::try_from(frame.height).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SCROLL_XF_BOUNDS",
            "stage height cannot be represented for scrollXF",
        )
    })?);
    if width <= 0 || height <= 0 {
        frame.draws.clear();
        return Ok(());
    }
    let source_left = scroll.visible_offset[0] as f32;
    let source_top = scroll.visible_offset[1] as f32;
    let source_right = source_left + width as f32;
    let source_bottom = source_top + height as f32;
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width,
        height,
    });
    let mut clipped = Vec::with_capacity(frame.draws.len());
    for mut draw in frame.draws.drain(..) {
        let draw_left = draw.vertices[0].position[0];
        let draw_top = draw.vertices[0].position[1];
        let draw_right = draw.vertices[3].position[0];
        let draw_bottom = draw.vertices[3].position[1];
        if ![draw_left, draw_top, draw_right, draw_bottom]
            .iter()
            .all(|value| value.is_finite())
            || draw_right <= draw_left
            || draw_bottom <= draw_top
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SCROLL_XF_DRAW",
                "scrollXF requires bounded axis-aligned stage draws",
            ));
        }
        let left = draw_left.max(source_left);
        let top = draw_top.max(source_top);
        let right = draw_right.min(source_right);
        let bottom = draw_bottom.min(source_bottom);
        if right <= left || bottom <= top {
            continue;
        }
        let u0 = draw.vertices[0].tex_coord[0];
        let v0 = draw.vertices[0].tex_coord[1];
        let u1 = draw.vertices[3].tex_coord[0];
        let v1 = draw.vertices[3].tex_coord[1];
        let map_u = |x: f32| u0 + (u1 - u0) * ((x - draw_left) / (draw_right - draw_left));
        let map_v = |y: f32| v0 + (v1 - v0) * ((y - draw_top) / (draw_bottom - draw_top));
        let color = draw.vertices[0].color;
        let vertex = |x: f32, y: f32, u: f32, v: f32| LegacyVertexV1 {
            position: [x - source_left, y - source_top],
            tex_coord: [u, v],
            color,
        };
        draw.vertices = [
            vertex(left, top, map_u(left), map_v(top)),
            vertex(right, top, map_u(right), map_v(top)),
            vertex(left, bottom, map_u(left), map_v(bottom)),
            vertex(right, bottom, map_u(right), map_v(bottom)),
        ];
        draw.scissor = scissor;
        clipped.push(draw);
    }
    frame.draws = clipped;
    Ok(())
}

fn apply_wscroll2_to_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    frame: &mut LegacyRenderResourceFrameV1,
    scroll: &crate::MinoriWScroll2State,
) -> Result<(), LegacyProviderError> {
    if (frame.width, frame.height) != (1280, 720) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_STAGE_IDENTITY",
            "WScroll2 requires the verified 1280x720 reference stage",
        ));
    }
    let sync_bytes = vfs
        .read_file(
            mount_set_id,
            &scroll.sync_resource_uri,
            MAX_WSCROLL2_SYNC_BYTES,
        )
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_READ",
                "WScroll2 sync resource could not be read",
            )
        })?;
    let _sync_values = parse_wscroll2_sync(&sync_bytes)?;
    let stage_draw_count = frame
        .draws
        .iter()
        .filter(|draw| draw.texture_id < 100)
        .count();
    if stage_draw_count != 2 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_STAGE_LAYERS",
            "WScroll2 requires one far panorama and one near panorama",
        ));
    }
    let mut wrapped = Vec::with_capacity(frame.draws.len() + 2);
    for draw in frame.draws.drain(..) {
        if draw.texture_id >= 100 {
            wrapped.push(draw);
            continue;
        }
        let resource = frame
            .texture_resources
            .iter()
            .find(|resource| resource.texture_id == draw.texture_id)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_WSCROLL2_TEXTURE",
                    "WScroll2 stage draw has no bound texture descriptor",
                )
            })?;
        if resource.decoded_width < frame.width || resource.decoded_height < frame.height {
            return Err(invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_PANORAMA_BOUNDS",
                "WScroll2 panorama is smaller than the reference viewport",
            ));
        }
        let offset = if draw.texture_id == 0 {
            scroll.background_offset
        } else {
            scroll.foreground_offset
        };
        append_wrapped_panorama_draw(
            &mut wrapped,
            &draw,
            resource,
            frame.width,
            frame.height,
            offset,
        )?;
    }
    frame.draws = wrapped;
    Ok(())
}

fn parse_wscroll2_sync(bytes: &[u8]) -> Result<Vec<i32>, LegacyProviderError> {
    let source = std::str::from_utf8(bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_SYNC_ENCODING",
            "WScroll2 sync resource is not bounded ASCII text",
        )
    })?;
    let mut values = Vec::new();
    for line in source.lines() {
        let token = line.trim();
        if token.is_empty() || token.starts_with(';') {
            continue;
        }
        if values.len() >= MAX_WSCROLL2_SYNC_VALUES {
            return Err(invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_BOUNDS",
                "WScroll2 sync resource exceeds the value limit",
            ));
        }
        let value = token.parse::<i32>().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_VALUE",
                "WScroll2 sync resource contains a non-integer row",
            )
        })?;
        if !(-16_384..=16_384).contains(&value) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_VALUE",
                "WScroll2 sync value exceeds the verified bound",
            ));
        }
        values.push(value);
    }
    if values.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_SYNC_EMPTY",
            "WScroll2 sync resource contains no values",
        ));
    }
    Ok(values)
}

fn append_wrapped_panorama_draw(
    output: &mut Vec<LegacyDrawV1>,
    template: &LegacyDrawV1,
    resource: &LegacyTextureResourceV1,
    frame_width: u32,
    frame_height: u32,
    offset: i64,
) -> Result<(), LegacyProviderError> {
    let source_width = i64::from(resource.decoded_width);
    let source_x = offset.rem_euclid(source_width);
    let first_width = (source_width - source_x).min(i64::from(frame_width));
    let second_width = i64::from(frame_width) - first_width;
    let mut append_segment = |source_left: i64, output_left: i64, width: i64| {
        if width == 0 {
            return Ok(());
        }
        let source_right = source_left.checked_add(width).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                "panorama range overflowed",
            )
        })?;
        let output_right = output_left.checked_add(width).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                "viewport range overflowed",
            )
        })?;
        let u0 = source_left as f32 / resource.decoded_width as f32;
        let u1 = source_right as f32 / resource.decoded_width as f32;
        let v1 = frame_height as f32 / resource.decoded_height as f32;
        let color = template.vertices[0].color;
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color,
        };
        output.push(LegacyDrawV1 {
            texture_id: template.texture_id,
            vertices: [
                vertex(output_left as f32, 0.0, u0, 0.0),
                vertex(output_right as f32, 0.0, u1, 0.0),
                vertex(output_left as f32, frame_height as f32, u0, v1),
                vertex(output_right as f32, frame_height as f32, u1, v1),
            ],
            blend: template.blend,
            texture_filter: template.texture_filter,
            scissor: Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: i32::try_from(frame_width).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                        "viewport width cannot be represented",
                    )
                })?,
                height: i32::try_from(frame_height).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                        "viewport height cannot be represented",
                    )
                })?,
            }),
        });
        Ok::<(), LegacyProviderError>(())
    };
    append_segment(source_x, 0, first_width)?;
    append_segment(0, first_width, second_width)?;
    Ok(())
}

fn describe_effect_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
    stage_size: (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut frame = describe_effect_frame_without_secondary(
        vfs,
        mount_set_id,
        state,
        effect,
        stage_size,
        true,
    )?;
    append_secondary_effect_to_frame(vfs, mount_set_id, state, &mut frame)?;
    apply_screen_shake_to_frame(state, &mut frame)?;
    frame.validate()?;
    Ok(frame)
}

fn describe_effect_frame_without_secondary(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
    (width, height): (u32, u32),
    include_panel: bool,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    if let Some(stage) = state.stage.as_ref() {
        append_stage_contents(
            vfs,
            mount_set_id,
            stage,
            height,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    append_character_contents(
        vfs,
        mount_set_id,
        &state.characters,
        width,
        height,
        &mut texture_resources,
        &mut draws,
    )?;
    let alpha = f32::from(effect.alpha_255) / 255.0;
    if let Some(resource_uri) = &effect.current_resource_uri {
        append_resource_layer(
            vfs,
            mount_set_id,
            resource_uri,
            0,
            0,
            if effect.next_resource_uri.is_some() {
                1.0
            } else {
                1.0 - alpha
            },
            100,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(resource_uri) = &effect.next_resource_uri {
        append_resource_layer(
            vfs,
            mount_set_id,
            resource_uri,
            0,
            0,
            alpha,
            101,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(panel) = state.panel.as_ref().filter(|_| include_panel) {
        if panel.mode != 1 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PANEL_MODE",
                "panel state contains an unverified mode",
            ));
        }
        append_panel_layer(
            vfs,
            mount_set_id,
            &panel.resource_uri,
            height,
            200,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let mut frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    if let Some(scroll_xf) = state.scroll_xf.as_ref() {
        apply_scroll_xf_to_frame(&mut frame, scroll_xf)?;
    }
    if let Some(wscroll2) = state.wscroll2.as_ref() {
        apply_wscroll2_to_frame(vfs, mount_set_id, &mut frame, wscroll2)?;
    }
    frame.validate()?;
    Ok(frame)
}

fn append_panel_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    stage_height: u32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_resource_layer(
        vfs,
        mount_set_id,
        resource_uri,
        0,
        0,
        1.0,
        texture_id,
        texture_resources,
        draws,
    )?;
    let image_height = texture_resources
        .last()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PANEL_RESOURCE",
                "panel texture metadata was not appended",
            )
        })?
        .decoded_height;
    let top = i64::from(stage_height)
        .checked_sub(i64::from(image_height))
        .and_then(|value| value.checked_add(64))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PANEL_POSITION",
                "panel position overflowed the verified coordinate range",
            )
        })?;
    let top = i32::try_from(top).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_POSITION",
            "panel position cannot be represented by the render contract",
        )
    })? as f32;
    let draw = draws.last_mut().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_RESOURCE",
            "panel draw was not appended",
        )
    })?;
    for vertex in &mut draw.vertices {
        vertex.position[1] += top;
    }
    Ok(())
}

fn panel_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "panel presentation requires explicit host dimensions",
        )
    })?;
    let effect = visible_effect_frame(state, sequence)?;
    let frame = describe_effect_frame(vfs, mount_set_id, state, &effect, stage_size)?;
    Ok(LegacySequenced {
        sequence,
        value: frame,
    })
}

fn visible_effect_frame(
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<MinoriEffectFrame, LegacyProviderError> {
    let Some(effect) = &state.effect else {
        return Ok(MinoriEffectFrame {
            sequence,
            current_resource_uri: None,
            next_resource_uri: None,
            alpha_255: 0,
        });
    };
    let current = usize::try_from(effect.visible_current_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_STATE",
            "visible effect resource index cannot be represented",
        )
    })?;
    let next = usize::try_from(effect.visible_next_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_STATE",
            "visible effect resource index cannot be represented",
        )
    })?;
    Ok(MinoriEffectFrame {
        sequence,
        current_resource_uri: effect
            .resources
            .get(current)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_EFFECT_STATE",
                    "visible current effect resource is outside the sequence",
                )
            })?
            .clone(),
        next_resource_uri: effect
            .resources
            .get(next)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_EFFECT_STATE",
                    "visible next effect resource is outside the sequence",
                )
            })?
            .clone(),
        alpha_255: effect.visible_alpha_255,
    })
}

fn effect_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "effect presentation requires explicit host dimensions",
        )
    })?;
    let frame = describe_effect_frame(vfs, mount_set_id, state, effect, stage_size)?;
    Ok(LegacySequenced {
        sequence: effect.sequence,
        value: frame,
    })
}

fn character_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    character: &MinoriCharacterFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "character presentation requires explicit host dimensions",
        )
    })?;
    let frame = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, character.sequence)?,
            stage_size,
        )?
    };
    Ok(LegacySequenced {
        sequence: character.sequence,
        value: frame,
    })
}

fn axis_scroll_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    axis_scroll: &MinoriAxisScrollFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let effect = visible_effect_frame(state, axis_scroll.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &effect)
}

fn linear_scroll_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    linear_scroll: &MinoriLinearScrollFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let effect = visible_effect_frame(state, linear_scroll.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &effect)
}

fn scroll_xf_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    scroll_xf: &MinoriScrollXfFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let effect = visible_effect_frame(state, scroll_xf.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &effect)
}

fn wscroll2_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    frame: &MinoriWScroll2Frame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    if state.wscroll2.is_none() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_STATE",
            "WScroll2 presentation has no active runtime state",
        ));
    }
    let visible = visible_effect_frame(state, frame.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &visible)
}

fn firefly_event_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    event: &MinoriVmEvent,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "Firefly presentation requires explicit host dimensions",
        )
    })?;
    let (sequence, frame) = match event {
        MinoriVmEvent::Firefly(frame) => (
            frame.sequence,
            describe_firefly_frame(vfs, mount_set_id, state, stage_size)?,
        ),
        MinoriVmEvent::FireflyCleared { sequence } => (
            *sequence,
            describe_effect_frame(
                vfs,
                mount_set_id,
                state,
                &MinoriEffectFrame {
                    sequence: *sequence,
                    current_resource_uri: None,
                    next_resource_uri: None,
                    alpha_255: 0,
                },
                stage_size,
            )?,
        ),
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FIREFLY_EVENT",
                "non-Firefly event was sent to the Firefly presentation mapper",
            ));
        }
    };
    Ok(LegacySequenced {
        sequence,
        value: frame,
    })
}

fn secondary_effect_event_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    event: &MinoriVmEvent,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "secondary effect presentation requires explicit host dimensions",
        )
    })?;
    let sequence = match event {
        MinoriVmEvent::SecondaryEffect(MinoriSecondaryEffectFrame { sequence })
        | MinoriVmEvent::SecondaryEffectCleared { sequence } => *sequence,
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SECONDARY_EFFECT_EVENT",
                "non-secondary event was sent to the secondary effect mapper",
            ));
        }
    };
    let frame = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, sequence)?,
            stage_size,
        )?
    };
    Ok(LegacySequenced {
        sequence,
        value: frame,
    })
}

fn screen_shake_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    frame: &MinoriScreenShakeFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    if state.screen_shake.is_none() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCREEN_SHAKE_STATE",
            "screen shake presentation has no active runtime state",
        ));
    }
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "screen shake presentation requires explicit host dimensions",
        )
    })?;
    let rendered = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, frame.sequence)?,
            stage_size,
        )?
    };
    Ok(LegacySequenced {
        sequence: frame.sequence,
        value: rendered,
    })
}

fn describe_firefly_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    if (width, height) != (1280, 720) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_FIREFLY_STAGE_IDENTITY",
            "the verified Firefly effect requires the 1280x720 reference stage",
        ));
    }
    let firefly = state.firefly.as_ref().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_FIREFLY_STATE",
            "Firefly presentation has no active effect state",
        )
    })?;
    if firefly.resources.len() != 3
        || firefly.particles.is_empty()
        || firefly.particles.len() > 256
        || firefly
            .particles
            .iter()
            .any(|particle| usize::from(particle.kind) >= firefly.resources.len())
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_FIREFLY_STATE",
            "Firefly particle state is outside the verified bounds",
        ));
    }
    let base = describe_effect_frame_without_secondary(
        vfs,
        mount_set_id,
        state,
        &MinoriEffectFrame {
            sequence: state.effect_sequence,
            current_resource_uri: None,
            next_resource_uri: None,
            alpha_255: 0,
        },
        (width, height),
        true,
    )?;
    let mut texture_resources = base.texture_resources;
    let mut draws = base.draws;
    let mut sprite_sizes = Vec::with_capacity(firefly.resources.len());
    for (index, resource_uri) in firefly.resources.iter().enumerate() {
        let texture_id = 300u32
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture id overflowed",
                )
            })?;
        let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
        sprite_sizes.push((resource.decoded_width, resource.decoded_height));
        texture_resources.push(resource);
    }
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width: i32::try_from(width).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_FIREFLY_STAGE_IDENTITY",
                "Firefly stage width cannot be represented",
            )
        })?,
        height: i32::try_from(height).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_FIREFLY_STAGE_IDENTITY",
                "Firefly stage height cannot be represented",
            )
        })?,
    });
    let global_alpha = f32::from(firefly.fade_alpha_256) / 256.0;
    for particle in &firefly.particles {
        if !particle.active || particle.opacity_255 == 0 || global_alpha == 0.0 {
            continue;
        }
        let texture_index = usize::from(particle.kind);
        let texture_id = 300u32
            .checked_add(u32::try_from(texture_index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture id overflowed",
                )
            })?;
        let (sprite_width, sprite_height) = sprite_sizes[texture_index];
        let left = particle.position[0] as f32;
        let top = particle.position[1] as f32;
        let right = left + sprite_width as f32;
        let bottom = top + sprite_height as f32;
        let opacity = global_alpha * f32::from(particle.opacity_255) / 255.0;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FIREFLY_ALPHA",
                "Firefly particle alpha is outside the normalized range",
            ));
        }
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, opacity],
        };
        draws.push(LegacyDrawV1 {
            texture_id,
            vertices: [
                vertex(left, top, 0.0, 0.0),
                vertex(right, top, 1.0, 0.0),
                vertex(left, bottom, 0.0, 1.0),
                vertex(right, bottom, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor,
        });
    }
    let mut frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    append_secondary_effect_to_frame(vfs, mount_set_id, state, &mut frame)?;
    apply_screen_shake_to_frame(state, &mut frame)?;
    frame.validate()?;
    Ok(frame)
}

fn append_secondary_effect_to_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    frame: &mut LegacyRenderResourceFrameV1,
) -> Result<(), LegacyProviderError> {
    let Some(effect) = state.secondary_effect.as_ref() else {
        return Ok(());
    };
    if (frame.width, frame.height) != (1280, 720)
        || effect.particles.len() != 50
        || effect.alpha_256 > 256
        || effect
            .particles
            .iter()
            .any(|particle| !particle.active || particle.kind >= 3)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SECONDARY_EFFECT_STATE",
            "secondary effect state is outside the verified SnowH bounds",
        ));
    }
    let mut sprite_sizes = Vec::with_capacity(effect.resources.len());
    for (index, resource_uri) in effect.resources.iter().enumerate() {
        let texture_id = 600u32
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture id overflowed",
                )
            })?;
        let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
        sprite_sizes.push((resource.decoded_width, resource.decoded_height));
        frame.texture_resources.push(resource);
    }
    let alpha = f32::from(effect.alpha_256) / 256.0;
    if alpha == 0.0 {
        return Ok(());
    }
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    });
    for particle in &effect.particles {
        let texture_index = usize::from(particle.kind);
        let texture_id = 600u32
            .checked_add(u32::try_from(texture_index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture id overflowed",
                )
            })?;
        let (sprite_width, sprite_height) = sprite_sizes[texture_index];
        let left = particle.position[0] as f32;
        let top = particle.position[1] as f32;
        let right = left + sprite_width as f32;
        let bottom = top + sprite_height as f32;
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, alpha],
        };
        frame.draws.push(LegacyDrawV1 {
            texture_id,
            vertices: [
                vertex(left, top, 0.0, 0.0),
                vertex(right, top, 1.0, 0.0),
                vertex(left, bottom, 0.0, 1.0),
                vertex(right, bottom, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor,
        });
    }
    Ok(())
}

fn apply_screen_shake_to_frame(
    state: &MinoriRuntimeState,
    frame: &mut LegacyRenderResourceFrameV1,
) -> Result<(), LegacyProviderError> {
    let Some(shake) = state.screen_shake.as_ref() else {
        return Ok(());
    };
    if (frame.width, frame.height) != (1280, 720)
        || !(1..=1280).contains(&shake.amplitude)
        || shake
            .offset
            .iter()
            .any(|value| value.unsigned_abs() > shake.amplitude as u32)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCREEN_SHAKE_STATE",
            "screen shake state is outside the verified render bounds",
        ));
    }
    let offset = [shake.offset[0] as f32, shake.offset[1] as f32];
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    });
    for draw in &mut frame.draws {
        for vertex in &mut draw.vertices {
            vertex.position[0] += offset[0];
            vertex.position[1] += offset[1];
            if !vertex.position[0].is_finite() || !vertex.position[1].is_finite() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SCREEN_SHAKE_DRAW",
                    "screen shake produced a non-finite draw position",
                ));
            }
        }
        // Native Musica shifts the already-composited screen buffer. The
        // source geometry has already been clipped by the family adapters, so
        // the translated result is clipped only to the final viewport.
        draw.scissor = scissor;
    }
    Ok(())
}

fn append_stage_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    layer: &MinoriStageLayer,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_resource_layer(
        vfs,
        mount_set_id,
        &layer.resource_uri,
        layer.x,
        layer.y,
        1.0,
        texture_id,
        texture_resources,
        draws,
    )
}

fn append_stage_contents(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage: &MinoriStageCommand,
    stage_height: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if let Some(background) = &stage.background {
        append_stage_layer(vfs, mount_set_id, background, 1, texture_resources, draws)?;
    }
    for (index, stand) in stage.stands.iter().enumerate() {
        let texture_id = 16u32
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_LAYER_ID",
                    "stand layer index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_LAYER_ID",
                    "stand layer id overflowed the render resource namespace",
                )
            })?;
        append_stand_layer(
            vfs,
            mount_set_id,
            stand,
            stage_height,
            texture_id,
            texture_resources,
            draws,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_character_contents(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    characters: &BTreeMap<u32, MinoriCharacterState>,
    stage_width: u32,
    stage_height: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    let scissor = LegacyScissorV1 {
        x: 0,
        y: 0,
        width: i32::try_from(stage_width).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_BOUNDS",
                "character viewport width cannot be represented",
            )
        })?,
        height: i32::try_from(stage_height).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_BOUNDS",
                "character viewport height cannot be represented",
            )
        })?,
    };
    for character in characters.values().filter(|character| character.visible) {
        let [resource_uri] = character.resource_uris.as_slice() else {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CHARACTER_RESOURCE_COUNT",
                "character presentation requires the verified single-resource form",
            ));
        };
        if !resource_uri.to_ascii_lowercase().ends_with(".png") {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CHARACTER_CODEC",
                "character presentation requires a static PNG resource",
            ));
        }
        let texture_id = MINORI_CHARACTER_TEXTURE_BASE
            .checked_add(character.slot_id)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHARACTER_TEXTURE_ID",
                    "character texture id overflowed",
                )
            })?;
        let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
        let left = i64::from(character.anchor_position[0])
            .checked_sub(i64::from(resource.decoded_width) / 2)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHARACTER_POSITION",
                    "character horizontal anchor overflowed",
                )
            })?;
        let top = i64::from(stage_height)
            .checked_sub(i64::from(resource.decoded_height))
            .and_then(|value| value.checked_sub(i64::from(character.anchor_position[1])))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHARACTER_POSITION",
                    "character bottom-relative anchor overflowed",
                )
            })?;
        let left = i32::try_from(left).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_POSITION",
                "character horizontal anchor cannot be represented",
            )
        })?;
        let top = i32::try_from(top).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_POSITION",
                "character vertical anchor cannot be represented",
            )
        })?;
        append_texture_draw(
            &resource,
            left,
            top,
            f32::from(character.opacity_256) / 256.0,
            draws,
        )?;
        let draw = draws.last_mut().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_RESOURCE",
                "character draw was not appended",
            )
        })?;
        draw.scissor = Some(scissor);
        if !character.positive_orientation {
            for vertex in &mut draw.vertices {
                vertex.tex_coord[0] = 1.0 - vertex.tex_coord[0];
            }
        }
        texture_resources.push(resource);
    }
    Ok(())
}

fn append_stand_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stand: &MinoriStandLayer,
    stage_height: u32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if !stand.resource_uri.to_ascii_lowercase().ends_with(".png") {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_CODEC",
            "verified stand positioning requires a static PNG resource",
        ));
    }
    let resource = read_texture_resource(vfs, mount_set_id, &stand.resource_uri, texture_id)?;
    let left = i64::from(stand.position) - i64::from(resource.decoded_width) / 2;
    let top = i64::from(stage_height) - i64::from(resource.decoded_height);
    let left = i32::try_from(left).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
            "centered stand X position cannot be represented",
        )
    })?;
    let top = i32::try_from(top).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
            "bottom-anchored stand Y position cannot be represented",
        )
    })?;
    append_texture_draw(&resource, left, top, 1.0, draws)?;
    texture_resources.push(resource);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_resource_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    x: i32,
    y: i32,
    opacity: f32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
    append_texture_draw(&resource, x, y, opacity, draws)?;
    texture_resources.push(resource);
    Ok(())
}

fn append_texture_draw(
    resource: &LegacyTextureResourceV1,
    x: i32,
    y: i32,
    opacity: f32,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_EFFECT_ALPHA",
            "effect alpha is outside the normalized bound",
        ));
    }
    let left = x as f32;
    let top = y as f32;
    let right = left + resource.decoded_width as f32;
    let bottom = top + resource.decoded_height as f32;
    let vertex = |x, y, u, v| LegacyVertexV1 {
        position: [x, y],
        tex_coord: [u, v],
        color: [1.0, 1.0, 1.0, opacity],
    };
    draws.push(LegacyDrawV1 {
        texture_id: resource.texture_id,
        vertices: [
            vertex(left, top, 0.0, 0.0),
            vertex(right, top, 1.0, 0.0),
            vertex(left, bottom, 0.0, 1.0),
            vertex(right, bottom, 1.0, 1.0),
        ],
        blend: LegacyBlendMode::Alpha,
        texture_filter: LegacyTextureFilter::Linear,
        scissor: None,
    });
    Ok(())
}

fn read_texture_resource(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    texture_id: u32,
) -> Result<LegacyTextureResourceV1, LegacyProviderError> {
    let stat = vfs
        .stat_file(mount_set_id, resource_uri)
        .inspect_err(|error| {
            tracing::debug!(
                target: "astra_emu_minori::resource",
                event = "astra_emu_minori_resource_stat_failed",
                resource_identity = %Hash256::from_sha256(resource_uri.as_bytes()),
                texture_id,
                diagnostic = %error.code(),
                "resource stat failed"
            );
        })?;
    if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_BOUNDS",
            "stage image is empty or exceeds the resource byte bound",
        ));
    }
    let bytes = vfs
        .read_file_range(
            mount_set_id,
            resource_uri,
            stat.revision,
            ByteRange {
                offset: 0,
                len: stat.len,
            },
            MAX_RESOURCE_BYTES,
        )
        .inspect_err(|error| {
            tracing::debug!(
                target: "astra_emu_minori::resource",
                event = "astra_emu_minori_resource_read_failed",
                resource_identity = %Hash256::from_sha256(resource_uri.as_bytes()),
                texture_id,
                diagnostic = %error.code(),
                "resource read failed"
            );
        })?
        .bytes;
    let image_reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_STAGE_IMAGE_FORMAT",
                "stage image format could not be determined",
            )
        })?;
    let (image_width, image_height) = image_reader.into_dimensions().map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_METADATA",
            "stage image dimensions could not be read safely",
        )
    })?;
    if image_width == 0 || image_height == 0 || image_width > 16_384 || image_height > 16_384 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_BOUNDS",
            "stage image dimensions are outside the supported bound",
        ));
    }
    let codec = image_codec(resource_uri)?;
    let revision = texture_binding_revision(resource_uri, stat.revision.0);
    Ok(LegacyTextureResourceV1 {
        texture_id,
        resource_uri: resource_uri.to_owned(),
        codec: codec.into(),
        revision,
        decoded_width: image_width,
        decoded_height: image_height,
        decoded_format: LegacyTextureFormat::Rgba8,
    })
}

fn texture_binding_revision(resource_uri: &str, source_revision: u64) -> u64 {
    let mut identity = Vec::with_capacity(resource_uri.len() + std::mem::size_of::<u64>());
    identity.extend_from_slice(&source_revision.to_le_bytes());
    identity.extend_from_slice(resource_uri.as_bytes());
    let mut revision = u64::from_le_bytes(
        Hash256::from_sha256(&identity).as_bytes()[..8]
            .try_into()
            .expect("sha256 prefix has a fixed width"),
    );
    if revision == 0 {
        revision = 1;
    }
    revision
}

fn image_codec(resource_uri: &str) -> Result<&'static str, LegacyProviderError> {
    let extension = resource_uri
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        Ok("png")
    } else if extension.eq_ignore_ascii_case("bmp") {
        Ok("bmp")
    } else if extension.eq_ignore_ascii_case("jpg") {
        Ok("jpg")
    } else if extension.eq_ignore_ascii_case("jpeg") {
        Ok("jpeg")
    } else if extension.eq_ignore_ascii_case("webp") {
        Ok("webp")
    } else {
        Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_CODEC",
            "stage image extension has no explicitly bound decode codec",
        ))
    }
}

fn load_script(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    target: &str,
) -> Result<(String, Hash256, crate::ScScript), LegacyProviderError> {
    let script_uri = format!("minori:/scr/{target}");
    validate_script_uri(&script_uri)?;
    let bytes = vfs
        .read_file(mount_set_id, &script_uri, MAX_SCRIPT_BYTES)
        .inspect_err(|error| {
            tracing::debug!(
                target: "astra_emu_minori::resource",
                event = "astra_emu_minori_chain_script_read_failed",
                resource_identity = %Hash256::from_sha256(script_uri.as_bytes()),
                diagnostic = %error.code(),
                "chain script read failed"
            );
        })?;
    let script_hash = Hash256::from_sha256(&bytes);
    let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
    Ok((script_uri, script_hash, script))
}

fn validate_script_uri(script_uri: &str) -> Result<(), LegacyProviderError> {
    let Some(target) = script_uri.strip_prefix("minori:/scr/") else {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_URI",
            "script URI is outside the Minori script mount",
        ));
    };
    if target.is_empty()
        || target.len() > 256
        || !target.to_ascii_lowercase().ends_with(".sc")
        || target.contains('/')
        || target.contains('\\')
        || target.contains("..")
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_URI",
            "script URI contains an invalid direct-entry name",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinoriSystemUiAction {
    Present,
    PresentWithAudioRefresh,
    PresentAfterConfigClose,
    PresentWithAudioTest(MinoriConfigAudioBus),
    StartGame,
    CloseBacklog,
    ReplayBacklogVoice,
    Exit,
}

fn system_ui_action_name(action: MinoriSystemUiAction) -> &'static str {
    match action {
        MinoriSystemUiAction::Present => "present",
        MinoriSystemUiAction::PresentWithAudioRefresh => "present_with_audio_refresh",
        MinoriSystemUiAction::PresentAfterConfigClose => "present_after_config_close",
        MinoriSystemUiAction::PresentWithAudioTest(_) => "present_with_audio_test",
        MinoriSystemUiAction::StartGame => "start_game",
        MinoriSystemUiAction::CloseBacklog => "close_backlog",
        MinoriSystemUiAction::ReplayBacklogVoice => "replay_backlog_voice",
        MinoriSystemUiAction::Exit => "exit",
    }
}

fn system_page_name(page: MinoriSystemPage) -> &'static str {
    match page {
        MinoriSystemPage::None => "none",
        MinoriSystemPage::Title => "title",
        MinoriSystemPage::Load => "load",
        MinoriSystemPage::Save => "save",
        MinoriSystemPage::Config => "config",
        MinoriSystemPage::Backlog => "backlog",
        MinoriSystemPage::Memories => "memories",
        MinoriSystemPage::GalleryCg => "gallery_cg",
        MinoriSystemPage::GalleryBgm => "gallery_bgm",
        MinoriSystemPage::GalleryReplay => "gallery_replay",
        MinoriSystemPage::GalleryMovie => "gallery_movie",
    }
}

fn append_system_page_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<MinoriSystemPage>, LegacyProviderError> {
    let page = session.vm.state().system_ui.page;
    if session.reported_system_page == Some(page) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.system_page".into(),
        value: system_page_name(page).into(),
    });
    Ok(Some(page))
}

fn play_mode_name(mode: MinoriPlayMode) -> &'static str {
    match mode {
        MinoriPlayMode::Normal => "normal",
        MinoriPlayMode::Auto => "auto",
        MinoriPlayMode::Skip => "skip",
    }
}

fn append_play_mode_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<MinoriPlayMode>, LegacyProviderError> {
    let mode = session.vm.state().system_ui.play_mode;
    if session.reported_play_mode == Some(mode) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.play_mode".into(),
        value: play_mode_name(mode).into(),
    });
    Ok(Some(mode))
}

fn append_gallery_unlock_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<usize>, LegacyProviderError> {
    let count = session.vm.state().gallery_unlocks.len();
    if session.reported_gallery_unlock_count == Some(count) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.gallery_unlock_count".into(),
        value: count.to_string(),
    });
    Ok(Some(count))
}

fn append_choice_active_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<bool>, LegacyProviderError> {
    let active = matches!(
        session.vm.state().wait.as_ref(),
        Some(MinoriWaitState::Choice { .. })
    );
    if session.reported_choice_active == Some(active) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.choice_active".into(),
        value: active.to_string(),
    });
    Ok(Some(active))
}

fn encode_global_progress(unlocks: &[Hash256]) -> Result<Vec<u8>, LegacyProviderError> {
    let progress = MinoriGlobalProgressV1 {
        schema: MINORI_GLOBAL_PROGRESS_SCHEMA.into(),
        gallery_unlocks: unlocks.to_vec(),
    };
    postcard::to_allocvec(&progress).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_ENCODE",
            "global progress payload could not be encoded",
        )
    })
}

fn decode_global_progress(bytes: &[u8]) -> Result<Vec<Hash256>, LegacyProviderError> {
    let progress: MinoriGlobalProgressV1 = postcard::from_bytes(bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_DECODE",
            "global progress payload could not be decoded",
        )
    })?;
    if progress.schema != MINORI_GLOBAL_PROGRESS_SCHEMA {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SCHEMA",
            "global progress payload schema is unsupported",
        ));
    }
    Ok(progress.gallery_unlocks)
}

fn begin_global_progress_load(
    session: &mut MinoriSession,
    input: &LegacyStepInput,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    if !session.global_progress.enabled
        || session.global_progress.loaded
        || session.global_progress.pending.is_some()
        || !input.await_results.is_empty()
        || !input.provider_results.is_empty()
        || input
            .input_edges
            .iter()
            .any(|edge| edge.control != MINORI_CONTROL_KEY)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_LOAD_STATE",
            "global progress load was requested in an invalid session state",
        ));
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let token_id = format!("minori.global.progress.load.token.{sequence}");
    let request_id = format!("minori.global.progress.load.request.{sequence}");
    session.global_progress.pending = Some(MinoriGlobalProgressRequest::Load {
        request_id: request_id.clone(),
    });
    session
        .vm
        .advance_provider_tick(input.tick_index)
        .map_err(runtime_error)?;
    let output = LegacyStepOutput {
        status: LegacyRuntimeStatus::Awaiting,
        live: LegacyLiveOutput::default(),
        control: LegacyControlTransaction {
            waits: vec![LegacyWaitRequest::ProviderCompletion {
                token_id,
                request_id,
                provider_id: MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
                operation: "read".into(),
                key: MINORI_GLOBAL_PROGRESS_SLOT.into(),
                payload: Vec::new(),
            }],
            ..LegacyControlTransaction::default()
        },
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta::default(),
        state_revision: session.vm.state().fixed_tick,
    };
    output.validate(&input.budget)?;
    Ok(output)
}

fn consume_global_progress_result(
    session: &mut MinoriSession,
    input: &LegacyStepInput,
) -> Result<(), LegacyProviderError> {
    let pending = session.global_progress.pending.clone().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_RESULT_UNEXPECTED",
            "platform storage result has no matching global progress request",
        )
    })?;
    if input.provider_results.len() != 1 || !input.await_results.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_RESULT_COUNT",
            "global progress requires exactly one ordered provider result",
        ));
    }
    let result = &input.provider_results[0];
    let (request_id, unlocks) = match pending {
        MinoriGlobalProgressRequest::Load { request_id, .. } => {
            let unlocks = match result.status.as_str() {
                "missing" if result.payload.is_empty() => Vec::new(),
                "completed" => decode_global_progress(&result.payload)?,
                _ => {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_LOAD_RESULT",
                        "platform storage returned an invalid global progress load result",
                    ));
                }
            };
            (request_id, unlocks)
        }
        MinoriGlobalProgressRequest::Store {
            request_id,
            unlocks,
            ..
        } => {
            if result.status != "completed" || !result.payload.is_empty() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_STORE_RESULT",
                    "platform storage returned an invalid global progress store result",
                ));
            }
            (request_id, unlocks)
        }
    };
    if result.request_id != request_id || result.provider_id != MINORI_PLATFORM_STORAGE_PROVIDER_ID
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_RESULT_IDENTITY",
            "platform storage result does not match the pending global progress request",
        ));
    }
    session
        .vm
        .merge_verified_gallery_unlocks(&unlocks)
        .map_err(runtime_error)?;
    session.global_progress.loaded = true;
    session.global_progress.persisted_unlocks = unlocks;
    session.global_progress.pending = None;
    Ok(())
}

fn append_global_progress_store(
    session: &mut MinoriSession,
    output: &mut LegacyStepOutput,
) -> Result<(), LegacyProviderError> {
    if !session.global_progress.enabled
        || !session.global_progress.loaded
        || session.global_progress.pending.is_some()
        || session.vm.state().gallery_unlocks == session.global_progress.persisted_unlocks
    {
        return Ok(());
    }
    let unlocks = session.vm.state().gallery_unlocks.clone();
    let payload = encode_global_progress(&unlocks)?;
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let token_id = format!("minori.global.progress.store.token.{sequence}");
    let request_id = format!("minori.global.progress.store.request.{sequence}");
    session.global_progress.pending = Some(MinoriGlobalProgressRequest::Store {
        request_id: request_id.clone(),
        unlocks,
    });
    output
        .control
        .waits
        .push(LegacyWaitRequest::ProviderCompletion {
            token_id,
            request_id,
            provider_id: MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
            operation: "write".into(),
            key: MINORI_GLOBAL_PROGRESS_SLOT.into(),
            payload,
        });
    output.status = LegacyRuntimeStatus::Awaiting;
    output.state_revision = session.vm.state().fixed_tick;
    Ok(())
}

fn apply_system_ui_input(
    vm: &mut MinoriVm,
    input: &LegacyStepInput,
) -> Result<MinoriSystemUiAction, LegacyProviderError> {
    if vm.state().system_ui.page == MinoriSystemPage::Config {
        return apply_config_input(vm, input);
    }
    if vm.state().system_ui.page == MinoriSystemPage::Backlog {
        let wheel = backlog_wheel_direction(input)?;
        let replay = input
            .input_edges
            .iter()
            .any(|edge| edge.pressed && edge.control == "enter");
        if replay && wheel.is_some() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_BACKLOG_INPUT_AMBIGUOUS",
                "backlog voice replay cannot share a tick with wheel navigation",
            ));
        }
        return match (wheel, replay) {
            (Some(-1), false) => {
                vm.move_backlog(-1).map_err(runtime_error)?;
                Ok(MinoriSystemUiAction::Present)
            }
            (Some(1), false) => Ok(MinoriSystemUiAction::CloseBacklog),
            (None, true) => Ok(MinoriSystemUiAction::ReplayBacklogVoice),
            (None, false) => Ok(MinoriSystemUiAction::Present),
            (Some(_), _) => unreachable!("backlog wheel direction is normalized"),
        };
    }
    let mut action = MinoriSystemUiAction::Present;
    for edge in input.input_edges.iter().filter(|edge| edge.pressed) {
        let page = vm.state().system_ui.page;
        let title_item_count = if vm.title_variant() == 2 {
            MINORI_TITLE_MEMORIES_ITEM_COUNT
        } else {
            MINORI_TITLE_BASE_ITEM_COUNT
        };
        match (page, edge.control.as_str()) {
            (MinoriSystemPage::Title, "arrow_up") => vm
                .move_system_focus(-1, title_item_count)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Title, "arrow_down") => vm
                .move_system_focus(1, title_item_count)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Title, "enter" | "space") => {
                action = match (vm.title_variant(), vm.state().system_ui.focus_index) {
                    (_, 0) => {
                        vm.set_system_page(MinoriSystemPage::None, 0)
                            .map_err(runtime_error)?;
                        MinoriSystemUiAction::StartGame
                    }
                    (_, 1) => {
                        vm.set_system_page(MinoriSystemPage::Load, 0)
                            .map_err(runtime_error)?;
                        MinoriSystemUiAction::Present
                    }
                    (_, 2) => {
                        vm.open_config().map_err(runtime_error)?;
                        MinoriSystemUiAction::Present
                    }
                    (2, 3) => {
                        vm.set_system_page(MinoriSystemPage::Memories, 0)
                            .map_err(runtime_error)?;
                        MinoriSystemUiAction::Present
                    }
                    (_, 3) | (2, 4) => MinoriSystemUiAction::Exit,
                    _ => {
                        return Err(invalid(
                            "ASTRA_EMU_MINORI_TITLE_FOCUS",
                            "title focus is outside the verified menu bounds",
                        ));
                    }
                };
            }
            (MinoriSystemPage::Memories, "arrow_up") => vm
                .move_system_focus(-1, MINORI_MEMORIES_ITEM_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Memories, "arrow_down") => vm
                .move_system_focus(1, MINORI_MEMORIES_ITEM_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Memories, "enter" | "space") => {
                let target = match vm.state().system_ui.focus_index {
                    0 => MinoriSystemPage::GalleryCg,
                    1 => MinoriSystemPage::GalleryReplay,
                    2 => MinoriSystemPage::GalleryBgm,
                    3 => MinoriSystemPage::GalleryMovie,
                    4 => MinoriSystemPage::Title,
                    _ => {
                        return Err(invalid(
                            "ASTRA_EMU_MINORI_MEMORIES_FOCUS",
                            "Memories focus is outside the verified menu bounds",
                        ));
                    }
                };
                vm.set_system_page(target, 0).map_err(runtime_error)?;
            }
            (MinoriSystemPage::Memories, "escape") => {
                vm.set_system_page(MinoriSystemPage::Title, 0)
                    .map_err(runtime_error)?;
            }
            (
                MinoriSystemPage::GalleryCg
                | MinoriSystemPage::GalleryBgm
                | MinoriSystemPage::GalleryReplay
                | MinoriSystemPage::GalleryMovie,
                "escape",
            ) => {
                vm.set_system_page(MinoriSystemPage::Memories, 0)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::Load, "escape") => {
                vm.set_system_page(MinoriSystemPage::Title, 0)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::Load, "enter" | "space") => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_LOAD_SLOT_REQUIRED",
                    "load confirmation requires a verified populated slot",
                ));
            }
            _ => {}
        }
        if action != MinoriSystemUiAction::Present {
            break;
        }
    }
    Ok(action)
}

fn apply_config_input(
    vm: &mut MinoriVm,
    input: &LegacyStepInput,
) -> Result<MinoriSystemUiAction, LegacyProviderError> {
    let apply = input
        .input_edges
        .iter()
        .any(|edge| edge.pressed && matches!(edge.control.as_str(), "enter" | "space"));
    let cancel = input
        .input_edges
        .iter()
        .any(|edge| edge.pressed && edge.control == "escape");
    let pointer_pressed = input
        .input_edges
        .iter()
        .any(|edge| edge.pressed && edge.control == MINORI_POINTER_PRIMARY);
    let pointer_moved = input
        .input_edges
        .iter()
        .any(|edge| matches!(edge.control.as_str(), MINORI_POINTER_X | MINORI_POINTER_Y));
    let pointer_action =
        pointer_pressed || (pointer_moved && vm.state().system_ui.pointer_primary_pressed);
    let action_count = usize::from(apply) + usize::from(cancel) + usize::from(pointer_action);
    if action_count > 1 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_INPUT_AMBIGUOUS",
            "config apply, cancel, and pointer controls cannot share one fixed tick",
        ));
    }
    let control = if apply {
        Some(MinoriConfigControl::Apply)
    } else if cancel {
        Some(MinoriConfigControl::Cancel)
    } else if pointer_action {
        config_control_at(
            vm.state().system_ui.pointer_x,
            vm.state().system_ui.pointer_y,
        )
    } else {
        None
    };
    let Some(control) = control else {
        return Ok(MinoriSystemUiAction::Present);
    };
    match vm.apply_config_control(control).map_err(runtime_error)? {
        MinoriConfigChange::Present => Ok(MinoriSystemUiAction::Present),
        MinoriConfigChange::AudioParamsChanged => Ok(MinoriSystemUiAction::PresentWithAudioRefresh),
        MinoriConfigChange::Applied | MinoriConfigChange::Cancelled => {
            Ok(MinoriSystemUiAction::PresentAfterConfigClose)
        }
        MinoriConfigChange::TestAudio(bus) => Ok(MinoriSystemUiAction::PresentWithAudioTest(bus)),
    }
}

fn config_control_at(x: i32, y: i32) -> Option<MinoriConfigControl> {
    let slider_value = |track_left: i32| {
        let position = (x - track_left - 11).clamp(0, 200);
        u8::try_from(position * 100 / 200).expect("clamped config slider fits u8")
    };
    if (40..260).contains(&x) {
        return match y {
            152..192 => Some(MinoriConfigControl::MessageSpeedUnread(slider_value(40))),
            228..268 => Some(MinoriConfigControl::MessageSpeedRead(slider_value(40))),
            304..340 => Some(MinoriConfigControl::MessageSpeedAutoPlay(slider_value(40))),
            _ => config_non_slider_control_at(x, y),
        };
    }
    if (576..796).contains(&x) {
        return match y {
            148..188 => Some(MinoriConfigControl::BgmVolume(slider_value(576))),
            224..264 => Some(MinoriConfigControl::VoiceVolume(slider_value(576))),
            300..336 => Some(MinoriConfigControl::SeVolume(slider_value(576))),
            _ => config_non_slider_control_at(x, y),
        };
    }
    config_non_slider_control_at(x, y)
}

fn config_non_slider_control_at(x: i32, y: i32) -> Option<MinoriConfigControl> {
    let hit = |left, top, right, bottom| (left..right).contains(&x) && (top..bottom).contains(&y);
    let control = if hit(248, 492, 276, 512) {
        MinoriConfigControl::FontPrevious
    } else if hit(248, 524, 276, 548) {
        MinoriConfigControl::FontNext
    } else if hit(36, 592, 132, 616) {
        MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Auto)
    } else if hit(148, 592, 268, 616) {
        MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Skip)
    } else if hit(312, 120, 456, 152) {
        MinoriConfigControl::Fullscreen(true)
    } else if hit(312, 164, 456, 196) {
        MinoriConfigControl::Fullscreen(false)
    } else if hit(312, 248, 544, 280) {
        MinoriConfigControl::ToggleScreenEffect
    } else if hit(312, 292, 544, 320) {
        MinoriConfigControl::ToggleTextShadow
    } else if hit(312, 336, 544, 364) {
        MinoriConfigControl::ToggleAnimation
    } else if hit(312, 424, 512, 472) {
        MinoriConfigControl::ToggleBacklogVoicePlayback
    } else if hit(312, 476, 512, 524) {
        MinoriConfigControl::ToggleStopVoiceAtNextMessage
    } else if hit(312, 572, 512, 620) {
        MinoriConfigControl::ToggleProgressInBackground
    } else if hit(680, 120, 740, 144) {
        MinoriConfigControl::ToggleBgmMute
    } else if hit(680, 196, 740, 220) {
        MinoriConfigControl::ToggleVoiceMute
    } else if hit(680, 268, 740, 292) {
        MinoriConfigControl::ToggleSeMute
    } else if hit(744, 120, 780, 144) {
        MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Bgm)
    } else if hit(744, 196, 780, 220) {
        MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Voice)
    } else if hit(744, 268, 780, 292) {
        MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Se)
    } else if hit(578, 433, 688, 465) {
        MinoriConfigControl::ToggleCharacterVoice(0)
    } else if hit(578, 470, 688, 502) {
        MinoriConfigControl::ToggleCharacterVoice(1)
    } else if hit(578, 508, 688, 540) {
        MinoriConfigControl::ToggleCharacterVoice(2)
    } else if hit(578, 545, 688, 577) {
        MinoriConfigControl::ToggleCharacterVoice(3)
    } else if hit(699, 433, 776, 465) {
        MinoriConfigControl::ToggleCharacterVoice(4)
    } else if hit(592, 600, 648, 640) {
        MinoriConfigControl::Apply
    } else if hit(701, 600, 775, 640) {
        MinoriConfigControl::Cancel
    } else {
        return None;
    };
    Some(control)
}

fn backlog_wheel_direction(input: &LegacyStepInput) -> Result<Option<i32>, LegacyProviderError> {
    let mut direction = None;
    for edge in input
        .input_edges
        .iter()
        .filter(|edge| edge.control == "wheel")
    {
        let current = if edge.value < 0.0 {
            Some(-1)
        } else if edge.value > 0.0 {
            Some(1)
        } else {
            None
        };
        if let Some(current) = current {
            if direction.is_some_and(|existing| existing != current) {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_BACKLOG_WHEEL_AMBIGUOUS",
                    "one fixed tick contains conflicting backlog wheel directions",
                ));
            }
            direction = Some(current);
        }
    }
    Ok(direction)
}

fn system_ui_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let status = if session.vm.state().terminal {
        LegacyRuntimeStatus::Terminal
    } else {
        LegacyRuntimeStatus::Active
    };
    let audio_command_count = u64::try_from(audio_commands.len()).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_AUDIO_COMMAND_BOUNDS",
            "system UI audio command count cannot be represented",
        )
    })?;
    let mut live = LegacyLiveOutput {
        clear_text: status == LegacyRuntimeStatus::Terminal,
        audio_commands,
        ..LegacyLiveOutput::default()
    };
    // Scene2D is retained by the host. A system page that did not change in
    // this fixed tick must not rebuild its resource frame: doing so would
    // reopen and parse the same PAZ image on every tick and would also submit
    // a semantically redundant scene transaction. Input is the only way a
    // system page can change its focus/variant here; restore explicitly asks
    // for a fresh presentation.
    let system_page_changed = session.reported_system_page
        != Some(session.vm.state().system_ui.page)
        || !input.input_edges.is_empty()
        || session.restore_presentation_pending;
    if status != LegacyRuntimeStatus::Terminal && system_page_changed {
        live.clear_text = true;
        let is_backlog = session.vm.state().system_ui.page == MinoriSystemPage::Backlog;
        let sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        live.resource_scenes.push(LegacySequenced {
            sequence,
            value: if is_backlog {
                describe_backlog_frame(
                    vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    sequence,
                )?
            } else {
                describe_system_page(vfs, &session.mount_set_id, session.stage_size, &session.vm)?
            },
        });
        if is_backlog {
            append_backlog_text(session, input.tick_index, &mut live)?;
        }
    }
    let mut output = LegacyStepOutput {
        status,
        live,
        control: LegacyControlTransaction::default(),
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta {
            audio_commands: audio_command_count,
            ..LegacyCoverageDelta::default()
        },
        state_revision: session.vm.state().fixed_tick,
    };
    if session.vm.state().system_ui.page == MinoriSystemPage::Backlog
        && input.await_results.is_empty()
        && input.input_edges.is_empty()
        && audio_command_count > 0
    {
        let wait = session
            .vm
            .state()
            .wait
            .as_ref()
            .ok_or_else(|| runtime_error(MinoriRuntimeError::Backlog))?;
        output.control.waits.push(legacy_wait(wait));
    }
    let reported_system_page = append_system_page_observation(session, &mut output.control)?;
    let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
    let reported_gallery_unlock_count =
        append_gallery_unlock_observation(session, &mut output.control)?;
    let reported_choice_active = append_choice_active_observation(session, &mut output.control)?;
    output.validate(&input.budget)?;
    if let Some(page) = reported_system_page {
        session.reported_system_page = Some(page);
    }
    if let Some(mode) = reported_play_mode {
        session.reported_play_mode = Some(mode);
    }
    if let Some(count) = reported_gallery_unlock_count {
        session.reported_gallery_unlock_count = Some(count);
    }
    if let Some(active) = reported_choice_active {
        session.reported_choice_active = Some(active);
    }
    session.restore_presentation_pending = false;
    Ok(output)
}

fn append_validated_audio_commands<'a>(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    commands: impl IntoIterator<Item = &'a MinoriAudioCommand>,
    state: &MinoriRuntimeState,
    output: &mut Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<(), LegacyProviderError> {
    for command in commands {
        let (sequence, command) = map_audio_command(command, state)?;
        if let LegacyAudioCommandV1::LoadResource { resource_uri, .. } = &command {
            let stat = vfs.stat_file(mount_set_id, resource_uri)?;
            if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                    "audio resource is empty or exceeds the session bound",
                ));
            }
        }
        command.validate()?;
        output.push(LegacySequenced {
            sequence,
            value: command,
        });
    }
    Ok(())
}

fn gameplay_resume_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let stage_size = session.stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_STAGE_SIZE",
            "closing backlog requires explicit host dimensions",
        )
    })?;
    let scene_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let frame = if session.vm.state().firefly.is_some() {
        describe_firefly_frame(vfs, &session.mount_set_id, session.vm.state(), stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            &session.mount_set_id,
            session.vm.state(),
            &visible_effect_frame(session.vm.state(), scene_sequence)?,
            stage_size,
        )?
    };
    let current_message = session.vm.state().message.as_ref().map(|message| {
        session
            .vm
            .state()
            .backlog
            .iter()
            .rev()
            .find(|entry| {
                entry.source == message.source
                    && entry.message_id == message.message_id
                    && entry.text_hash == message.text_hash
                    && entry.speaker_hash == message.speaker_hash
                    && entry.voice_hash == message.voice_hash
            })
            .map(|entry| (entry.text.clone(), entry.speaker.clone()))
    });
    let current_message = match current_message {
        Some(Some(message)) => Some(message),
        Some(None) => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_BACKLOG_MESSAGE_IDENTITY",
                "current message is missing from the retained backlog",
            ));
        }
        None => None,
    };
    let audio_command_count = u64::try_from(audio_commands.len()).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_AUDIO_COMMAND_BOUNDS",
            "backlog audio command count cannot be represented",
        )
    })?;
    let mut live = LegacyLiveOutput {
        clear_text: true,
        resource_scenes: vec![LegacySequenced {
            sequence: scene_sequence,
            value: frame,
        }],
        audio_commands,
        ..LegacyLiveOutput::default()
    };
    if let Some((text, speaker)) = current_message {
        append_resumed_message_text(session, input.tick_index, text, speaker, &mut live)?;
    }
    if matches!(
        session.vm.state().wait.as_ref(),
        Some(MinoriWaitState::Choice { .. })
    ) {
        let sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let _ = append_choice_live_output(session, vfs, input.tick_index, sequence, &mut live)?;
    }
    let mut output = LegacyStepOutput {
        status: if session.vm.state().wait.is_some() {
            LegacyRuntimeStatus::Awaiting
        } else {
            LegacyRuntimeStatus::Active
        },
        live,
        control: LegacyControlTransaction::default(),
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta {
            audio_commands: audio_command_count,
            ..LegacyCoverageDelta::default()
        },
        state_revision: session.vm.state().fixed_tick,
    };
    let reported_system_page = append_system_page_observation(session, &mut output.control)?;
    let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
    let reported_gallery_unlock_count =
        append_gallery_unlock_observation(session, &mut output.control)?;
    let reported_choice_active = append_choice_active_observation(session, &mut output.control)?;
    output.validate(&input.budget)?;
    if let Some(page) = reported_system_page {
        session.reported_system_page = Some(page);
    }
    if let Some(mode) = reported_play_mode {
        session.reported_play_mode = Some(mode);
    }
    if let Some(count) = reported_gallery_unlock_count {
        session.reported_gallery_unlock_count = Some(count);
    }
    if let Some(active) = reported_choice_active {
        session.reported_choice_active = Some(active);
    }
    session.restore_presentation_pending = false;
    Ok(output)
}

fn append_resumed_message_text(
    session: &mut MinoriSession,
    tick_index: u64,
    text: String,
    speaker: Option<String>,
    live: &mut LegacyLiveOutput,
) -> Result<(), LegacyProviderError> {
    if text.len() > MAX_EPHEMERAL_TEXT_BYTES
        || speaker
            .as_ref()
            .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
            "restored message exceeds the ephemeral text channel bound",
        ));
    }
    let presentation_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let capture_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let lease_id = format!("minori.text.resume.{tick_index}.{capture_sequence}");
    let presentation = LegacyTextPresentationLeaseV1 {
        lease_id: lease_id.clone(),
        presentation: minori_message_presentation(session.stage_size)?,
    };
    presentation.validate()?;
    if session
        .ephemeral_text
        .insert(
            lease_id.clone(),
            LegacyEphemeralText {
                lease_id: lease_id.clone(),
                text: text.clone(),
                speaker,
            },
        )
        .is_some()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
            "resumed message lease id is duplicated",
        ));
    }
    live.text_presentations.push(LegacySequenced {
        sequence: presentation_sequence,
        value: presentation,
    });
    live.text.push(LegacyTextLease {
        sequence: capture_sequence,
        lease_id,
        byte_len: text.len().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                "resumed message length cannot be represented",
            )
        })?,
        source_ref: "minori.sc.message.resume".into(),
    });
    Ok(())
}

fn append_backlog_text(
    session: &mut MinoriSession,
    tick_index: u64,
    live: &mut LegacyLiveOutput,
) -> Result<(), LegacyProviderError> {
    let cursor = usize::try_from(session.vm.state().system_ui.backlog_cursor.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog text presentation has no active cursor",
        )
    })?)
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog text cursor cannot be represented",
        )
    })?;
    let entry = session
        .vm
        .state()
        .backlog
        .get(cursor)
        .cloned()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
                "backlog text cursor is outside the retained history",
            )
        })?;
    if entry.text.len() > MAX_EPHEMERAL_TEXT_BYTES
        || entry
            .speaker
            .as_ref()
            .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
            "backlog record exceeds the ephemeral text channel bound",
        ));
    }
    let presentation_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let capture_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let lease_id = format!("minori.text.backlog.{tick_index}.{capture_sequence}");
    let presentation = LegacyTextPresentationLeaseV1 {
        lease_id: lease_id.clone(),
        // IDA confirms that backlog state 11 keeps CMessagePanel mode 1 and
        // state 12 submits the selected CLog record through the same layout.
        presentation: minori_message_presentation(session.stage_size)?,
    };
    presentation.validate()?;
    if session
        .ephemeral_text
        .insert(
            lease_id.clone(),
            LegacyEphemeralText {
                lease_id: lease_id.clone(),
                text: entry.text.clone(),
                speaker: entry.speaker,
            },
        )
        .is_some()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
            "backlog text lease id is duplicated",
        ));
    }
    live.text_presentations.push(LegacySequenced {
        sequence: presentation_sequence,
        value: presentation,
    });
    live.text.push(LegacyTextLease {
        sequence: capture_sequence,
        lease_id,
        byte_len: entry.text.len().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                "backlog message length cannot be represented",
            )
        })?,
        source_ref: "minori.sc.backlog".into(),
    });
    Ok(())
}

fn describe_backlog_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_STAGE_SIZE",
            "backlog presentation requires explicit host dimensions",
        )
    })?;
    if stage_size != (1280, 720) || state.system_ui.page != MinoriSystemPage::Backlog {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_STAGE_IDENTITY",
            "verified backlog presentation requires the 1280x720 reference stage",
        ));
    }
    let cursor = usize::try_from(state.system_ui.backlog_cursor.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog presentation has no active cursor",
        )
    })?)
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog cursor cannot be represented",
        )
    })?;
    if cursor >= state.backlog.len() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog cursor is outside the retained history",
        ));
    }
    let effect = visible_effect_frame(state, sequence)?;
    let mut frame = describe_effect_frame_without_secondary(
        vfs,
        mount_set_id,
        state,
        &effect,
        stage_size,
        true,
    )?;
    append_secondary_effect_to_frame(vfs, mount_set_id, state, &mut frame)?;
    apply_screen_shake_to_frame(state, &mut frame)?;

    let gauge_index = frame.texture_resources.len();
    append_resource_layer(
        vfs,
        mount_set_id,
        "minori:/sys/backlogGauge.png",
        0,
        0,
        1.0,
        MINORI_BACKLOG_GAUGE_TEXTURE_ID,
        &mut frame.texture_resources,
        &mut frame.draws,
    )?;
    let gauge = frame.texture_resources.get(gauge_index).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE",
            "backlog gauge resource was not appended",
        )
    })?;
    if (gauge.decoded_width, gauge.decoded_height) != (18, 144) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE_DIMENSIONS",
            "backlog gauge does not match the verified dimensions",
        ));
    }
    let count = state.backlog.len();
    let ball_y = 138usize
        .checked_mul(cursor)
        .and_then(|value| value.checked_div(count))
        .map(|value| value.saturating_sub(7).min(124) + 3)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
                "backlog gauge position overflowed",
            )
        })?;
    let ball_index = frame.texture_resources.len();
    append_resource_layer(
        vfs,
        mount_set_id,
        "minori:/sys/ball.png",
        2,
        i32::try_from(ball_y).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
                "backlog gauge position cannot be represented",
            )
        })?,
        1.0,
        MINORI_BACKLOG_BALL_TEXTURE_ID,
        &mut frame.texture_resources,
        &mut frame.draws,
    )?;
    let ball = frame.texture_resources.get(ball_index).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE",
            "backlog ball resource was not appended",
        )
    })?;
    if (ball.decoded_width, ball.decoded_height) != (14, 14) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE_DIMENSIONS",
            "backlog ball does not match the verified dimensions",
        ));
    }
    frame.validate()?;
    Ok(frame)
}

fn describe_system_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let (width, height) = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SYSTEM_STAGE_SIZE",
            "system UI requires explicit host dimensions",
        )
    })?;
    if (width, height) != (1280, 720) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_STAGE_IDENTITY",
            "verified Minori system pages require the 1280x720 reference stage",
        ));
    }
    if vm.state().system_ui.page == MinoriSystemPage::Config {
        return describe_config_page(vfs, mount_set_id, width, height, vm);
    }
    let resource_uri = match vm.state().system_ui.page {
        MinoriSystemPage::Title => match vm.title_variant() {
            0 => "minori:/sys/topMenu0.png",
            1 => "minori:/sys/topMenu1.png",
            2 => "minori:/sys/topMenu2.png",
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TITLE_VARIANT",
                    "verified title variant is outside the supported range",
                ));
            }
        },
        MinoriSystemPage::Load => "minori:/sys/saveloadBase.png",
        MinoriSystemPage::Config => unreachable!("config uses its stateful presentation path"),
        MinoriSystemPage::Memories => "minori:/sys/memories.png",
        MinoriSystemPage::GalleryCg => "minori:/sys/cgmode0.png",
        MinoriSystemPage::GalleryBgm => "minori:/sys/musicPage1.png",
        MinoriSystemPage::GalleryReplay => "minori:/sys/flash0.png",
        MinoriSystemPage::GalleryMovie => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GALLERY_MOVIE_SCRIPT_REQUIRED",
                "the original movie gallery is script-driven and is not a static system page",
            ));
        }
        MinoriSystemPage::None => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_PAGE",
                "gameplay does not have a system-page presentation",
            ));
        }
        MinoriSystemPage::Save | MinoriSystemPage::Backlog => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_PAGE_PRESENTATION_ROUTE",
                "system page must use its verified dedicated presentation path",
            ));
        }
    };
    let resource =
        read_texture_resource(vfs, mount_set_id, resource_uri, MINORI_SYSTEM_TEXTURE_ID)?;
    if (resource.decoded_width, resource.decoded_height) != (width, height) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_RESOURCE_DIMENSIONS",
            "system page resource dimensions do not match the reference stage",
        ));
    }
    let mut draws = Vec::with_capacity(1);
    append_texture_draw(&resource, 0, 0, 1.0, &mut draws)?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![resource],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_config_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let base = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/configBase.png",
        MINORI_SYSTEM_TEXTURE_ID,
    )?;
    let knob = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/knob.png",
        MINORI_CONFIG_KNOB_TEXTURE_ID,
    )?;
    let checkmark = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/checkmark.png",
        MINORI_CONFIG_CHECKMARK_TEXTURE_ID,
    )?;
    let circle = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/circle.png",
        MINORI_CONFIG_CIRCLE_TEXTURE_ID,
    )?;
    if (base.decoded_width, base.decoded_height) != (width, height)
        || (knob.decoded_width, knob.decoded_height) != (15, 25)
        || (checkmark.decoded_width, checkmark.decoded_height) != (21, 32)
        || (circle.decoded_width, circle.decoded_height) != (74, 74)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_RESOURCE_DIMENSIONS",
            "config resources do not match the verified dimensions",
        ));
    }
    let config = vm.config_for_presentation().map_err(runtime_error)?;
    let mut draws = Vec::with_capacity(24);
    append_texture_draw(&base, 0, 0, 1.0, &mut draws)?;
    for (value, left, top) in [
        (config.message_speed_unread, 42, 159),
        (config.message_speed_read, 42, 235),
        (config.message_speed_auto_play, 42, 310),
        (config.bgm_volume, 578, 156),
        (config.voice_volume, 578, 231),
        (config.se_volume, 578, 305),
    ] {
        append_texture_draw(&knob, left + i32::from(value) * 2, top, 1.0, &mut draws)?;
    }
    let mut checks = Vec::with_capacity(15);
    checks.push(match config.preferred_play_mode {
        MinoriPlayMode::Auto => (40, 588),
        MinoriPlayMode::Skip => (153, 588),
        MinoriPlayMode::Normal => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CONFIG_PLAY_MODE",
                "config preferred play mode is not verified",
            ));
        }
    });
    checks.push(if config.fullscreen {
        (319, 120)
    } else {
        (319, 164)
    });
    for (enabled, position) in [
        (config.screen_effect, (319, 248)),
        (config.text_shadow, (319, 292)),
        (config.animation, (319, 336)),
        (config.backlog_voice_playback, (319, 424)),
        (config.stop_voice_at_next_message, (319, 476)),
        (config.progress_in_background, (319, 572)),
        (config.bgm_muted, (684, 117)),
        (config.voice_muted, (684, 193)),
        (config.se_muted, (684, 265)),
    ] {
        if enabled {
            checks.push(position);
        }
    }
    for (enabled, position) in config.character_voice_enabled.iter().copied().zip([
        (575, 424),
        (575, 461),
        (575, 499),
        (575, 536),
        (696, 424),
    ]) {
        if enabled {
            checks.push(position);
        }
    }
    for (left, top) in checks {
        append_texture_draw(&checkmark, left, top, 1.0, &mut draws)?;
    }
    let pointer = (
        vm.state().system_ui.pointer_x,
        vm.state().system_ui.pointer_y,
    );
    let hover_circle = if (592..648).contains(&pointer.0) && (600..640).contains(&pointer.1) {
        Some((584, 584))
    } else if (701..775).contains(&pointer.0) && (600..640).contains(&pointer.1) {
        Some((701, 584))
    } else {
        None
    };
    if let Some((left, top)) = hover_circle {
        append_texture_draw(&circle, left, top, 1.0, &mut draws)?;
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![base, knob, checkmark, circle],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn waiting_output(
    session: &mut MinoriSession,
    wait: MinoriWaitState,
    live: LegacyLiveOutput,
    event: Option<LegacyEvent>,
    publish_rebound_wait: bool,
    input: &LegacyStepInput,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let mut output = LegacyStepOutput {
        status: LegacyRuntimeStatus::Awaiting,
        live,
        // A wait request is edge-triggered: it is published only by the
        // command that creates the token. Re-emitting the same pending token
        // on later ticks would violate RuntimeWorld AwaitQueue uniqueness.
        control: LegacyControlTransaction {
            events: event.into_iter().collect(),
            waits: publish_rebound_wait
                .then(|| legacy_wait(&wait))
                .into_iter()
                .collect(),
            ..LegacyControlTransaction::default()
        },
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta::default(),
        state_revision: session.vm.state().fixed_tick,
    };
    let reported_system_page = append_system_page_observation(session, &mut output.control)?;
    let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
    let reported_gallery_unlock_count =
        append_gallery_unlock_observation(session, &mut output.control)?;
    let reported_choice_active = append_choice_active_observation(session, &mut output.control)?;
    output.validate(&input.budget)?;
    if let Some(page) = reported_system_page {
        session.reported_system_page = Some(page);
    }
    if let Some(mode) = reported_play_mode {
        session.reported_play_mode = Some(mode);
    }
    if let Some(count) = reported_gallery_unlock_count {
        session.reported_gallery_unlock_count = Some(count);
    }
    if let Some(active) = reported_choice_active {
        session.reported_choice_active = Some(active);
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
struct ChoiceVisualLayout {
    left: i32,
    top: i32,
    width: u32,
    row_height: u32,
}

fn append_choice_live_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    tick_index: u64,
    sequence: u64,
    live: &mut LegacyLiveOutput,
) -> Result<LegacyEvent, LegacyProviderError> {
    let (option_hashes, selected_index) = session
        .vm
        .state()
        .choice
        .as_ref()
        .map(|choice| {
            (
                choice.option_hashes.clone(),
                choice.selected_index.unwrap_or_default(),
            )
        })
        .ok_or_else(|| runtime_error(MinoriRuntimeError::Choice))?;
    let options = session.vm.choice_display_texts().map_err(runtime_error)?;
    if options.len() != option_hashes.len()
        || options
            .iter()
            .any(|option| option.is_empty() || option.len() > MAX_EPHEMERAL_TEXT_BYTES)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_TEXT_BOUNDS",
            "choice text violates the ephemeral text bounds",
        ));
    }
    let scene_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let (scene, layout) = choice_resource_presentation(
        vfs,
        &session.mount_set_id,
        session.stage_size,
        session.vm.state(),
        option_hashes.len(),
        selected_index,
        scene_sequence,
    )?;
    live.resource_scenes.push(scene);
    for (index, option) in options.into_iter().enumerate() {
        let index_u32 = u32::try_from(index).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_STATE",
                "choice option index cannot be represented",
            )
        })?;
        let row_offset = layout.row_height.checked_mul(index_u32).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice row offset overflowed",
            )
        })?;
        let vertical_padding = layout.row_height.saturating_sub(30) / 2;
        let y = i64::from(layout.top)
            .checked_add(i64::from(row_offset))
            .and_then(|value| value.checked_add(i64::from(vertical_padding)))
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                    "choice text position overflowed",
                )
            })?;
        let presentation_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let capture_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let lease_id = format!("minori.choice.{tick_index}.{capture_sequence}.{index}");
        let presentation = LegacyTextPresentationLeaseV1 {
            lease_id: lease_id.clone(),
            presentation: LegacyTextPresentationV1 {
                layout_id: format!("minori.choice.option.{index}"),
                language: "ja-JP".into(),
                font_families: vec!["Noto Sans JP".into()],
                body: LegacyTextRegionV1 {
                    x: layout.left,
                    y,
                    width: layout.width,
                    height: 30,
                    font_size: 26.0,
                    line_height: 30.0,
                    max_lines: 1,
                    horizontal_alignment: LegacyTextHorizontalAlignmentV1::Center,
                },
                speaker: None,
                rgba: [255, 255, 255, 255],
                outline: Some(LegacyTextOutlineV1 {
                    radius: 2,
                    rgba: [0, 0, 0, 192],
                }),
            },
        };
        presentation.validate()?;
        if session
            .ephemeral_text
            .insert(
                lease_id.clone(),
                LegacyEphemeralText {
                    lease_id: lease_id.clone(),
                    text: option.clone(),
                    speaker: None,
                },
            )
            .is_some()
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
                "choice text lease id is duplicated",
            ));
        }
        live.text_presentations.push(LegacySequenced {
            sequence: presentation_sequence,
            value: presentation,
        });
        live.text.push(LegacyTextLease {
            sequence: capture_sequence,
            lease_id,
            byte_len: option.len().try_into().map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_TEXT_BOUNDS",
                    "choice text length cannot be represented",
                )
            })?,
            source_ref: "minori.sc.select".into(),
        });
    }
    choice_presentation_from_parts(&option_hashes, selected_index, sequence)
}

fn choice_resource_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    option_count: usize,
    selected_index: u32,
    sequence: u64,
) -> Result<
    (
        LegacySequenced<LegacyRenderResourceFrameV1>,
        ChoiceVisualLayout,
    ),
    LegacyProviderError,
> {
    let (stage_width, stage_height) = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_STAGE_IDENTITY",
            "choice presentation requires explicit host dimensions",
        )
    })?;
    if (stage_width, stage_height) != (1280, 720)
        || !(1..=4).contains(&option_count)
        || usize::try_from(selected_index)
            .ok()
            .is_none_or(|index| index >= option_count)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_STAGE_IDENTITY",
            "choice presentation is outside the verified stage or option bounds",
        ));
    }
    let mut frame = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, (stage_width, stage_height))?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, sequence)?,
            (stage_width, stage_height),
        )?
    };
    let mut choice_resources = Vec::with_capacity(MINORI_CHOICE_RESOURCE_URIS.len());
    for (index, uri) in MINORI_CHOICE_RESOURCE_URIS.iter().enumerate() {
        let texture_id = MINORI_CHOICE_TEXTURE_BASE
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_RESOURCE",
                    "choice resource index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_RESOURCE",
                    "choice texture id overflowed",
                )
            })?;
        choice_resources.push(read_texture_resource(vfs, mount_set_id, uri, texture_id)?);
    }
    let width = choice_resources[0].decoded_width;
    let row_height = choice_resources[0].decoded_height;
    if width == 0
        || row_height < 30
        || choice_resources.iter().any(|resource| {
            resource.decoded_width != width || resource.decoded_height != row_height
        })
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_RESOURCE_IDENTITY",
            "choice state resources do not share the verified dimensions",
        ));
    }
    let total_height = row_height
        .checked_mul(u32::try_from(option_count).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice option count cannot be represented",
            )
        })?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice layout height overflowed",
            )
        })?;
    if width > stage_width || total_height > stage_height {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
            "choice resources exceed the stage bounds",
        ));
    }
    let left = i32::try_from((stage_width - width) / 2).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
            "choice horizontal position cannot be represented",
        )
    })?;
    let top = i32::try_from((stage_height - total_height) / 2).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
            "choice vertical position cannot be represented",
        )
    })?;
    for index in 0..option_count {
        let texture_index = if index == usize::try_from(selected_index).unwrap_or_default() {
            1
        } else {
            0
        };
        let resource = &choice_resources[texture_index];
        let index = i64::try_from(index).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice row index cannot be represented",
            )
        })?;
        let y = i64::from(top)
            .checked_add(i64::from(row_height) * index)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                    "choice row position overflowed",
                )
            })?;
        append_texture_draw(resource, left, y, 1.0, &mut frame.draws)?;
    }
    frame.texture_resources.extend(choice_resources);
    frame.validate()?;
    Ok((
        LegacySequenced {
            sequence,
            value: frame,
        },
        ChoiceVisualLayout {
            left,
            top,
            width,
            row_height,
        },
    ))
}

fn choice_direction(control: &str) -> Option<i32> {
    if control == MINORI_CHOICE_NAVIGATION_CONTROLS[0] {
        Some(-1)
    } else if control == MINORI_CHOICE_NAVIGATION_CONTROLS[1] {
        Some(1)
    } else {
        None
    }
}

fn choice_presentation_from_parts(
    option_hashes: &[Hash256],
    selected_index: u32,
    sequence: u64,
) -> Result<LegacyEvent, LegacyProviderError> {
    if !(1..=4).contains(&option_hashes.len())
        || usize::try_from(selected_index)
            .ok()
            .is_none_or(|index| index >= option_hashes.len())
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_STATE",
            "choice presentation has an invalid option selection",
        ));
    }
    let payload = postcard::to_allocvec(&MinoriChoicePresentation {
        schema: MINORI_CHOICE_PRESENTATION_SCHEMA.into(),
        option_hashes: option_hashes.to_vec(),
        selected_index,
    })
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_ENCODE",
            "choice presentation could not be encoded",
        )
    })?;
    Ok(LegacyEvent {
        sequence,
        event: MINORI_CHOICE_PRESENTATION_SCHEMA.into(),
        value: Hash256::from_sha256(&payload).to_string(),
    })
}

fn movie_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    movie: &MinoriMovieState,
    sequence: u64,
) -> Result<LegacySequenced<LegacyVideoCommandV1>, LegacyProviderError> {
    if stage_size != Some((movie.width, movie.height)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOVIE_STAGE_IDENTITY",
            "movie dimensions do not match the explicit runtime stage",
        ));
    }
    let stat = vfs
        .stat_file(mount_set_id, &movie.resource_uri)
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_MOVIE_RESOURCE",
                "movie resource is unavailable",
            )
        })?;
    if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOVIE_RESOURCE",
            "movie resource violates the bounded VFS contract",
        ));
    }
    let command = LegacyVideoCommandV1::Play {
        playback_id: movie.media_id.clone(),
        resource_uri: movie.resource_uri.clone(),
        mode: LegacyVideoMode::ModalWithAudio,
        stage_width: movie.width,
        stage_height: movie.height,
    };
    command.validate()?;
    Ok(LegacySequenced {
        sequence,
        value: command,
    })
}

fn legacy_wait(wait: &MinoriWaitState) -> LegacyWaitRequest {
    match wait {
        MinoriWaitState::Time {
            token_id,
            timer_ticks: _,
            milliseconds,
        } => LegacyWaitRequest::Time {
            token_id: token_id.clone(),
            milliseconds: *milliseconds,
        },
        MinoriWaitState::AxisScroll {
            token_id,
            milliseconds,
        }
        | MinoriWaitState::LinearScroll {
            token_id,
            milliseconds,
        }
        | MinoriWaitState::CharacterTransition {
            token_id,
            milliseconds,
            ..
        } => LegacyWaitRequest::Time {
            token_id: token_id.clone(),
            milliseconds: *milliseconds,
        },
        MinoriWaitState::Input { token_id } => LegacyWaitRequest::Input {
            token_id: token_id.clone(),
            keys: message_input_keys(),
        },
        MinoriWaitState::Choice { token_id } => LegacyWaitRequest::Input {
            token_id: token_id.clone(),
            keys: choice_input_keys(),
        },
        MinoriWaitState::Media { token_id, media_id } => LegacyWaitRequest::MediaFence {
            token_id: token_id.clone(),
            media_id: media_id.clone(),
        },
        MinoriWaitState::Presentation { token_id, fence_id } => {
            LegacyWaitRequest::PresentationFence {
                token_id: token_id.clone(),
                fence_id: fence_id.clone(),
            }
        }
        MinoriWaitState::Provider {
            token_id,
            request_id,
        } => LegacyWaitRequest::ProviderCompletion {
            token_id: token_id.clone(),
            request_id: request_id.clone(),
            provider_id: "astra.family.provider".into(),
            operation: "complete".into(),
            key: request_id.clone(),
            payload: Vec::new(),
        },
    }
}

fn wait_token(wait: &MinoriWaitState) -> &str {
    match wait {
        MinoriWaitState::Time { token_id, .. }
        | MinoriWaitState::AxisScroll { token_id, .. }
        | MinoriWaitState::LinearScroll { token_id, .. }
        | MinoriWaitState::CharacterTransition { token_id, .. }
        | MinoriWaitState::Input { token_id }
        | MinoriWaitState::Choice { token_id }
        | MinoriWaitState::Media { token_id, .. }
        | MinoriWaitState::Presentation { token_id, .. }
        | MinoriWaitState::Provider { token_id, .. } => token_id,
    }
}

fn validate_session_binding(
    ctx: &LegacyRuntimeHostCtx,
    session: &MinoriSession,
) -> Result<(), LegacyProviderError> {
    if ctx.mount_set_id != session.mount_set_id {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOUNT_BINDING",
            "host mount does not match the open session",
        ));
    }
    Ok(())
}

fn script_error(_error: crate::ScParseError) -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_SCRIPT_PARSE",
        "Minori script failed strict parsing",
    )
}

fn runtime_error(error: MinoriRuntimeError) -> LegacyProviderError {
    LegacyProviderError::invalid(runtime_error_code(&error), error.to_string())
}

fn runtime_error_code(error: &MinoriRuntimeError) -> &'static str {
    match error {
        MinoriRuntimeError::State => "ASTRA_EMU_MINORI_RUNTIME_STATE",
        MinoriRuntimeError::ProgramCounter => "ASTRA_EMU_MINORI_RUNTIME_PC",
        MinoriRuntimeError::Label => "ASTRA_EMU_MINORI_RUNTIME_LABEL",
        MinoriRuntimeError::Operand => "ASTRA_EMU_MINORI_RUNTIME_OPERAND",
        MinoriRuntimeError::UnsupportedOpcode { .. } => "ASTRA_EMU_MINORI_RUNTIME_OPCODE",
        MinoriRuntimeError::UnsupportedPragma { .. } => "ASTRA_EMU_MINORI_RUNTIME_PRAGMA",
        MinoriRuntimeError::Budget => "ASTRA_EMU_MINORI_RUNTIME_BUDGET",
        MinoriRuntimeError::Waiting => "ASTRA_EMU_MINORI_RUNTIME_WAIT",
        MinoriRuntimeError::Overflow => "ASTRA_EMU_MINORI_RUNTIME_OVERFLOW",
        MinoriRuntimeError::Snapshot => "ASTRA_EMU_MINORI_RUNTIME_SNAPSHOT",
        MinoriRuntimeError::ChainTarget => "ASTRA_EMU_MINORI_RUNTIME_CHAIN",
        MinoriRuntimeError::AudioResource => "ASTRA_EMU_MINORI_RUNTIME_AUDIO_RESOURCE",
        MinoriRuntimeError::Effect { .. } => "ASTRA_EMU_MINORI_RUNTIME_EFFECT",
        MinoriRuntimeError::UnsupportedEffectKind { .. } => "ASTRA_EMU_MINORI_RUNTIME_EFFECT_KIND",
        MinoriRuntimeError::Panel { .. } => "ASTRA_EMU_MINORI_RUNTIME_PANEL",
        MinoriRuntimeError::Choice => "ASTRA_EMU_MINORI_RUNTIME_CHOICE",
        MinoriRuntimeError::Firefly => "ASTRA_EMU_MINORI_RUNTIME_FIREFLY",
        MinoriRuntimeError::SecondaryEffect => "ASTRA_EMU_MINORI_RUNTIME_SECONDARY_EFFECT",
        MinoriRuntimeError::ScreenShake => "ASTRA_EMU_MINORI_RUNTIME_SCREEN_SHAKE",
        MinoriRuntimeError::ScrollXf => "ASTRA_EMU_MINORI_RUNTIME_SCROLL_XF",
        MinoriRuntimeError::WScroll2 => "ASTRA_EMU_MINORI_RUNTIME_WSCROLL2",
        MinoriRuntimeError::Character => "ASTRA_EMU_MINORI_RUNTIME_CHARACTER",
        MinoriRuntimeError::AxisScroll => "ASTRA_EMU_MINORI_RUNTIME_AXIS_SCROLL",
        MinoriRuntimeError::LinearScroll => "ASTRA_EMU_MINORI_RUNTIME_LINEAR_SCROLL",
        MinoriRuntimeError::Backlog => "ASTRA_EMU_MINORI_RUNTIME_BACKLOG",
    }
}

fn session_missing() -> LegacyProviderError {
    invalid("ASTRA_EMU_MINORI_SESSION_MISSING", "session is not active")
}

fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};
    use astra_emu_family_api::{
        LegacyAwaitResult, LegacyInputEdge, LegacyReplayMode, LegacyStepBudget,
    };
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

    use super::*;
    use crate::MinoriPlayMode;

    struct MemoryReader {
        scripts: BTreeMap<String, Vec<u8>>,
    }

    impl LegacyVfsReader for MemoryReader {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" {
                return Err(invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"));
            }
            let script = self
                .scripts
                .get(uri)
                .ok_or_else(|| invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"))?;
            let digest = Hash256::from_sha256(script);
            let revision = u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap());
            Ok(ByteSourceStat {
                len: script.len() as u64,
                revision: SourceRevision(revision),
            })
        }

        fn read_file_range(
            &self,
            mount_set_id: &str,
            uri: &str,
            expected_revision: SourceRevision,
            range: ByteRange,
            max_bytes: u64,
        ) -> Result<RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file(mount_set_id, uri)?;
            range
                .validate(stat.len, max_bytes)
                .map_err(|_| invalid("TEST_VFS_BOUNDS", "fixture range is invalid"))?;
            if expected_revision != stat.revision {
                return Err(invalid("TEST_VFS_REVISION", "fixture revision changed"));
            }
            let script = self
                .scripts
                .get(uri)
                .ok_or_else(|| invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"))?;
            let bytes = script[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(RangeReadResult {
                range,
                revision: stat.revision,
                bytes: bytes.into(),
            })
        }
    }

    #[test]
    fn provider_lifecycle_wait_snapshot_restore_and_shutdown() {
        let script = b".setglobal REN_CLEAR = 1\r\n.wait 20\r\n.end\r\n".to_vec();
        let case_fingerprint = Hash256::from_sha256(b"case");
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.test".into()),
                    case_fingerprint,
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([(
                        "astra.hosted_trace_profile".into(),
                        "evidence".into(),
                    )]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(first.status, LegacyRuntimeStatus::Awaiting);
        assert!(first.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "1"
        }));
        let token = match &first.control.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 200);
                token_id.clone()
            }
            _ => panic!("expected time wait"),
        };
        let snapshot = provider.save(&ctx, &session).unwrap();
        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        provider.restore(&ctx, &session, &snapshot).unwrap();
        let completed = provider
            .step(
                &ctx,
                &session,
                step_input(
                    2,
                    vec![LegacyAwaitResult {
                        token_id: token,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert_eq!(shutdown.instruction_count, 3);
        assert_eq!(shutdown.evidence_vm_trace.len(), 3);
        assert_eq!(shutdown.evidence_vm_trace[0].program_counter, 0);
        assert_eq!(shutdown.evidence_vm_trace[0].opcode, 11);
        assert_eq!(shutdown.evidence_vm_trace[1].program_counter, 1);
        assert_eq!(shutdown.evidence_vm_trace[1].opcode, 8);
        assert_eq!(shutdown.evidence_vm_trace[2].program_counter, 2);
        assert_eq!(shutdown.evidence_vm_trace[2].opcode, 28);
        assert!(!provider.has_active_sessions());
    }

    #[test]
    fn provider_round_trips_verified_global_progress_through_platform_storage() {
        let script = b".setglobal REN_CLEAR = 1\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.progress".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([(
                        MINORI_GLOBAL_PROGRESS_OPTION.into(),
                        MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
                    )]),
                },
            )
            .unwrap();

        let mut load_input = step_input(1, Vec::new());
        load_input.input_edges = vec![astra_emu_family_api::LegacyInputEdge {
            control: MINORI_CONTROL_KEY.into(),
            pressed: true,
            value: 1.0,
            sequence: 1,
        }];
        let load = provider.step(&ctx, &session, load_input).unwrap();
        assert_eq!(load.status, LegacyRuntimeStatus::Awaiting);
        let LegacyWaitRequest::ProviderCompletion {
            request_id: load_request_id,
            provider_id,
            operation,
            key,
            payload,
            ..
        } = &load.control.waits[0]
        else {
            panic!("expected platform storage load")
        };
        assert_eq!(provider_id, MINORI_PLATFORM_STORAGE_PROVIDER_ID);
        assert_eq!(operation, "read");
        assert_eq!(key, MINORI_GLOBAL_PROGRESS_SLOT);
        assert!(payload.is_empty());

        let mut load_result = step_input(2, Vec::new());
        load_result.provider_results = vec![astra_emu_family_api::LegacyProviderResult {
            request_id: load_request_id.clone(),
            provider_id: MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
            status: "missing".into(),
            payload: Vec::new(),
            sequence: 1,
        }];
        let store = provider.step(&ctx, &session, load_result).unwrap();
        assert_eq!(store.status, LegacyRuntimeStatus::Awaiting);
        let LegacyWaitRequest::ProviderCompletion {
            request_id: store_request_id,
            operation,
            payload,
            ..
        } = &store.control.waits[0]
        else {
            panic!("expected platform storage write")
        };
        assert_eq!(operation, "write");
        assert_eq!(
            decode_global_progress(payload).unwrap(),
            [Hash256::from_sha256(b"REN_CLEAR")]
        );
        let stored_progress = payload.clone();

        let mut store_result = step_input(3, Vec::new());
        store_result.provider_results = vec![astra_emu_family_api::LegacyProviderResult {
            request_id: store_request_id.clone(),
            provider_id: MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
            status: "completed".into(),
            payload: Vec::new(),
            sequence: 2,
        }];
        let terminal = provider.step(&ctx, &session, store_result).unwrap();
        assert_eq!(terminal.status, LegacyRuntimeStatus::Terminal);
        assert!(terminal.control.waits.is_empty());

        let snapshot = provider.save(&ctx, &session).unwrap();
        provider.restore(&ctx, &session, &snapshot).unwrap();
        let restored_snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(restored_snapshot.family_sections, snapshot.family_sections);

        let reopened = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.progress.reopen".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([(
                        MINORI_GLOBAL_PROGRESS_OPTION.into(),
                        MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
                    )]),
                },
            )
            .unwrap();
        let load = provider
            .step(&ctx, &reopened, step_input(1, Vec::new()))
            .unwrap();
        let LegacyWaitRequest::ProviderCompletion { request_id, .. } = &load.control.waits[0]
        else {
            panic!("expected reopened platform storage load")
        };
        let mut load_result = step_input(2, Vec::new());
        load_result.provider_results = vec![astra_emu_family_api::LegacyProviderResult {
            request_id: request_id.clone(),
            provider_id: MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
            status: "completed".into(),
            payload: stored_progress,
            sequence: 1,
        }];
        let terminal = provider.step(&ctx, &reopened, load_result).unwrap();
        assert_eq!(terminal.status, LegacyRuntimeStatus::Terminal);
        assert!(terminal.control.waits.is_empty());
        assert_eq!(
            provider.sessions[&reopened.0].vm.state().gallery_unlocks,
            [Hash256::from_sha256(b"REN_CLEAR")]
        );
    }

    #[test]
    fn provider_snapshots_quiescent_unloaded_global_progress_for_restore_rollback() {
        let script = b".end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let request = LegacyOpenRequest {
            requested_session_id: LegacyRuntimeSessionId("session.progress.rollback".into()),
            case_fingerprint: Hash256::from_sha256(b"case"),
            script_uri: "minori:/scr/test.sc".into(),
            fixed_delta_ns: 16_666_667,
            session_seed: 7,
            compatibility_profile: "minori.reference".into(),
            family_options: BTreeMap::from([(
                MINORI_GLOBAL_PROGRESS_OPTION.into(),
                MINORI_PLATFORM_STORAGE_PROVIDER_ID.into(),
            )]),
        };
        let session = provider.open(&ctx, request.clone()).unwrap();
        let unlock = Hash256::from_sha256(b"REN_CLEAR");
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .vm
            .merge_verified_gallery_unlocks(&[unlock])
            .unwrap();
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .global_progress = MinoriGlobalProgressSession {
            enabled: true,
            loaded: true,
            persisted_unlocks: vec![unlock],
            pending: None,
        };
        let loaded_snapshot = provider.save(&ctx, &session).unwrap();
        provider.shutdown(&ctx, &session).unwrap();

        let reopened = provider.open(&ctx, request).unwrap();
        let rollback_snapshot = provider.save(&ctx, &reopened).unwrap();
        let rollback_progress: MinoriGlobalProgressSnapshotV1 =
            postcard::from_bytes(&rollback_snapshot.family_sections[1].bytes).unwrap();
        assert!(!rollback_progress.loaded);
        assert!(rollback_progress.persisted_unlocks.is_empty());

        provider.restore(&ctx, &reopened, &loaded_snapshot).unwrap();
        assert!(provider.sessions[&reopened.0].global_progress.loaded);
        assert_eq!(
            provider.sessions[&reopened.0]
                .global_progress
                .persisted_unlocks,
            [unlock]
        );

        provider
            .restore(&ctx, &reopened, &rollback_snapshot)
            .unwrap();
        assert!(!provider.sessions[&reopened.0].global_progress.loaded);
        let load = provider
            .step(&ctx, &reopened, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(load.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            load.control.waits.as_slice(),
            [LegacyWaitRequest::ProviderCompletion { operation, .. }] if operation == "read"
        ));
    }

    #[test]
    fn provider_title_launch_uses_verified_system_assets_and_restores_page_state() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let encode_rgba = |width: u32, height: u32| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &vec![0; usize::try_from(width * height * 4).unwrap()],
                    width,
                    height,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            png
        };
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
                ("minori:/sys/topMenu0.png".into(), page_png.clone()),
                ("minori:/sys/configBase.png".into(), page_png.clone()),
                ("minori:/sys/knob.png".into(), encode_rgba(15, 25)),
                ("minori:/sys/checkmark.png".into(), encode_rgba(21, 32)),
                ("minori:/sys/circle.png".into(), encode_rgba(74, 74)),
                (
                    "minori:/sys/BGMTest.wav".into(),
                    b"RIFF\x04\0\0\0WAVE".to_vec(),
                ),
                ("minori:/sys/saveloadBase.png".into(), page_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.title".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        let title = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(title.status, LegacyRuntimeStatus::Active);
        assert_eq!(title.live.resource_scenes.len(), 1);
        assert_eq!(title.control.blackboard.len(), 4);
        assert!(title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.system_page" && mutation.value == "title" }));
        assert!(title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "0"
        }));
        assert!(title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "false"
        }));
        assert!(title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.play_mode" && mutation.value == "normal" }));
        assert_eq!(
            title.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/topMenu0.png"
        );
        let retained_title = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert!(retained_title.live.resource_scenes.is_empty());
        assert!(retained_title.control.blackboard.is_empty());
        let snapshot = provider.save(&ctx, &session).unwrap();
        let title_revision = title.live.resource_scenes[0].value.texture_resources[0].revision;
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );

        let config = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: "arrow_down".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: "arrow_down".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: "enter".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(
            config.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/configBase.png"
        );
        assert_eq!(
            config.live.resource_scenes[0].value.texture_resources.len(),
            4
        );
        assert_eq!(config.live.resource_scenes[0].value.draws.len(), 18);
        assert_eq!(config.control.blackboard.len(), 1);
        assert_eq!(config.control.blackboard[0].value, "config");
        assert_ne!(
            config.live.resource_scenes[0].value.texture_resources[0].revision,
            title_revision
        );
        let audio_test = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: false,
                            value: 750.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: false,
                            value: 130.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_PRIMARY.into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert!(matches!(
            audio_test.live.audio_commands.as_slice(),
            [
                LegacySequenced {
                    value: LegacyAudioCommandV1::LoadResource {
                        encoding: LegacyAudioEncoding::Wav,
                        resource_uri,
                        ..
                    },
                    ..
                },
                LegacySequenced {
                    value: LegacyAudioCommandV1::Play { repeat: false, .. },
                    ..
                }
            ] if resource_uri == "minori:/sys/BGMTest.wav"
        ));
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: MINORI_POINTER_PRIMARY.into(),
                        pressed: false,
                        value: 0.0,
                        sequence: 1,
                    }],
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        let title_after_config = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(title_after_config.status, LegacyRuntimeStatus::Active);
        assert_eq!(title_after_config.control.blackboard.len(), 1);
        assert_eq!(title_after_config.control.blackboard[0].value, "title");
        assert_eq!(
            title_after_config.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/topMenu0.png"
        );

        provider.restore(&ctx, &session, &snapshot).unwrap();
        let returned_to_title = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(returned_to_title.status, LegacyRuntimeStatus::Active);
        assert_eq!(returned_to_title.control.blackboard.len(), 5);
        assert!(returned_to_title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.system_page" && mutation.value == "title" }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "0"
        }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "false"
        }));
        assert!(returned_to_title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.play_mode" && mutation.value == "normal" }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.route_complete" && mutation.value == "true"
        }));
        assert_eq!(returned_to_title.live.resource_scenes.len(), 1);
        assert_eq!(
            returned_to_title.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/topMenu0.png"
        );
    }

    #[test]
    fn config_hit_map_clamps_sliders_and_preserves_original_action_regions() {
        assert_eq!(
            config_control_at(40, 160),
            Some(MinoriConfigControl::MessageSpeedUnread(0))
        );
        assert_eq!(
            config_control_at(151, 160),
            Some(MinoriConfigControl::MessageSpeedUnread(50))
        );
        assert_eq!(
            config_control_at(259, 160),
            Some(MinoriConfigControl::MessageSpeedUnread(100))
        );
        assert_eq!(
            config_control_at(700, 130),
            Some(MinoriConfigControl::ToggleBgmMute)
        );
        assert_eq!(
            config_control_at(750, 205),
            Some(MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Voice))
        );
        assert_eq!(
            config_control_at(610, 620),
            Some(MinoriConfigControl::Apply)
        );
        assert_eq!(
            config_control_at(80, 600),
            Some(MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Auto))
        );
        assert_eq!(
            config_control_at(200, 600),
            Some(MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Skip))
        );
        assert_eq!(
            config_control_at(250, 500),
            Some(MinoriConfigControl::FontPrevious)
        );
        assert_eq!(config_control_at(0, 0), None);
    }

    #[test]
    fn auto_menu_rebinds_the_active_message_wait_without_a_manual_advance() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let source = b".message\r\n.end\r\n";
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), source.to_vec()),
                ("minori:/sys/topMenu0.png".into(), page_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.auto-rebind".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let message = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        let token_id = match message.control.waits.as_slice() {
            [LegacyWaitRequest::Input { token_id, .. }] => token_id.clone(),
            _ => panic!("expected the initial message input wait"),
        };
        let rebound = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: false,
                            value: 1125.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: false,
                            value: 577.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_PRIMARY.into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert!(matches!(
            rebound.control.waits.as_slice(),
            [LegacyWaitRequest::Time {
                token_id: rebound_token,
                milliseconds: 500,
            }] if rebound_token == &token_id
        ));
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.play_mode,
            MinoriPlayMode::Auto
        );
    }

    #[test]
    fn config_volume_and_mute_are_applied_at_the_shared_audio_boundary() {
        let source = b".end\r\n";
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap(),
            7,
        )
        .unwrap();
        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.system_ui.config.bgm_volume = 40;
        state.audio.insert(
            0,
            crate::MinoriAudioState {
                bus: "bgm".into(),
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: "minori:/bgm/test.ogg".into(),
                looped: true,
                volume_milli: 500,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        let (_, mapped) = map_audio_command(
            &MinoriAudioCommand::SetParams {
                sequence: 1,
                stream_id: 0,
                volume: 0.5,
                pan: 0.0,
                repeat: true,
            },
            vm.state(),
        )
        .unwrap();
        assert!(matches!(
            mapped,
            LegacyAudioCommandV1::SetParams { volume, .. } if (volume - 0.2).abs() < f32::EPSILON
        ));
        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.system_ui.config.bgm_muted = true;
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        assert_eq!(effective_audio_volume(vm.state(), 0, 0.5).unwrap(), 0.0);

        state.system_ui.config.se_volume = 25;
        state.audio.insert(
            2,
            crate::MinoriAudioState {
                bus: "se2".into(),
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: "minori:/se/test.ogg".into(),
                looped: true,
                volume_milli: 800,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        assert_eq!(effective_audio_volume(vm.state(), 2, 0.8).unwrap(), 0.2);
    }

    #[test]
    fn provider_exposes_original_memories_order_only_for_the_verified_title_variant() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
                ("minori:/sys/topMenu2.png".into(), page_png.clone()),
                ("minori:/sys/memories.png".into(), page_png.clone()),
                ("minori:/sys/cgmode0.png".into(), page_png.clone()),
                ("minori:/sys/flash0.png".into(), page_png.clone()),
                ("minori:/sys/musicPage1.png".into(), page_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.memories".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .vm
            .merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();

        let title = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(
            title.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/topMenu2.png"
        );
        let select_page = |tick: u64, down_count: usize| LegacyStepInput {
            input_edges: (0..down_count)
                .map(|index| LegacyInputEdge {
                    control: "arrow_down".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: index as u64 + 1,
                })
                .chain(std::iter::once(LegacyInputEdge {
                    control: "enter".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: down_count as u64 + 1,
                }))
                .collect(),
            ..step_input(tick, Vec::new())
        };
        let memories = provider.step(&ctx, &session, select_page(2, 3)).unwrap();
        assert_eq!(
            memories.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/memories.png"
        );
        let cg = provider.step(&ctx, &session, select_page(3, 0)).unwrap();
        assert_eq!(
            cg.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/cgmode0.png"
        );
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "escape".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        let replay = provider.step(&ctx, &session, select_page(5, 1)).unwrap();
        assert_eq!(
            replay.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/flash0.png"
        );
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "escape".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        let bgm = provider.step(&ctx, &session, select_page(7, 2)).unwrap();
        assert_eq!(
            bgm.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/musicPage1.png"
        );
    }

    #[test]
    fn shipping_session_does_not_collect_evidence_vm_trace() {
        let script = b".end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let mut ctx = context();
        ctx.target = "windows".into();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.shipping".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([(
                        "astra.hosted_trace_profile".into(),
                        "shipping".into(),
                    )]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert!(shutdown.evidence_vm_trace.is_empty());
    }

    #[test]
    fn provider_tail_chains_and_restores_the_active_script_identity() {
        let entry = b".set local = 1\r\n.chain K01.sc\r\n".to_vec();
        let next = b".wait 20\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), entry),
                ("minori:/scr/K01.sc".into(), next),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.chain".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let chained = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(chained.status, LegacyRuntimeStatus::Active);
        assert_eq!(chained.trace[0].action.as_deref(), Some("chain"));

        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let snapshot = provider.save(&ctx, &session).unwrap();
        provider.restore(&ctx, &session, &snapshot).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
    }

    #[test]
    fn provider_exposes_message_plaintext_only_through_a_one_shot_lease() {
        let script =
            b".message 42  speaker hello world\r\n.message 43  speaker second\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.message".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Awaiting);
        let presentation = &output.live.text_presentations[0];
        assert_eq!(presentation.sequence, 1);
        let lease = &output.live.text[0];
        assert_eq!(lease.sequence, 2);
        assert_eq!(presentation.value.lease_id, lease.lease_id);
        assert_eq!(lease.byte_len, 11);
        assert_eq!(lease.source_ref, "minori.sc.message");
        let presentation = &presentation.value.presentation;
        assert_eq!(presentation.layout_id, "minori.message");
        assert_eq!(presentation.language, "ja-JP");
        assert_eq!(presentation.font_families, ["Noto Sans JP"]);
        assert_eq!(presentation.body.font_size, 26.0);
        assert_eq!(presentation.body.max_lines, 3);
        let text = provider
            .take_ephemeral_text(&ctx, &session, &lease.lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(text.text, "hello world");
        assert_eq!(text.speaker.as_deref(), Some("speaker"));
        assert!(provider
            .take_ephemeral_text(&ctx, &session, &lease.lease_id)
            .unwrap()
            .is_none());
        assert!(matches!(
            output.control.waits.as_slice(),
            [LegacyWaitRequest::Input { keys, .. }] if *keys == message_input_keys()
        ));
        let wait_token = match &output.control.waits[0] {
            LegacyWaitRequest::Input { token_id, .. } => token_id.clone(),
            _ => unreachable!("message output was already verified as an input wait"),
        };

        let snapshot = provider.save(&ctx, &session).unwrap();
        provider.restore(&ctx, &session, &snapshot).unwrap();
        let restored = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(restored.status, LegacyRuntimeStatus::Awaiting);
        assert!(restored.live.clear_text);
        assert_eq!(restored.live.text_presentations.len(), 1);
        assert_eq!(restored.live.text.len(), 1);
        assert_eq!(restored.live.text[0].source_ref, "minori.sc.message.resume");
        let restored_text = provider
            .take_ephemeral_text(&ctx, &session, &restored.live.text[0].lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored_text.text, "hello world");
        assert_eq!(restored_text.speaker.as_deref(), Some("speaker"));

        provider.restore(&ctx, &session, &snapshot).unwrap();
        let continued = provider
            .step(
                &ctx,
                &session,
                step_input(
                    2,
                    vec![LegacyAwaitResult {
                        token_id: wait_token,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(continued.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(continued.live.resource_scenes.len(), 1);
        assert_eq!(continued.live.text_presentations.len(), 1);
        assert_eq!(continued.live.text.len(), 1);
    }

    #[test]
    fn provider_game_menu_click_toggles_auto_without_advancing_as_message_click() {
        let script =
            b".message 1  speaker first\r\n.message 2  speaker second\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.auto".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let initial_token_id = match &first.control.waits[0] {
            LegacyWaitRequest::Input { token_id, .. } => token_id.clone(),
            _ => panic!("expected message input wait"),
        };
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: true,
                            value: 1125.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: true,
                            value: 577.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_PRIMARY.into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            output.control.waits.as_slice(),
            [LegacyWaitRequest::Time {
                token_id: rebound_token,
                milliseconds: 500,
            }] if rebound_token == &initial_token_id
        ));
        assert!(matches!(
            provider.sessions[&session.0].vm.state().wait,
            Some(MinoriWaitState::Time {
                ref token_id,
                timer_ticks: 50,
                milliseconds: 500,
            }) if token_id == &initial_token_id
        ));
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.play_mode,
            MinoriPlayMode::Auto
        );
        assert!(output
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.play_mode" && mutation.value == "auto" }));
    }

    #[test]
    fn held_control_rebinds_the_active_provider_message_wait() {
        let script = b".pragma enable_control\r\n.message 1  speaker first\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.control-rebind".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let token_id = match first.control.waits.as_slice() {
            [LegacyWaitRequest::Input { token_id, .. }] => token_id.clone(),
            _ => panic!("expected message input wait"),
        };

        let pressed = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: MINORI_CONTROL_KEY.into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert!(matches!(
            pressed.control.waits.as_slice(),
            [LegacyWaitRequest::Time {
                token_id: rebound_token,
                milliseconds: 10,
            }] if rebound_token == &token_id
        ));

        let released = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: MINORI_CONTROL_KEY.into(),
                        pressed: false,
                        value: 0.0,
                        sequence: 2,
                    }],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert!(matches!(
            released.control.waits.as_slice(),
            [LegacyWaitRequest::Input {
                token_id: rebound_token,
                ..
            }] if rebound_token == &token_id
        ));
    }

    #[test]
    fn provider_suspends_message_wait_for_verified_backlog_wheel_navigation() {
        let script =
            b".panel 1\r\n.message 42 voice[50,-25] speaker hello world\r\n.end\r\n".to_vec();
        let mut panel_png = Vec::new();
        PngEncoder::new(&mut panel_png)
            .write_image(&vec![255; 4 * 263], 1, 263, ExtendedColorType::Rgba8)
            .unwrap();
        let mut gauge_png = Vec::new();
        PngEncoder::new(&mut gauge_png)
            .write_image(&vec![255; 18 * 144 * 4], 18, 144, ExtendedColorType::Rgba8)
            .unwrap();
        let mut ball_png = Vec::new();
        PngEncoder::new(&mut ball_png)
            .write_image(&vec![255; 14 * 14 * 4], 14, 14, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/msgPanel.png".into(), panel_png),
                ("minori:/sys/backlogGauge.png".into(), gauge_png),
                ("minori:/sys/ball.png".into(), ball_png),
                ("minori:/voice/voice".into(), b"OggSfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.backlog".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let panel = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(panel.status, LegacyRuntimeStatus::Active);
        let message = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(message.status, LegacyRuntimeStatus::Awaiting);

        let backlog = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "wheel".into(),
                        pressed: false,
                        value: -120.0,
                        sequence: 1,
                    }],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(backlog.status, LegacyRuntimeStatus::Active);
        assert!(backlog.live.clear_text);
        let frame = &backlog.live.resource_scenes[0].value;
        assert_eq!(frame.texture_resources.len(), 3);
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.resource_uri.as_str())
                .collect::<Vec<_>>(),
            [
                "minori:/sys/msgPanel.png",
                "minori:/sys/backlogGauge.png",
                "minori:/sys/ball.png",
            ]
        );
        assert_eq!(frame.draws.last().unwrap().vertices[0].position, [2.0, 3.0]);
        assert_eq!(backlog.live.text_presentations.len(), 1);
        assert_eq!(backlog.live.text.len(), 1);
        assert_eq!(backlog.live.text[0].source_ref, "minori.sc.backlog");
        let backlog_text = provider
            .take_ephemeral_text(&ctx, &session, &backlog.live.text[0].lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(backlog_text.text, "hello world");
        assert_eq!(backlog_text.speaker.as_deref(), Some("speaker"));
        let retained = provider
            .step(&ctx, &session, step_input(4, Vec::new()))
            .unwrap();
        assert!(!retained.live.clear_text);
        assert!(retained.live.resource_scenes.is_empty());
        assert!(retained.live.text_presentations.is_empty());
        assert!(retained.live.text.is_empty());
        let wait_before_replay = provider.sessions[&session.0].vm.state().wait.clone();
        let replay = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 2,
                    }],
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(replay.status, LegacyRuntimeStatus::Active);
        assert_eq!(replay.live.audio_commands.len(), 3);
        assert!(matches!(
            replay.live.audio_commands.as_slice(),
            [
                LegacySequenced {
                    value: LegacyAudioCommandV1::Stop { stream_id: 4, .. },
                    ..
                },
                LegacySequenced {
                    value: LegacyAudioCommandV1::LoadResource { stream_id: 4, resource_uri, .. },
                    ..
                },
                LegacySequenced {
                    value: LegacyAudioCommandV1::Play { stream_id: 4, volume, pan, repeat: false, .. },
                    ..
                }
            ] if resource_uri == "minori:/voice/voice" && *volume == 0.5 && *pan == -0.25
        ));
        assert_eq!(
            provider.sessions[&session.0].vm.state().wait,
            wait_before_replay
        );
        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider.restore(&ctx, &session, &snapshot).unwrap();

        let resumed = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "wheel".into(),
                        pressed: false,
                        value: 120.0,
                        sequence: 3,
                    }],
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(resumed.status, LegacyRuntimeStatus::Awaiting);
        assert!(resumed.live.clear_text);
        assert_eq!(resumed.live.text_presentations.len(), 1);
        assert_eq!(resumed.live.text.len(), 1);
        assert_eq!(resumed.live.text[0].source_ref, "minori.sc.message.resume");
    }

    #[test]
    fn provider_resolves_input_waits_from_canonical_key_edges() {
        let script = b".message 42  speaker hello\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.message.input".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        assert!(output.control.waits.is_empty());
    }

    #[test]
    fn provider_moves_and_commits_choice_from_canonical_key_edges() {
        assert_eq!(MINORI_CHOICE_RESOURCE_URIS[0], "minori:/sys/SelectBLur.png");
        let script = b".char load 100 CH.png\r\n.select first:label1 second:label2\r\n.label label1\r\n.end\r\n.label label2\r\n.end\r\n".to_vec();
        let mut choice_png = Vec::new();
        PngEncoder::new(&mut choice_png)
            .write_image(&vec![255; 320 * 48 * 4], 320, 48, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                (MINORI_CHOICE_RESOURCE_URIS[0].into(), choice_png.clone()),
                (MINORI_CHOICE_RESOURCE_URIS[1].into(), choice_png.clone()),
                (MINORI_CHOICE_RESOURCE_URIS[2].into(), choice_png.clone()),
                ("minori:/st/CH.png".into(), choice_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.choice".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let character = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(character.status, LegacyRuntimeStatus::Active);
        let first = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(first.status, LegacyRuntimeStatus::Awaiting);
        assert!(first.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "true"
        }));
        assert!(matches!(
            first.control.waits.as_slice(),
            [LegacyWaitRequest::Input { keys, .. }] if *keys == choice_input_keys()
        ));
        let first_presentation = first
            .control
            .events
            .iter()
            .find(|event| event.event == MINORI_CHOICE_PRESENTATION_SCHEMA)
            .expect("choice presentation event");
        let expected = choice_presentation_from_parts(
            &[
                Hash256::from_sha256(b"first"),
                Hash256::from_sha256(b"second"),
            ],
            0,
            first_presentation.sequence,
        )
        .unwrap();
        assert_eq!(first_presentation, &expected);
        assert_eq!(first.live.resource_scenes.len(), 1);
        let choice_frame = &first.live.resource_scenes.last().unwrap().value;
        let texture_ids = choice_frame
            .texture_resources
            .iter()
            .map(|resource| resource.texture_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(texture_ids.len(), choice_frame.texture_resources.len());
        assert!(texture_ids.contains(&(MINORI_CHARACTER_TEXTURE_BASE + 100)));
        assert!(texture_ids.contains(&MINORI_CHOICE_TEXTURE_BASE));
        assert_eq!(first.live.text_presentations.len(), 2);
        assert_eq!(first.live.text.len(), 2);
        assert!(first.live.text_presentations.iter().all(|binding| binding
            .value
            .presentation
            .body
            .horizontal_alignment
            == LegacyTextHorizontalAlignmentV1::Center));

        let snapshot = provider.save(&ctx, &session).unwrap();
        provider.restore(&ctx, &session, &snapshot).unwrap();
        let restored = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(restored.status, LegacyRuntimeStatus::Awaiting);
        assert!(restored.live.clear_text);
        assert_eq!(restored.live.resource_scenes.len(), 2);
        assert_eq!(restored.live.text_presentations.len(), 2);
        assert_eq!(restored.live.text.len(), 2);

        let moved = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "arrow_down".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(moved.status, LegacyRuntimeStatus::Awaiting);
        let moved_presentation = moved
            .control
            .events
            .iter()
            .find(|event| event.event == MINORI_CHOICE_PRESENTATION_SCHEMA)
            .expect("updated choice presentation event");
        let expected = choice_presentation_from_parts(
            &[
                Hash256::from_sha256(b"first"),
                Hash256::from_sha256(b"second"),
            ],
            1,
            moved_presentation.sequence,
        )
        .unwrap();
        assert_eq!(moved_presentation, &expected);
        assert_eq!(moved.live.resource_scenes.len(), 1);
        assert_eq!(moved.live.text_presentations.len(), 2);
        assert_eq!(moved.live.text.len(), 2);

        let completed = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 2,
                    }],
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        assert!(completed.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "false"
        }));
        assert!(completed.control.waits.is_empty());
        assert!(completed.live.clear_text);
    }

    #[test]
    fn provider_control_edge_enables_bounded_timer_fast_forward() {
        let script = b".pragma enable_control\r\n.wait 500\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.control.skip".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "control".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(1, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        assert!(output.control.waits.is_empty());
    }

    #[test]
    fn provider_control_hold_stops_only_a_script_skippable_movie() {
        let script =
            b".pragma disable_control\r\n.movie 9989 op.avi 1280 720 t\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/mov/op.avi".into(), b"RIFFfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.movie.skip".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let started = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "control".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(1, Vec::new())
                },
            )
            .unwrap();
        let (token_id, media_id) = match started.control.waits.as_slice() {
            [LegacyWaitRequest::MediaFence { token_id, media_id }] => {
                (token_id.clone(), media_id.clone())
            }
            _ => panic!("expected media fence"),
        };
        assert!(matches!(
            started.live.video.as_slice(),
            [LegacySequenced {
                value: LegacyVideoCommandV1::Play { playback_id, .. },
                ..
            }] if playback_id == &media_id
        ));

        let stopping = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(stopping.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            stopping.live.video.as_slice(),
            [LegacySequenced {
                value: LegacyVideoCommandV1::Stop { playback_id },
                ..
            }] if playback_id == &media_id
        ));

        let completed = provider
            .step(
                &ctx,
                &session,
                step_input(
                    3,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
    }

    #[test]
    fn provider_blocks_message_without_the_verified_reference_stage() {
        let script = b".message 42 voice speaker body\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.message.invalid-stage".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        assert_eq!(
            provider
                .step(&ctx, &session, step_input(1, Vec::new()))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY"
        );
    }

    #[test]
    fn provider_validates_and_emits_bgm_through_the_shared_audio_contract() {
        let script = b".playBGM theme.ogg * * 80\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bgm/theme.ogg".into(), b"OggSfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.bgm".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Active);
        assert_eq!(output.coverage.audio_commands, 2);
        assert_eq!(output.live.audio_commands.len(), 2);
        let commands = output
            .live
            .audio_commands
            .iter()
            .map(|command| command.value.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            commands[0],
            LegacyAudioCommandV1::LoadResource {
                stream_id: 0,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: "minori:/bgm/theme.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            LegacyAudioCommandV1::Play {
                stream_id: 0,
                volume: 0.8,
                pan: 0.0,
                repeat: true,
                fade_in_ms: 2,
            }
        );

        let snapshot = provider.save(&ctx, &session).unwrap();
        let terminal = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(terminal.status, LegacyRuntimeStatus::Terminal);
        provider.restore(&ctx, &session, &snapshot).unwrap();
        let restored = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(restored.status, LegacyRuntimeStatus::Terminal);
        assert_eq!(restored.coverage.audio_commands, 2);
        assert_eq!(restored.live.audio_commands.len(), 2);
        assert!(matches!(
            &restored.live.audio_commands[0].value,
            LegacyAudioCommandV1::LoadResource { stream_id: 0, .. }
        ));
        assert!(matches!(
            &restored.live.audio_commands[1].value,
            LegacyAudioCommandV1::Play {
                stream_id: 0,
                volume,
                repeat: true,
                fade_in_ms: 0,
                ..
            } if *volume == 0.8
        ));
    }

    #[test]
    fn provider_emits_a_resource_bound_stage_frame_without_decoded_pixels() {
        let script = b".transition 0 * 10\r\n.stage * BLACK.png 0 0\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BLACK.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.stage".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let frame = &output.live.resource_scenes[0].value;
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.texture_resources.len(), 1);
        assert_eq!(frame.draws.len(), 1);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/bg/BLACK.png"
        );
        assert_eq!(frame.texture_resources[0].decoded_width, 1);
        assert_eq!(frame.texture_resources[0].decoded_height, 1);
    }

    #[test]
    fn provider_maps_axis_scroll_to_time_wait_and_resource_frame_updates() {
        let script =
            b".stage * BG.png 461 0\r\n.hscroll 0 -10\r\n.endscroll 0\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.axis-scroll".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(started.trace[0].action.as_deref(), Some("axis_scroll"));

        let waiting = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let token = match &waiting.control.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 445);
                token_id.clone()
            }
            _ => panic!("expected axis-scroll time wait"),
        };
        let frame = &waiting
            .live
            .resource_scenes
            .last()
            .expect("expected axis-scroll presentation")
            .value;
        assert_eq!(frame.draws[0].vertices[0].position, [445.0, 0.0]);

        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        let terminal = provider
            .step(
                &ctx,
                &session,
                step_input(
                    4,
                    vec![LegacyAwaitResult {
                        token_id: token,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(terminal.status, LegacyRuntimeStatus::Terminal);
    }

    #[test]
    fn provider_centers_and_bottom_anchors_verified_static_png_stands() {
        let script = b".stage * BG.png 0 0 STAND.png 727,1685\r\n.end\r\n".to_vec();
        let mut background = Vec::new();
        PngEncoder::new(&mut background)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut stand = Vec::new();
        let stand_pixels = vec![255; 1203 * 773 * 4];
        PngEncoder::new(&mut stand)
            .write_image(&stand_pixels, 1203, 773, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background),
                ("minori:/st/STAND.png".into(), stand),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.stage-stand".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let frame = &output
            .live
            .resource_scenes
            .last()
            .expect("expected stage presentation")
            .value;
        assert_eq!(frame.texture_resources.len(), 2);
        assert_eq!(frame.draws.len(), 2);
        assert_eq!(
            frame.texture_resources[1].resource_uri,
            "minori:/st/STAND.png"
        );
        assert_eq!(frame.draws[1].vertices[0].position, [126.0, -53.0]);
        assert_eq!(frame.draws[1].vertices[3].position, [1329.0, 720.0]);

        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider.restore(&ctx, &session, &snapshot).unwrap();
    }

    #[test]
    fn provider_composes_signed_character_slots_with_native_anchor_and_orientation() {
        let script =
            b".stage * BG.png 0 0\r\n.char load -11 CH.png\r\n.char pos -11 727 0\r\n.end\r\n"
                .to_vec();
        let mut background = Vec::new();
        PngEncoder::new(&mut background)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut character = Vec::new();
        PngEncoder::new(&mut character)
            .write_image(
                &vec![255; 100 * 200 * 4],
                100,
                200,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background),
                ("minori:/st/CH.png".into(), character),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.character".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Active);
        assert_eq!(output.trace[0].action.as_deref(), Some("character"));
        let frame = &output
            .live
            .resource_scenes
            .last()
            .expect("expected character presentation")
            .value;
        assert_eq!(frame.texture_resources.len(), 2);
        assert_eq!(frame.draws.len(), 2);
        let draw = &frame.draws[1];
        assert_eq!(draw.texture_id, 10_011);
        assert_eq!(draw.vertices[0].position, [677.0, 520.0]);
        assert_eq!(draw.vertices[3].position, [777.0, 720.0]);
        assert_eq!(draw.vertices[0].tex_coord, [1.0, 0.0]);
        assert_eq!(draw.vertices[1].tex_coord, [0.0, 0.0]);
        assert_eq!(
            draw.scissor,
            Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        );

        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider.restore(&ctx, &session, &snapshot).unwrap();
    }

    #[test]
    fn provider_emits_retained_character_frames_during_blocking_transition() {
        let script =
            b".stage * BG.png 0 0\r\n.char load 11 CH.png\r\n.char trans 11 100 0\r\n.end\r\n"
                .to_vec();
        let mut background = Vec::new();
        PngEncoder::new(&mut background)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut character = Vec::new();
        PngEncoder::new(&mut character)
            .write_image(&[255; 4 * 4 * 4], 4, 4, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background),
                ("minori:/st/CH.png".into(), character),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.character-transition".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 50_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(1, 50_000_000))
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(2, 50_000_000))
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input_with_delta(3, 50_000_000))
            .unwrap();
        assert_eq!(started.status, LegacyRuntimeStatus::Awaiting);
        let token_id = match &started.control.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 100);
                token_id.clone()
            }
            _ => panic!("expected transition timer"),
        };

        let midpoint = provider
            .step(&ctx, &session, step_input_with_delta(4, 50_000_000))
            .unwrap();
        assert_eq!(midpoint.status, LegacyRuntimeStatus::Awaiting);
        let draw = midpoint.live.resource_scenes[0].value.draws.last().unwrap();
        assert_eq!(draw.vertices[0].color[3], 0.5);

        let completed = provider
            .step(
                &ctx,
                &session,
                step_input_with_delta_and_await(
                    5,
                    50_000_000,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        assert_eq!(
            provider.sessions[&session.0].vm.state().characters[&11].opacity_256,
            0
        );
    }

    #[test]
    fn provider_stage_frames_retire_unmarked_character_slots_after_one_keep() {
        let script = b".stage * BG.png 0 0\r\n.char load 11 CH1.png\r\n.char load 12 CH2.png\r\n.char keep 11\r\n.stage * BG2.png 0 0\r\n.stage * BG3.png 0 0\r\n.end\r\n"
            .to_vec();
        let png = |rgba: [u8; 4]| {
            let mut encoded = Vec::new();
            PngEncoder::new(&mut encoded)
                .write_image(&rgba, 1, 1, ExtendedColorType::Rgba8)
                .unwrap();
            encoded
        };
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png([0, 0, 0, 255])),
                ("minori:/bg/BG2.png".into(), png([1, 1, 1, 255])),
                ("minori:/bg/BG3.png".into(), png([2, 2, 2, 255])),
                ("minori:/st/CH1.png".into(), png([3, 3, 3, 255])),
                ("minori:/st/CH2.png".into(), png([4, 4, 4, 255])),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.character-stage-retention".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        for tick in 1..=4 {
            provider
                .step(&ctx, &session, step_input(tick, Vec::new()))
                .unwrap();
        }
        let retained = provider
            .step(&ctx, &session, step_input(5, Vec::new()))
            .unwrap();
        let retained_frame = &retained.live.resource_scenes[0].value;
        assert_eq!(retained_frame.texture_resources.len(), 2);
        assert!(retained_frame
            .texture_resources
            .iter()
            .any(|resource| resource.texture_id == MINORI_CHARACTER_TEXTURE_BASE + 11));
        assert!(!retained_frame
            .texture_resources
            .iter()
            .any(|resource| resource.texture_id == MINORI_CHARACTER_TEXTURE_BASE + 12));

        let discarded = provider
            .step(&ctx, &session, step_input(6, Vec::new()))
            .unwrap();
        let discarded_frame = &discarded.live.resource_scenes[0].value;
        assert_eq!(discarded_frame.texture_resources.len(), 1);
        assert!(discarded_frame
            .texture_resources
            .iter()
            .all(|resource| resource.texture_id < MINORI_CHARACTER_TEXTURE_BASE));
    }

    #[test]
    fn scroll_xf_clips_translates_and_remaps_stage_draws() {
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let mut frame = LegacyRenderResourceFrameV1 {
            width: 4,
            height: 2,
            texture_resources: Vec::new(),
            draws: vec![LegacyDrawV1 {
                texture_id: 1,
                vertices: [
                    vertex(0.0, 0.0, 0.0, 0.0),
                    vertex(4.0, 0.0, 1.0, 0.0),
                    vertex(0.0, 2.0, 0.0, 1.0),
                    vertex(4.0, 2.0, 1.0, 1.0),
                ],
                blend: LegacyBlendMode::Alpha,
                texture_filter: LegacyTextureFilter::Linear,
                scissor: None,
            }],
        };
        let scroll = crate::MinoriScrollXfState {
            start_extent: [2, 2],
            end_extent: [2, 2],
            start_offset: [1, 0],
            end_offset: [1, 0],
            duration_ms: 1000,
            easing: 0,
            elapsed_ns: 0,
            completed: false,
            visible_extent: [2, 2],
            visible_offset: [1, 0],
        };
        apply_scroll_xf_to_frame(&mut frame, &scroll).unwrap();
        assert_eq!(frame.draws.len(), 1);
        let draw = &frame.draws[0];
        assert_eq!(draw.vertices[0].position, [0.0, 0.0]);
        assert_eq!(draw.vertices[3].position, [2.0, 2.0]);
        assert_eq!(draw.vertices[0].tex_coord, [0.25, 0.0]);
        assert_eq!(draw.vertices[3].tex_coord, [0.75, 1.0]);
        assert_eq!(
            draw.scissor,
            Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            })
        );
    }

    #[test]
    fn wscroll2_validates_sync_and_wraps_far_and_near_panoramas() {
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let resource = |texture_id, uri: &str| LegacyTextureResourceV1 {
            texture_id,
            resource_uri: uri.into(),
            codec: "png".into(),
            revision: u64::from_le_bytes(
                Hash256::from_sha256(uri.as_bytes()).as_bytes()[..8]
                    .try_into()
                    .unwrap(),
            ),
            decoded_width: 3840,
            decoded_height: 720,
            decoded_format: LegacyTextureFormat::Rgba8,
        };
        let draw = |texture_id| LegacyDrawV1 {
            texture_id,
            vertices: [
                vertex(0.0, 0.0, 0.0, 0.0),
                vertex(3840.0, 0.0, 1.0, 0.0),
                vertex(0.0, 720.0, 0.0, 1.0),
                vertex(3840.0, 720.0, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor: None,
        };
        let mut frame = LegacyRenderResourceFrameV1 {
            width: 1280,
            height: 720,
            texture_resources: vec![
                resource(0, "minori:/bg/far.png"),
                resource(1, "minori:/st/near.png"),
            ],
            draws: vec![draw(0), draw(1)],
        };
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/st/walk.txt".into(),
                b"13\r\n16\r\n19\r\n".to_vec(),
            )]),
        });
        let scroll = crate::MinoriWScroll2State {
            sync_resource_uri: "minori:/st/walk.txt".into(),
            period_ticks: 60,
            speed_tenths: -8,
            elapsed_ns: 166_666_667,
            elapsed_ticks: 10,
            foreground_offset: -8,
            background_offset: -1,
            background_remainder: -3,
        };
        apply_wscroll2_to_frame(&vfs, "mount.test", &mut frame, &scroll).unwrap();
        frame.validate().unwrap();
        assert_eq!(frame.draws.len(), 4);
        assert_eq!(frame.draws[0].texture_id, 0);
        assert_eq!(frame.draws[0].vertices[0].position, [0.0, 0.0]);
        assert_eq!(frame.draws[0].vertices[3].position, [1.0, 720.0]);
        assert_eq!(frame.draws[1].vertices[0].position, [1.0, 0.0]);
        assert_eq!(frame.draws[1].vertices[3].position, [1280.0, 720.0]);
        assert_eq!(frame.draws[2].texture_id, 1);

        assert!(parse_wscroll2_sync(b"; comment\r\nnot-a-number\r\n").is_err());
        assert!(parse_wscroll2_sync(b"\r\n").is_err());
    }

    #[test]
    fn provider_emits_bounded_firefly_resources_and_particle_draws() {
        let script =
            b".effect Firefly Firefly_c 3 1000\r\n.wait 20\r\n.effect end\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let png_revision = u64::from_le_bytes(
            Hash256::from_sha256(&png).as_bytes()[..8]
                .try_into()
                .unwrap(),
        );
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/Firefly_cS.png".into(), png.clone()),
                ("minori:/sys/Firefly_cM.png".into(), png.clone()),
                ("minori:/sys/Firefly_cL.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.firefly".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        let started = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let frame = &started.live.resource_scenes[0].value;
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.texture_resources.len(), 3);
        assert!(frame.draws.is_empty());
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.resource_uri.as_str())
                .collect::<Vec<_>>(),
            [
                "minori:/sys/Firefly_cS.png",
                "minori:/sys/Firefly_cM.png",
                "minori:/sys/Firefly_cL.png",
            ]
        );
        assert!(frame.texture_resources.iter().all(|resource| {
            resource.decoded_width == 1
                && resource.decoded_height == 1
                && resource.revision != 0
                && resource.revision != png_revision
        }));
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.revision)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let animated = &waiting
            .live
            .resource_scenes
            .last()
            .expect("expected animated Firefly presentation")
            .value;
        assert_eq!(animated.draws.len(), 3);
        assert!(animated.draws.iter().all(|draw| {
            draw.scissor.as_ref().is_some_and(|scissor| {
                scissor.x == 0 && scissor.y == 0 && scissor.width == 1280 && scissor.height == 720
            }) && draw.blend == LegacyBlendMode::Alpha
        }));
        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider.restore(&ctx, &session, &snapshot).unwrap();
    }

    #[test]
    fn provider_composes_bounded_snow_h_as_the_secondary_slot() {
        let script = b".effect2 SnowH\r\n.wait 20\r\n.effect2 fadeout\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/snowS.png".into(), png.clone()),
                ("minori:/sys/snowM.png".into(), png.clone()),
                ("minori:/sys/snowL.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.snow-h".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        let started = provider
            .step(&ctx, &session, step_input_with_delta(1, 16_000_000))
            .unwrap();
        assert_eq!(started.trace[0].action.as_deref(), Some("secondary_effect"));
        let frame = &started.live.resource_scenes[0].value;
        assert_eq!(frame.texture_resources.len(), 3);
        assert!(frame.draws.is_empty());
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.resource_uri.as_str())
                .collect::<Vec<_>>(),
            [
                "minori:/sys/snowS.png",
                "minori:/sys/snowM.png",
                "minori:/sys/snowL.png",
            ]
        );

        let waiting = provider
            .step(&ctx, &session, step_input_with_delta(2, 16_000_000))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let animated = &waiting
            .live
            .resource_scenes
            .last()
            .expect("expected animated SnowH presentation")
            .value;
        assert_eq!(animated.texture_resources.len(), 3);
        assert_eq!(animated.draws.len(), 50);
        assert!(animated.draws.iter().all(|draw| {
            (600..=602).contains(&draw.texture_id)
                && draw
                    .vertices
                    .iter()
                    .all(|vertex| vertex.color[3] == 1.0 / 256.0)
                && draw.scissor
                    == Some(LegacyScissorV1 {
                        x: 0,
                        y: 0,
                        width: 1280,
                        height: 720,
                    })
        }));

        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider.restore(&ctx, &session, &snapshot).unwrap();
    }

    #[test]
    fn provider_applies_screen_shake_after_compositing_the_stage() {
        let script =
            b".stage * BG.png 0 0\r\n.shakeScreen V 10 30\r\n.wait 20\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.screen-shake".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        provider
            .step(&ctx, &session, step_input_with_delta(1, 16_000_000))
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input_with_delta(2, 16_000_000))
            .unwrap();
        assert_eq!(started.trace[0].action.as_deref(), Some("screen_shake"));
        assert_eq!(
            started.live.resource_scenes[0].value.draws[0].vertices[0].position,
            [0.0, 0.0]
        );
        provider
            .step(&ctx, &session, step_input_with_delta(3, 16_000_000))
            .unwrap();
        let animated = provider
            .step(&ctx, &session, step_input_with_delta(4, 16_000_000))
            .unwrap();
        assert_eq!(animated.status, LegacyRuntimeStatus::Awaiting);
        let frame = &animated.live.resource_scenes.last().unwrap().value;
        assert_eq!(frame.draws[0].vertices[0].position, [0.0, -10.0]);
        assert_eq!(
            frame.draws[0].scissor,
            Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        );
        let snapshot = provider.save(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider.restore(&ctx, &session, &snapshot).unwrap();
    }

    #[test]
    fn provider_records_zero_resource_crossfade2_without_presentation_fallback() {
        let script = b".effect CrossFade2\r\n.wait 20\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.effect".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 20_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let cleared = provider
            .step(&ctx, &session, step_input_with_delta(1, 20_000_000))
            .unwrap();
        assert!(cleared.live.resource_scenes.is_empty());
        assert_eq!(cleared.trace[0].action.as_deref(), Some("effect_clear"));

        let waiting = provider
            .step(&ctx, &session, step_input_with_delta(2, 20_000_000))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(waiting.control.waits.len(), 1);
    }

    #[test]
    fn provider_composes_verified_message_panel_over_the_visible_effect_frame() {
        let script = b".effect CrossFade2\r\n.panel 1\r\n.end\r\n".to_vec();
        let mut panel_png = Vec::new();
        PngEncoder::new(&mut panel_png)
            .write_image(&vec![255; 4 * 263], 1, 263, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/msgPanel.png".into(), panel_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.panel".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 20_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(1, 20_000_000))
            .unwrap();
        let panel = provider
            .step(&ctx, &session, step_input_with_delta(2, 20_000_000))
            .unwrap();
        let frame = &panel.live.resource_scenes[0].value;
        assert_eq!(frame.texture_resources.len(), 1);
        assert_eq!(frame.draws.len(), 1);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/sys/msgPanel.png"
        );
        assert_eq!(frame.texture_resources[0].texture_id, 200);
        assert_eq!(frame.texture_resources[0].decoded_height, 263);
        assert_eq!(frame.draws[0].vertices[0].position[1], 521.0);
        assert_eq!(frame.draws[0].vertices[2].position[1], 784.0);
    }

    fn context() -> LegacyRuntimeHostCtx {
        LegacyRuntimeHostCtx {
            case_id: "case.test".into(),
            package_id: "package.test".into(),
            package_hash: Hash256::from_sha256(b"package"),
            mount_set_id: "mount.test".into(),
            media_service_ids: vec!["media.test".into()],
            permission_policy_id: "policy.test".into(),
            report_sink_id: "report.test".into(),
            target: "headless-test".into(),
            profile: "test".into(),
        }
    }

    fn step_input(tick_index: u64, await_results: Vec<LegacyAwaitResult>) -> LegacyStepInput {
        step_input_with_delta_and_await(tick_index, 16_666_667, await_results)
    }

    fn step_input_with_delta(tick_index: u64, delta_ns: u64) -> LegacyStepInput {
        step_input_with_delta_and_await(tick_index, delta_ns, Vec::new())
    }

    fn step_input_with_delta_and_await(
        tick_index: u64,
        delta_ns: u64,
        await_results: Vec<LegacyAwaitResult>,
    ) -> LegacyStepInput {
        LegacyStepInput {
            tick_index,
            delta_ns,
            session_seed: 7,
            mode: LegacyReplayMode::Live,
            input_edges: Vec::new(),
            await_results,
            provider_results: Vec::new(),
            budget: LegacyStepBudget {
                max_instructions: 64,
                max_effects: 64,
                max_trace_entries: 64,
            },
        }
    }
}
