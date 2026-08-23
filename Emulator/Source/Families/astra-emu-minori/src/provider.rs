use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use astra_byte_source::OwnedByteBuffer;
use astra_core::Hash256;
use astra_emu_extension_api::{
    TranslationTextRequestV1, TranslationTextResponseV1, TRANSLATION_TEXT_HOOK_ID,
};
use astra_emu_family_api::{
    validate_symbol, FamilyId, LegacyAudioCommandV1, LegacyAudioEncoding, LegacyBlendMode,
    LegacyControlTransaction, LegacyCoverageDelta, LegacyDiagnostic, LegacyDrawV1,
    LegacyFamilyHostServicesV9, LegacyFamilyPluginDescriptor, LegacyHookInvocationV1,
    LegacyHookStatusV1, LegacyLayerBlendV9, LegacyLayerFilterV9, LegacyLayerOperationV9,
    LegacyLayerStateV9, LegacyLayerTransactionV9, LegacyLayerTransformV9, LegacyLiveOutput,
    LegacyOpenRequest, LegacyProbeReport, LegacyProbeRequest, LegacyProviderError,
    LegacyRenderResourceFrameV1, LegacyRuntimeHostCtx, LegacyRuntimeProvider,
    LegacyRuntimeSessionId, LegacyRuntimeStatus, LegacySequenced, LegacyShutdownReport,
    LegacyStepInput, LegacyStepOutput, LegacySurfaceCommitV9, LegacySurfaceDamageV9,
    LegacySurfaceFormatV9, LegacyTextureFormat, LegacyTextureResourceV1, LegacyTraceEntry,
    LegacyVertexV1, LegacyVfsReader, LegacyWaitRequest, LegacyWritableFileHostV1,
    LegacyWritableFileRequestV1, LEGACY_FAMILY_ABI_FINGERPRINT,
};

use crate::{
    parse_sc, MinoriAudioCommand, MinoriEffectFrame, MinoriRuntimeError, MinoriRuntimeState,
    MinoriStageCommand, MinoriStageLayer, MinoriVm, MinoriVmEvent, MinoriWaitState,
    ScOpcodeCatalog,
};

pub const MINORI_FAMILY_ID: &str = "minori";
pub const MINORI_RUNTIME_PROVIDER_ID: &str = "astra.emu.family.minori";
const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_NATIVE_SAVE_BYTES: u64 = 16 * 1024 * 1024;
const NATIVE_SAVE_PATH: &str = "save/minori-slot-000.asav";
fn message_input_keys() -> Vec<String> {
    ["enter", "space", "pointer.primary"]
        .iter()
        .map(|key| key.to_string())
        .collect()
}

struct MinoriSession {
    mount_set_id: String,
    fixed_delta_ns: u64,
    session_seed: u64,
    stage_size: Option<(u32, u32)>,
    vm: MinoriVm,
    surface_generations: BTreeMap<String, u64>,
    active_layers: BTreeMap<String, LegacyLayerStateV9>,
    layer_sources: BTreeMap<String, String>,
    next_layer_sequence: u64,
    text_renderer: crate::text_renderer::MinoriTextRenderer,
    last_text: Option<(String, Option<String>)>,
    hook_timeout_ms: u32,
    poisoned: bool,
}

pub struct MinoriRuntimeProvider {
    services: LegacyFamilyHostServicesV9,
    sessions: BTreeMap<String, MinoriSession>,
}

impl MinoriRuntimeProvider {
    pub fn new(services: LegacyFamilyHostServicesV9) -> Self {
        Self {
            services,
            sessions: BTreeMap::new(),
        }
    }

    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    fn vfs(&self) -> &Arc<dyn LegacyVfsReader> {
        &self.services.vfs
    }
}

pub fn create_static_minori_provider(
    services: LegacyFamilyHostServicesV9,
) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError> {
    let provider = MinoriRuntimeProvider::new(services);
    provider.descriptor().validate()?;
    Ok(Box::new(provider))
}

pub fn minori_descriptor() -> LegacyFamilyPluginDescriptor {
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
            "surface.write".into(),
            "hook.invoke".into(),
            "writable_file".into(),
            "media.submit".into(),
        ],
        report_redaction: "astra.emu.redaction.v1".into(),
        license: "MPL-2.0".into(),
    }
}

impl LegacyRuntimeProvider for MinoriRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        minori_descriptor()
    }

    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError> {
        ctx.validate()?;
        let candidates = request
            .candidate_uris
            .iter()
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
        let bytes = self
            .vfs()
            .read_file(&request.root_mount_id, candidate, MAX_SCRIPT_BYTES)?;
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
            self.vfs()
                .read_file(&ctx.mount_set_id, &request.script_uri, MAX_SCRIPT_BYTES)?;
        let script_hash = Hash256::from_sha256(&bytes);
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        let vm = MinoriVm::new(
            request.script_uri,
            script_hash,
            script,
            request.session_seed,
        )
        .map_err(runtime_error)?;
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
        let id = request.requested_session_id;
        self.sessions.insert(
            id.0.clone(),
            MinoriSession {
                mount_set_id: ctx.mount_set_id.clone(),
                fixed_delta_ns: request.fixed_delta_ns,
                session_seed: request.session_seed,
                stage_size,
                vm,
                surface_generations: BTreeMap::new(),
                active_layers: BTreeMap::new(),
                layer_sources: BTreeMap::new(),
                next_layer_sequence: 1,
                text_renderer: crate::text_renderer::MinoriTextRenderer::new()
                    .map_err(|code| invalid("ASTRA_EMU_MINORI_TEXT_RENDERER", code))?,
                last_text: None,
                hook_timeout_ms: parse_hook_timeout(&request.family_options)?,
                poisoned: false,
            },
        );
        Ok(id)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        ctx.validate()?;
        input.validate()?;
        let vfs = Arc::clone(self.vfs());
        let services = self.services.clone();
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
        if !input.provider_results.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STEP_CHANNEL",
                "provider result semantics are not supported",
            ));
        }
        let native_save_action = native_save_action(&input.input_edges)?;
        if matches!(native_save_action, Some(NativeSaveAction::Load)) {
            if !input.await_results.is_empty() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_NATIVE_LOAD_AWAIT",
                    "native load cannot consume a completion from the discarded state",
                ));
            }
            let bytes = read_native_save(
                services.writable_files.as_ref(),
                &session_id.0,
                NATIVE_SAVE_PATH,
            )?;
            session
                .vm
                .restore_native_save(&bytes, input.tick_index)
                .map_err(runtime_error)?;
        }
        let animated_effect = session
            .vm
            .advance_effect_clock(input.delta_ns)
            .map_err(runtime_error)?;
        if let Some(wait) = session.vm.state().wait.clone() {
            if input.await_results.is_empty() {
                session
                    .vm
                    .advance_waiting_tick(input.tick_index)
                    .map_err(runtime_error)?;
                let effect = animated_effect
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
                    .transpose()?;
                let layers = effect
                    .map(|frame| {
                        publish_resource_frame(
                            &services,
                            session,
                            &session_id.0,
                            input.tick_index,
                            frame.value,
                        )
                    })
                    .transpose()?
                    .into_iter()
                    .collect();
                if matches!(native_save_action, Some(NativeSaveAction::Save)) {
                    write_native_save(
                        services.writable_files.as_ref(),
                        &session_id.0,
                        NATIVE_SAVE_PATH,
                        &session.vm.encode_native_save().map_err(runtime_error)?,
                    )?;
                }
                return waiting_output(&session.vm, wait, layers);
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
        } else if !input.await_results.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_AWAIT_UNEXPECTED",
                "step supplied an await result without an active wait",
            ));
        }
        let before = session.vm.state().instruction_count;
        let event = match session.vm.step(input.tick_index) {
            Ok(event) => event,
            Err(error) => {
                session.poisoned = true;
                return Err(runtime_error(error));
            }
        };
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
        let mut live = LegacyLiveOutput::default();
        let mut diagnostics = Vec::new();
        if let Some(frame) = &animated_effect {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::Stage(_)
                        | MinoriVmEvent::Effect(_)
                        | MinoriVmEvent::Panel { .. }
                )
            ) {
                let frame = effect_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    frame,
                )?;
                live.layers.push(publish_resource_frame(
                    &services,
                    session,
                    &session_id.0,
                    input.tick_index,
                    frame.value,
                )?);
            }
        }
        if let Some(MinoriVmEvent::Message {
            presentation_sequence: _,
            capture_sequence,
            text,
            speaker,
            wait: _,
        }) = &event
        {
            if text.len() > MAX_TEXT_BYTES
                || speaker
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_TEXT_BYTES)
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                    "message or speaker exceeds the text render bound",
                ));
            }
            let hook_context = TextHookContext {
                session_id: &session_id.0,
                family_game_id: &ctx.case_id,
                fixed_step: input.tick_index,
                capture_sequence: *capture_sequence,
            };
            live.layers.push(publish_text_layer(
                &services,
                session,
                &hook_context,
                text,
                speaker.as_deref(),
                &mut diagnostics,
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
            let frame = match describe_stage_frame(&vfs, &session.mount_set_id, stage, stage_size) {
                Ok(frame) => frame,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            let _ = sequence;
            live.layers.push(publish_resource_frame(
                &services,
                session,
                &session_id.0,
                input.tick_index,
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
            live.layers.push(publish_resource_frame(
                &services,
                session,
                &session_id.0,
                input.tick_index,
                effect.value,
            )?);
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
            live.layers.push(publish_resource_frame(
                &services,
                session,
                &session_id.0,
                input.tick_index,
                panel.value,
            )?);
        }
        let mut audio_command_count = 0u64;
        if let Some(MinoriVmEvent::Audio { commands }) = &event {
            for command in commands {
                let (sequence, command) = map_audio_command(command);
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
            _ => Vec::new(),
        };
        let status = match &event {
            Some(MinoriVmEvent::Wait(_)) | Some(MinoriVmEvent::Message { .. }) => {
                LegacyRuntimeStatus::Awaiting
            }
            Some(MinoriVmEvent::Chain { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Audio { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Stage(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Effect(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Panel { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Terminal) => LegacyRuntimeStatus::Terminal,
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
                    Some(MinoriVmEvent::Effect(_)) => Some("effect".into()),
                    Some(MinoriVmEvent::Panel { .. }) => Some("panel".into()),
                    _ => None,
                },
                yield_reason: waits.first().map(|_| "wait".into()),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let output = LegacyStepOutput {
            status,
            live,
            control: LegacyControlTransaction {
                waits,
                ..LegacyControlTransaction::default()
            },
            trace,
            diagnostics,
            coverage: LegacyCoverageDelta {
                instructions: after - before,
                contexts: vec![0],
                audio_commands: audio_command_count,
                ..LegacyCoverageDelta::default()
            },
            state_revision: session.vm.state().fixed_tick,
        };
        if matches!(native_save_action, Some(NativeSaveAction::Save)) {
            write_native_save(
                services.writable_files.as_ref(),
                &session_id.0,
                NATIVE_SAVE_PATH,
                &session.vm.encode_native_save().map_err(runtime_error)?,
            )?;
        }
        output.validate()?;
        Ok(output)
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
            evidence_vm_trace: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

fn map_audio_command(command: &MinoriAudioCommand) -> (u64, LegacyAudioCommandV1) {
    match command {
        MinoriAudioCommand::LoadResource {
            sequence,
            stream_id,
            resource_uri,
        } => (
            *sequence,
            LegacyAudioCommandV1::LoadResource {
                stream_id: *stream_id,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: resource_uri.clone(),
            },
        ),
        MinoriAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => (
            *sequence,
            LegacyAudioCommandV1::Play {
                stream_id: *stream_id,
                volume: *volume,
                pan: *pan,
                repeat: *repeat,
                fade_in_ms: *fade_in_ms,
            },
        ),
        MinoriAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        } => (
            *sequence,
            LegacyAudioCommandV1::Stop {
                stream_id: *stream_id,
                fade_ms: *fade_ms,
            },
        ),
        MinoriAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        } => (
            *sequence,
            LegacyAudioCommandV1::SetParams {
                stream_id: *stream_id,
                volume: *volume,
                pan: *pan,
                repeat: *repeat,
            },
        ),
    }
}

fn describe_stage_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage: &MinoriStageCommand,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    if !stage.stands.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
            "stand position semantics are not yet verified",
        ));
    }
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    if let Some(background) = &stage.background {
        append_stage_layer(
            vfs,
            mount_set_id,
            background,
            1,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(foreground) = &stage.foreground {
        append_stage_layer(
            vfs,
            mount_set_id,
            foreground,
            2,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_effect_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    for (layer_id, layer) in &state.layers {
        if *layer_id >= 16 || layer.x_milli % 1000 != 0 || layer.y_milli % 1000 != 0 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
                "effect composition encountered unverified stage positioning",
            ));
        }
        append_resource_layer(
            vfs,
            mount_set_id,
            &layer.resource_uri,
            layer.x_milli / 1000,
            layer.y_milli / 1000,
            f32::from(layer.opacity_milli) / 1000.0,
            layer_id.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_LAYER_ID",
                    "stage layer id overflowed the render resource namespace",
                )
            })?,
            &mut texture_resources,
            &mut draws,
        )?;
    }
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
    if let Some(panel) = &state.panel {
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
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
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
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_EFFECT_ALPHA",
            "effect alpha is outside the normalized bound",
        ));
    }
    let bytes = vfs.read_file(mount_set_id, resource_uri, MAX_RESOURCE_BYTES)?;
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
    texture_resources.push(LegacyTextureResourceV1 {
        texture_id,
        resource_uri: resource_uri.to_owned(),
        codec: codec.into(),
        revision: 1,
        decoded_width: image_width,
        decoded_height: image_height,
        decoded_format: LegacyTextureFormat::Rgba8,
    });
    let left = x as f32;
    let top = y as f32;
    let right = left + image_width as f32;
    let bottom = top + image_height as f32;
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
        texture_filter: astra_emu_family_api::LegacyTextureFilter::Linear,
        scissor: None,
    });
    Ok(())
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
    let bytes = vfs.read_file(mount_set_id, &script_uri, MAX_SCRIPT_BYTES)?;
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

fn publish_resource_frame(
    services: &LegacyFamilyHostServicesV9,
    session: &mut MinoriSession,
    session_id: &str,
    fixed_step: u64,
    frame: LegacyRenderResourceFrameV1,
) -> Result<LegacyLayerTransactionV9, LegacyProviderError> {
    frame.validate()?;
    let resources = frame
        .texture_resources
        .iter()
        .map(|resource| (resource.texture_id, resource))
        .collect::<BTreeMap<_, _>>();
    let mut operations = Vec::new();
    let mut next_visual_layers = BTreeMap::new();
    for draw in &frame.draws {
        let resource = resources.get(&draw.texture_id).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_RESOURCE",
                "draw references a texture resource that is not present",
            )
        })?;
        let layer_id = format!("minori.visual.{:08}", draw.texture_id);
        let surface_id = format!("minori.surface.{:08}", draw.texture_id);
        let source_changed = session
            .layer_sources
            .get(&layer_id)
            .is_none_or(|uri| uri != &resource.resource_uri);
        let (generation, stride, damage) = if source_changed {
            let bytes = services.vfs.read_file(
                &session.mount_set_id,
                &resource.resource_uri,
                MAX_RESOURCE_BYTES,
            )?;
            let decoded = image::load_from_memory(bytes.as_slice())
                .map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_LAYER_DECODE",
                        "Minori image resource could not be decoded",
                    )
                })?
                .into_rgba8();
            if decoded.width() != resource.decoded_width
                || decoded.height() != resource.decoded_height
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_LAYER_DIMENSIONS",
                    "decoded image dimensions changed after metadata validation",
                ));
            }
            let generation = upload_rgba_surface(
                services,
                session,
                SurfaceUpload {
                    session_id,
                    fixed_step,
                    surface_id: &surface_id,
                    width: decoded.width(),
                    height: decoded.height(),
                    rgba: decoded.as_raw(),
                },
            )?;
            session
                .layer_sources
                .insert(layer_id.clone(), resource.resource_uri.clone());
            (
                generation,
                decoded.width().checked_mul(4).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_SURFACE_STRIDE",
                        "surface stride overflowed",
                    )
                })?,
                LegacySurfaceDamageV9::Full,
            )
        } else {
            let previous = session.active_layers.get(&layer_id).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_LAYER_STATE",
                    "retained layer source exists without retained layer state",
                )
            })?;
            (
                previous.generation,
                previous.stride,
                LegacySurfaceDamageV9::Unchanged,
            )
        };
        let left = draw.vertices[0].position[0];
        let top = draw.vertices[0].position[1];
        let opacity = draw.vertices[0].color[3];
        let layer = LegacyLayerStateV9 {
            layer_id: layer_id.clone(),
            role: minori_layer_role(draw.texture_id).into(),
            z_index: i32::try_from(draw.texture_id).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_LAYER_Z",
                    "texture id does not fit layer z order",
                )
            })?,
            surface_id,
            generation,
            width: resource.decoded_width,
            height: resource.decoded_height,
            stride,
            format: LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            damage,
            transform: LegacyLayerTransformV9 {
                m11: 1.0,
                m12: 0.0,
                m21: 0.0,
                m22: 1.0,
                tx: left,
                ty: top,
            },
            clip: None,
            opacity,
            texture_filter: LegacyLayerFilterV9::Linear,
            blend: if draw.texture_id == 1 {
                LegacyLayerBlendV9::Opaque
            } else {
                LegacyLayerBlendV9::Alpha
            },
            filter_graph: None,
        };
        operations.push(if session.active_layers.contains_key(&layer_id) {
            LegacyLayerOperationV9::Update(layer.clone())
        } else {
            LegacyLayerOperationV9::Create(layer.clone())
        });
        next_visual_layers.insert(layer_id, layer);
    }
    let removed = session
        .active_layers
        .keys()
        .filter(|layer_id| layer_id.starts_with("minori.visual."))
        .filter(|layer_id| !next_visual_layers.contains_key(*layer_id))
        .cloned()
        .collect::<Vec<_>>();
    for layer_id in removed {
        operations.push(LegacyLayerOperationV9::Destroy {
            layer_id: layer_id.clone(),
        });
        session.active_layers.remove(&layer_id);
        session.layer_sources.remove(&layer_id);
    }
    session.active_layers.extend(next_visual_layers);
    layer_transaction(session, frame.width, frame.height, operations)
}

struct TextHookContext<'a> {
    session_id: &'a str,
    family_game_id: &'a str,
    fixed_step: u64,
    capture_sequence: u64,
}

fn publish_text_layer(
    services: &LegacyFamilyHostServicesV9,
    session: &mut MinoriSession,
    hook_context: &TextHookContext<'_>,
    original_text: &str,
    speaker: Option<&str>,
    diagnostics: &mut Vec<LegacyDiagnostic>,
) -> Result<LegacyLayerTransactionV9, LegacyProviderError> {
    let (width, height) = session.stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY",
            "Minori text rendering requires explicit stage dimensions",
        )
    })?;
    let translated = translate_text(
        services,
        hook_context,
        original_text,
        session.hook_timeout_ms,
        diagnostics,
    );
    let text_identity = (translated.clone(), speaker.map(str::to_owned));
    let surface_id = "minori.surface.text";
    let (generation, damage) = if session.last_text.as_ref() == Some(&text_identity) {
        (
            *session.surface_generations.get(surface_id).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_TEXT_STATE",
                    "text identity exists without a committed surface generation",
                )
            })?,
            LegacySurfaceDamageV9::Unchanged,
        )
    } else {
        let pixels = match session
            .text_renderer
            .render(width, height, &translated, speaker)
        {
            Ok(pixels) => pixels,
            Err(code) if translated != original_text => {
                diagnostics.push(LegacyDiagnostic {
                    code: code.clone(),
                    severity: "warn".into(),
                    subject: "minori.translation".into(),
                    message: "translated text layout failed; Minori retained the original text"
                        .into(),
                });
                session
                    .text_renderer
                    .render(width, height, original_text, speaker)
                    .map_err(|fallback| invalid("ASTRA_EMU_MINORI_TEXT_LAYOUT", fallback))?
            }
            Err(code) => return Err(invalid("ASTRA_EMU_MINORI_TEXT_LAYOUT", code)),
        };
        let generation = upload_rgba_surface(
            services,
            session,
            SurfaceUpload {
                session_id: hook_context.session_id,
                fixed_step: hook_context.fixed_step,
                surface_id,
                width,
                height,
                rgba: &pixels,
            },
        )?;
        session.last_text = Some(text_identity);
        (generation, LegacySurfaceDamageV9::Full)
    };
    let layer_id = "minori.text".to_owned();
    let layer = LegacyLayerStateV9 {
        layer_id: layer_id.clone(),
        role: "text".into(),
        z_index: 1_000,
        surface_id: surface_id.into(),
        generation,
        width,
        height,
        stride: width.checked_mul(4).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_STRIDE",
                "text surface stride overflowed",
            )
        })?,
        format: LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
        damage,
        transform: LegacyLayerTransformV9 {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            tx: 0.0,
            ty: 0.0,
        },
        clip: None,
        opacity: 1.0,
        texture_filter: LegacyLayerFilterV9::Linear,
        blend: LegacyLayerBlendV9::Alpha,
        filter_graph: None,
    };
    let operation = if session.active_layers.contains_key(&layer_id) {
        LegacyLayerOperationV9::Update(layer.clone())
    } else {
        LegacyLayerOperationV9::Create(layer.clone())
    };
    session.active_layers.insert(layer_id, layer);
    layer_transaction(session, width, height, vec![operation])
}

fn translate_text(
    services: &LegacyFamilyHostServicesV9,
    context: &TextHookContext<'_>,
    original: &str,
    timeout_ms: u32,
    diagnostics: &mut Vec<LegacyDiagnostic>,
) -> String {
    let payload = match postcard::to_allocvec(&TranslationTextRequestV1 {
        utf8: original.as_bytes().to_vec(),
    }) {
        Ok(payload) => payload,
        Err(_) => return original.to_owned(),
    };
    let result = services.hooks.invoke(LegacyHookInvocationV1 {
        session_id: context.session_id.into(),
        invocation_id: format!(
            "minori.translation.{}.{}",
            context.fixed_step, context.capture_sequence
        ),
        family_id: MINORI_FAMILY_ID.into(),
        family_game_id: context.family_game_id.into(),
        hook_id: TRANSLATION_TEXT_HOOK_ID.into(),
        timeout_ms,
        payload: OwnedByteBuffer::from_vec(payload),
    });
    match result {
        Ok(result) => {
            diagnostics.extend(result.diagnostics);
            if result.status != LegacyHookStatusV1::Completed {
                return original.to_owned();
            }
            match postcard::from_bytes::<TranslationTextResponseV1>(result.payload.as_slice())
                .ok()
                .and_then(|response| response.validate().ok().map(str::to_owned))
            {
                Some(text) => text,
                None => {
                    diagnostics.push(LegacyDiagnostic {
                        code: "ASTRA_EMU_MINORI_HOOK_PROTOCOL".into(),
                        severity: "warn".into(),
                        subject: "minori.translation".into(),
                        message:
                            "translation response was invalid; Minori retained the original text"
                                .into(),
                    });
                    original.to_owned()
                }
            }
        }
        Err(error) => {
            diagnostics.push(LegacyDiagnostic {
                code: error.code().to_owned(),
                severity: "warn".into(),
                subject: "minori.translation".into(),
                message: "translation Hook failed; Minori retained the original text".into(),
            });
            original.to_owned()
        }
    }
}

struct SurfaceUpload<'a> {
    session_id: &'a str,
    fixed_step: u64,
    surface_id: &'a str,
    width: u32,
    height: u32,
    rgba: &'a [u8],
}

fn upload_rgba_surface(
    services: &LegacyFamilyHostServicesV9,
    session: &mut MinoriSession,
    upload: SurfaceUpload<'_>,
) -> Result<u64, LegacyProviderError> {
    let SurfaceUpload {
        session_id,
        fixed_step,
        surface_id,
        width,
        height,
        rgba,
    } = upload;
    let row_bytes = width.checked_mul(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SURFACE_STRIDE",
            "surface row size overflowed",
        )
    })?;
    let expected_len = usize::try_from(row_bytes)
        .ok()
        .and_then(|row| {
            usize::try_from(height)
                .ok()
                .and_then(|height| row.checked_mul(height))
        })
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SURFACE_SIZE",
                "surface byte size overflowed",
            )
        })?;
    if rgba.len() != expected_len {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SURFACE_SIZE",
            "decoded surface byte count does not match its dimensions",
        ));
    }
    let generation = session
        .surface_generations
        .get(surface_id)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SURFACE_GENERATION",
                "surface generation overflowed",
            )
        })?;
    let mut lease = services.surfaces.acquire(
        session_id,
        fixed_step,
        surface_id,
        width,
        height,
        LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
    )?;
    lease.validate()?;
    if lease.surface_id != surface_id
        || lease.generation != generation
        || lease.width != width
        || lease.height != height
        || lease.format != LegacySurfaceFormatV9::Rgba8SrgbPremultiplied
        || lease.stride < row_bytes
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SURFACE_LEASE",
            "Host returned a surface lease with mismatched identity or geometry",
        ));
    }
    let stride = usize::try_from(lease.stride).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SURFACE_STRIDE",
            "surface stride does not fit memory",
        )
    })?;
    let row_bytes = usize::try_from(row_bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SURFACE_STRIDE",
            "surface row size does not fit memory",
        )
    })?;
    copy_premultiplied_rows(rgba, row_bytes, lease.pixels.as_mut_slice(), stride)?;
    services.surfaces.commit(
        session_id,
        fixed_step,
        LegacySurfaceCommitV9 {
            lease,
            damage: LegacySurfaceDamageV9::Full,
        },
    )?;
    session
        .surface_generations
        .insert(surface_id.to_owned(), generation);
    Ok(generation)
}

fn copy_premultiplied_rows(
    source: &[u8],
    row_bytes: usize,
    target: &mut [u8],
    stride: usize,
) -> Result<(), LegacyProviderError> {
    if row_bytes == 0
        || !row_bytes.is_multiple_of(4)
        || stride < row_bytes
        || !source.len().is_multiple_of(row_bytes)
        || target.len() != source.len() / row_bytes * stride
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SURFACE_COPY",
            "surface row layout is inconsistent",
        ));
    }
    for (source, target) in source
        .chunks_exact(row_bytes)
        .zip(target.chunks_exact_mut(stride))
    {
        for (source_pixel, target_pixel) in source
            .as_chunks::<4>()
            .0
            .iter()
            .zip(target[..row_bytes].as_chunks_mut::<4>().0.iter_mut())
        {
            let alpha = u16::from(source_pixel[3]);
            target_pixel[0] = ((u16::from(source_pixel[0]) * alpha + 127) / 255) as u8;
            target_pixel[1] = ((u16::from(source_pixel[1]) * alpha + 127) / 255) as u8;
            target_pixel[2] = ((u16::from(source_pixel[2]) * alpha + 127) / 255) as u8;
            target_pixel[3] = source_pixel[3];
        }
    }
    Ok(())
}

fn layer_transaction(
    session: &mut MinoriSession,
    width: u32,
    height: u32,
    operations: Vec<LegacyLayerOperationV9>,
) -> Result<LegacyLayerTransactionV9, LegacyProviderError> {
    let transaction = LegacyLayerTransactionV9 {
        sequence: session.next_layer_sequence,
        viewport_width: width,
        viewport_height: height,
        operations,
    };
    transaction.validate()?;
    session.next_layer_sequence = session.next_layer_sequence.checked_add(1).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_LAYER_SEQUENCE",
            "layer transaction sequence overflowed",
        )
    })?;
    Ok(transaction)
}

fn minori_layer_role(texture_id: u32) -> &'static str {
    match texture_id {
        1 => "background",
        2 => "foreground",
        100 | 101 => "effect",
        200 => "panel",
        _ => "stand",
    }
}

fn parse_hook_timeout(options: &BTreeMap<String, String>) -> Result<u32, LegacyProviderError> {
    options
        .get("astra.translation.timeout_ms")
        .map(|value| {
            value.parse::<u32>().map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_HOOK_TIMEOUT",
                    "translation timeout must be represented as u32 milliseconds",
                )
            })
        })
        .transpose()
        .map(|value| value.unwrap_or(2_000))
}

fn waiting_output(
    vm: &MinoriVm,
    _wait: MinoriWaitState,
    layers: Vec<LegacyLayerTransactionV9>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let output = LegacyStepOutput {
        status: LegacyRuntimeStatus::Awaiting,
        live: LegacyLiveOutput {
            layers,
            ..LegacyLiveOutput::default()
        },
        // A wait request is edge-triggered: it is published only by the
        // command that creates the token. Re-emitting the same pending token
        // on later ticks would violate RuntimeWorld AwaitQueue uniqueness.
        control: LegacyControlTransaction::default(),
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta::default(),
        state_revision: vm.state().fixed_tick,
    };
    output.validate()?;
    Ok(output)
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
        MinoriWaitState::Input { token_id } => LegacyWaitRequest::Input {
            token_id: token_id.clone(),
            keys: message_input_keys(),
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
        },
    }
}

fn wait_token(wait: &MinoriWaitState) -> &str {
    match wait {
        MinoriWaitState::Time { token_id, .. }
        | MinoriWaitState::Input { token_id }
        | MinoriWaitState::Media { token_id, .. }
        | MinoriWaitState::Presentation { token_id, .. }
        | MinoriWaitState::Provider { token_id, .. } => token_id,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeSaveAction {
    Save,
    Load,
}

fn native_save_action(
    input_edges: &[astra_emu_family_api::LegacyInputEdge],
) -> Result<Option<NativeSaveAction>, LegacyProviderError> {
    let mut action = None;
    for edge in input_edges.iter().filter(|edge| edge.pressed) {
        let candidate = match edge.control.as_str() {
            "function:5" => Some(NativeSaveAction::Save),
            "function:9" => Some(NativeSaveAction::Load),
            _ => None,
        };
        if let Some(candidate) = candidate {
            if action.replace(candidate).is_some() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_NATIVE_SAVE_CONFLICT",
                    "a step may request exactly one native save or load action",
                ));
            }
        }
    }
    Ok(action)
}

fn read_native_save(
    files: &dyn LegacyWritableFileHostV1,
    session_id: &str,
    path: &str,
) -> Result<Vec<u8>, LegacyProviderError> {
    let stat = files.execute(
        session_id,
        LegacyWritableFileRequestV1::Stat { path: path.into() },
    )?;
    if !stat.exists || !stat.is_file || stat.length == 0 || stat.length > MAX_NATIVE_SAVE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_NATIVE_SAVE_STAT",
            "native save is missing, empty, not a file, or exceeds the size bound",
        ));
    }
    let read = files.execute(
        session_id,
        LegacyWritableFileRequestV1::ReadRange {
            path: path.into(),
            offset: 0,
            length: stat.length,
        },
    )?;
    if read.bytes.len() as u64 != stat.length {
        return Err(invalid(
            "ASTRA_EMU_MINORI_NATIVE_SAVE_READ",
            "native save read returned a truncated payload",
        ));
    }
    Ok(read.bytes.as_slice().to_vec())
}

fn write_native_save(
    files: &dyn LegacyWritableFileHostV1,
    session_id: &str,
    path: &str,
    bytes: &[u8],
) -> Result<(), LegacyProviderError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_NATIVE_SAVE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_NATIVE_SAVE_SIZE",
            "native save payload is empty or exceeds the size bound",
        ));
    }
    files.execute(
        session_id,
        LegacyWritableFileRequestV1::CreateDir {
            path: "save".into(),
        },
    )?;
    let temporary_path = format!("save/.minori-slot-000-{session_id}.tmp");
    let write = files.execute(
        session_id,
        LegacyWritableFileRequestV1::WriteRange {
            path: temporary_path.clone(),
            offset: 0,
            bytes: bytes.to_vec(),
        },
    )?;
    if write.written != bytes.len() as u64 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_NATIVE_SAVE_WRITE",
            "native save host reported a short write",
        ));
    }
    files.execute(
        session_id,
        LegacyWritableFileRequestV1::SetLength {
            path: temporary_path.clone(),
            length: bytes.len() as u64,
        },
    )?;
    files.execute(
        session_id,
        LegacyWritableFileRequestV1::AtomicReplace {
            temporary_path,
            destination_path: path.into(),
        },
    )?;
    Ok(())
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
    let code = match &error {
        MinoriRuntimeError::State => "ASTRA_EMU_MINORI_RUNTIME_STATE",
        MinoriRuntimeError::ProgramCounter => "ASTRA_EMU_MINORI_RUNTIME_PC",
        MinoriRuntimeError::Label => "ASTRA_EMU_MINORI_RUNTIME_LABEL",
        MinoriRuntimeError::Operand => "ASTRA_EMU_MINORI_RUNTIME_OPERAND",
        MinoriRuntimeError::UnsupportedOpcode { .. } => "ASTRA_EMU_MINORI_RUNTIME_OPCODE",
        MinoriRuntimeError::NonYieldingCycle => "ASTRA_EMU_MINORI_RUNTIME_NON_YIELDING_CYCLE",
        MinoriRuntimeError::Waiting => "ASTRA_EMU_MINORI_RUNTIME_WAIT",
        MinoriRuntimeError::Overflow => "ASTRA_EMU_MINORI_RUNTIME_OVERFLOW",
        MinoriRuntimeError::NativeSaveFormat => "ASTRA_EMU_MINORI_NATIVE_SAVE_FORMAT",
        MinoriRuntimeError::ChainTarget => "ASTRA_EMU_MINORI_RUNTIME_CHAIN",
        MinoriRuntimeError::AudioResource => "ASTRA_EMU_MINORI_RUNTIME_AUDIO_RESOURCE",
        MinoriRuntimeError::Effect => "ASTRA_EMU_MINORI_RUNTIME_EFFECT",
        MinoriRuntimeError::Panel => "ASTRA_EMU_MINORI_RUNTIME_PANEL",
    };
    LegacyProviderError::invalid(code, error.to_string())
}

fn session_missing() -> LegacyProviderError {
    invalid("ASTRA_EMU_MINORI_SESSION_MISSING", "session is not active")
}

fn invalid(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::LegacyWritableFileResultV1;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingWritableFiles {
        requests: Mutex<Vec<LegacyWritableFileRequestV1>>,
        bytes: Mutex<Vec<u8>>,
    }

    impl LegacyWritableFileHostV1 for RecordingWritableFiles {
        fn execute(
            &self,
            _session_id: &str,
            request: LegacyWritableFileRequestV1,
        ) -> Result<LegacyWritableFileResultV1, LegacyProviderError> {
            self.requests.lock().unwrap().push(request.clone());
            let mut result = LegacyWritableFileResultV1 {
                exists: false,
                is_file: false,
                length: 0,
                entries: Vec::new(),
                bytes: OwnedByteBuffer::from_vec(Vec::new()),
                written: 0,
            };
            match request {
                LegacyWritableFileRequestV1::Stat { .. } => {
                    let bytes = self.bytes.lock().unwrap();
                    result.exists = !bytes.is_empty();
                    result.is_file = result.exists;
                    result.length = bytes.len() as u64;
                }
                LegacyWritableFileRequestV1::ReadRange { offset, length, .. } => {
                    let bytes = self.bytes.lock().unwrap();
                    let start = usize::try_from(offset).unwrap();
                    let end = start.checked_add(usize::try_from(length).unwrap()).unwrap();
                    result.bytes = OwnedByteBuffer::from_vec(bytes[start..end].to_vec());
                }
                LegacyWritableFileRequestV1::WriteRange { offset, bytes, .. } => {
                    assert_eq!(offset, 0);
                    result.written = bytes.len() as u64;
                    *self.bytes.lock().unwrap() = bytes;
                }
                LegacyWritableFileRequestV1::SetLength { length, .. } => {
                    self.bytes
                        .lock()
                        .unwrap()
                        .truncate(usize::try_from(length).unwrap());
                }
                LegacyWritableFileRequestV1::CreateDir { .. }
                | LegacyWritableFileRequestV1::AtomicReplace { .. }
                | LegacyWritableFileRequestV1::List { .. }
                | LegacyWritableFileRequestV1::Remove { .. } => {}
            }
            Ok(result)
        }
    }

    #[test]
    fn descriptor_requires_native_multilayer_v9() {
        let descriptor = minori_descriptor();
        descriptor.validate().unwrap();
        assert_eq!(
            descriptor.core_kind,
            astra_emu_family_api::LegacyFamilyCoreKind::Native
        );
        assert_eq!(
            descriptor.presentation_mode,
            astra_emu_family_api::LegacyFamilyPresentationMode::MultiLayer
        );
        assert_eq!(descriptor.abi_fingerprint, LEGACY_FAMILY_ABI_FINGERPRINT);
    }

    #[test]
    fn rgba_copy_is_premultiplied_and_preserves_stride_padding() {
        let source = [200, 100, 50, 128, 20, 40, 60, 255];
        let mut target = [0_u8; 12];
        copy_premultiplied_rows(&source, 8, &mut target, 12).unwrap();
        assert_eq!(&target[..8], &[100, 50, 25, 128, 20, 40, 60, 255]);
        assert_eq!(&target[8..], &[0; 4]);
    }

    #[test]
    fn rgba_copy_rejects_inconsistent_layout() {
        let error = copy_premultiplied_rows(&[0; 8], 8, &mut [0; 7], 7).unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_MINORI_SURFACE_COPY");
    }

    #[test]
    fn native_save_uses_temporary_file_and_atomic_replace() {
        let files = RecordingWritableFiles::default();
        write_native_save(&files, "session-a", NATIVE_SAVE_PATH, b"native-save").unwrap();
        assert_eq!(
            read_native_save(&files, "session-a", NATIVE_SAVE_PATH).unwrap(),
            b"native-save"
        );
        let requests = files.requests.lock().unwrap();
        assert!(matches!(
            requests[0],
            LegacyWritableFileRequestV1::CreateDir { .. }
        ));
        assert!(matches!(
            requests[1],
            LegacyWritableFileRequestV1::WriteRange { .. }
        ));
        assert!(matches!(
            requests[2],
            LegacyWritableFileRequestV1::SetLength { .. }
        ));
        assert!(matches!(
            requests[3],
            LegacyWritableFileRequestV1::AtomicReplace { .. }
        ));
    }

    #[test]
    fn native_save_shortcuts_are_core_owned_and_conflicts_fail() {
        let edge = |control: &str, sequence| astra_emu_family_api::LegacyInputEdge {
            control: control.into(),
            pressed: true,
            value: 1.0,
            sequence,
        };
        assert_eq!(
            native_save_action(&[edge("function:5", 1)]).unwrap(),
            Some(NativeSaveAction::Save)
        );
        assert_eq!(
            native_save_action(&[edge("function:9", 1)]).unwrap(),
            Some(NativeSaveAction::Load)
        );
        assert_eq!(native_save_action(&[edge("enter", 1)]).unwrap(), None);
        assert_eq!(
            native_save_action(&[edge("function:5", 1), edge("function:9", 2)])
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_NATIVE_SAVE_CONFLICT"
        );
    }
}
