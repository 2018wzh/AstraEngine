use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use astra_core::{Diagnostic, Hash256, SchemaVersion, StableId};
use astra_emu_family_api::{
    LegacyAwaitResult, LegacyEffect, LegacyEphemeralText, LegacyInputEdge, LegacyOpenRequest,
    LegacyProbeReport, LegacyProbeRequest, LegacyProviderResult, LegacyReplayMode,
    LegacyRuntimeHostCtx, LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacySnapshotEnvelope,
    LegacyStepBudget, LegacyStepInput, LegacyStepOutput, LegacyWaitRequest,
};
use astra_plugin::ProductRuntimeProvider;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimeOutputCodec, RuntimeOutputDomain, RuntimeOutputEnvelope,
    RuntimeOutputSchemaDescriptor, RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport,
    RuntimeProbeRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionCodec,
    RuntimeSectionPayload, RuntimeShutdownReport, RuntimeStepInput, RuntimeStepMode,
    RuntimeStepOutput,
};
use astra_runtime::{
    ActionDescriptor, ActionInvocation, ActionTrace, AwaitResult, AwaitTokenId, BlackboardValue,
    DeterministicActionContext, EventPayload, GuardExpr, OrderedTickIngress, PackageHandle,
    PlayerInput, PresentationCommand, RuntimeAction, RuntimeConfig, RuntimeError, RuntimeWorld,
    SaveBlob, SaveRequest, StateDefinition, StateMachineDefinition, TickIngress, TickInput,
    TickRequest, TransitionDefinition,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const RUNTIME_ID: &str = "astra.emu.runtime";
const PROVIDER_ID: &str = "astra.emu.runtime_provider";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmuCaseProfile {
    pub schema: String,
    pub family_id: String,
    pub case_fingerprint: Hash256,
    pub script_uri: String,
    pub fixed_delta_ns: u64,
    pub compatibility_profile: String,
    pub mount_set_id: String,
    pub permission_policy_id: String,
    #[serde(default)]
    pub family_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmuStepPayload {
    pub input_edges: Vec<LegacyInputEdge>,
    pub await_results: Vec<LegacyAwaitResult>,
    pub provider_results: Vec<LegacyProviderResult>,
    pub budget: LegacyStepBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmuRuntimeState {
    family_id: String,
    status: String,
    family_state_hash: Hash256,
    fixed_step: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmuFamilySaveV1 {
    family: LegacySnapshotEnvelope,
    await_tokens: BTreeMap<String, AwaitTokenId>,
    pending_patch_effects: Vec<LegacyEffect>,
}

struct EmuSession {
    world: RuntimeWorld,
    family_session_id: LegacyRuntimeSessionId,
    host_ctx: LegacyRuntimeHostCtx,
    output: Arc<Mutex<Option<LegacyStepOutput>>>,
    await_tokens: Arc<Mutex<BTreeMap<String, AwaitTokenId>>>,
    pending_patch_effects: Vec<LegacyEffect>,
    poisoned: bool,
}

struct ApplyLegacyEffectsAction {
    output: Arc<Mutex<Option<LegacyStepOutput>>>,
    state_component: astra_runtime::ComponentId,
    await_tokens: Arc<Mutex<BTreeMap<String, AwaitTokenId>>>,
}

impl RuntimeAction for ApplyLegacyEffectsAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor {
            id: "astra.emu.apply_legacy_effects".into(),
            input_schema: "astra.emu.legacy_effect_input.v1".into(),
            output_schema: "astra.emu.legacy_effect_trace.v1".into(),
        }
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        _input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let output_guard = self
            .output
            .lock()
            .map_err(|_| RuntimeError::message("ASTRA_EMU_OUTPUT_LOCK_POISONED"))?;
        let output = output_guard.as_ref().ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_EMU_OUTPUT_MISSING",
                "family provider did not publish a step output",
            ))
        })?;
        let mut state = ctx.read_component::<EmuRuntimeState>(self.state_component)?;
        state.family_state_hash = output.state_hash;
        state.fixed_step = ctx.step();
        state.status = format!("{:?}", output.status).to_ascii_lowercase();
        ctx.replace_component(self.state_component, &state)?;
        for effect in &output.effects {
            match effect {
                LegacyEffect::RuntimeEvent { event, payload, .. } => ctx.emit_event(
                    astra_runtime::EventSource::StateMachine,
                    EventPayload {
                        kind: event.clone(),
                        data: [("payload".into(), BlackboardValue::Bytes(payload.clone()))]
                            .into_iter()
                            .collect(),
                    },
                ),
                LegacyEffect::Presentation {
                    command, payload, ..
                } => ctx.emit_presentation(PresentationCommand::Custom {
                    kind: command.clone(),
                    data: [
                        (
                            "payload_hash".into(),
                            BlackboardValue::String(Hash256::from_sha256(payload).to_string()),
                        ),
                        (
                            "payload_bytes".into(),
                            BlackboardValue::I64(payload.len() as i64),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                }),
                LegacyEffect::Audio {
                    command, payload, ..
                } => ctx.emit_serialized_effect("audio", command.clone(), payload)?,
                LegacyEffect::TextCapture { .. } => {
                    ctx.emit_serialized_effect("text", "astra.emu.text_capture.v1", effect)?
                }
                LegacyEffect::SetBlackboard { key, value, .. } => {
                    ctx.set_blackboard(key.clone(), BlackboardValue::Bytes(value.clone()))
                }
                LegacyEffect::ScheduleEvent {
                    due_tick,
                    event,
                    payload,
                    ..
                } => {
                    ctx.schedule_event(
                        *due_tick,
                        astra_runtime::EventSource::StateMachine,
                        EventPayload {
                            kind: event.clone(),
                            data: [("payload".into(), BlackboardValue::Bytes(payload.clone()))]
                                .into_iter()
                                .collect(),
                        },
                    );
                }
                LegacyEffect::SnapshotDirty { .. } => {
                    ctx.emit_serialized_effect("save", "astra.emu.snapshot_dirty.v1", effect)?
                }
            }
        }
        for wait in &output.waits {
            let family_token_id = wait_token_id(wait);
            let token = ctx.create_await(astra_runtime::AwaitKind::Custom(wait_kind(wait)));
            let mut tokens = self
                .await_tokens
                .lock()
                .map_err(|_| RuntimeError::message("ASTRA_EMU_AWAIT_LOCK_POISONED"))?;
            if tokens.insert(family_token_id, token.token_id).is_some() {
                return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                    "ASTRA_EMU_AWAIT_TOKEN_DUPLICATE",
                    "family provider emitted a duplicate pending wait token",
                )));
            }
            drop(tokens);
            ctx.push_await(token)?;
        }
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: [
                (
                    "family_state_hash".into(),
                    BlackboardValue::String(output.state_hash.to_string()),
                ),
                (
                    "effect_count".into(),
                    BlackboardValue::I64(output.effects.len() as i64),
                ),
            ]
            .into_iter()
            .collect(),
        })
    }
}

pub struct AstraEmuRuntimeProvider {
    instance_id: Option<ProviderInstanceId>,
    family: Box<dyn LegacyRuntimeProvider>,
    sessions: BTreeMap<String, EmuSession>,
}

impl AstraEmuRuntimeProvider {
    pub fn new(family: Box<dyn LegacyRuntimeProvider>) -> Result<Self, String> {
        family
            .descriptor()
            .validate()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            instance_id: None,
            family,
            sessions: BTreeMap::new(),
        })
    }

    pub fn descriptor_value() -> ProductRuntimeDescriptor {
        ProductRuntimeDescriptor {
            runtime_id: RUNTIME_ID.into(),
            product_kind: "legacy_visual_novel".into(),
            provider_id: PROVIDER_ID.into(),
            supported_targets: vec!["game".into()],
            capabilities: vec!["runtime.astra_emu".into()],
            package_sections: vec!["emu.case_profile".into()],
            release_checks: vec![
                "emu.provider_binding".into(),
                "emu.family_binding".into(),
                "emu.payload_redaction".into(),
            ],
            output_schemas: vec![
                schema(
                    RuntimeOutputDomain::Effect,
                    "astra.emu.legacy_step_output.v1",
                ),
                schema(
                    RuntimeOutputDomain::Presentation,
                    "astra.emu.render_frame.v1",
                ),
                schema(RuntimeOutputDomain::Audio, "astra.emu.audio_effect.v1"),
                schema(RuntimeOutputDomain::Trace, "astra.emu.legacy_trace.v1"),
                schema(
                    RuntimeOutputDomain::Observation,
                    "astra.emu.runtime_observation.v1",
                ),
                schema(
                    RuntimeOutputDomain::DirtySaveSection,
                    "astra.runtime.dirty_save_section.v1",
                ),
            ],
        }
    }

    pub fn take_ephemeral_text(
        &mut self,
        session_id: &GameRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<LegacyEphemeralText>, String> {
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        self.family
            .take_ephemeral_text(&session.host_ctx, &session.family_session_id, lease_id)
            .map_err(|error| error.to_string())
    }

    pub fn read_session_resource(
        &mut self,
        session_id: &GameRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, String> {
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        self.family
            .read_session_resource(
                &session.host_ctx,
                &session.family_session_id,
                resource_uri,
                max_bytes,
            )
            .map_err(|error| error.to_string())
    }

    pub fn probe_family(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, String> {
        if self.instance_id.is_none() {
            return Err("ASTRA_EMU_INSTANCE_MISSING".into());
        }
        if !self.sessions.is_empty() {
            return Err("ASTRA_EMU_PROBE_ACTIVE_SESSION".into());
        }
        ctx.validate().map_err(|error| error.to_string())?;
        let report = self
            .family
            .probe(ctx, request)
            .map_err(|error| error.to_string())?;
        report.validate().map_err(|error| error.to_string())?;
        if report.family_id != self.family.descriptor().family_id {
            return Err("ASTRA_EMU_PROBE_FAMILY_ID_MISMATCH".into());
        }
        Ok(report)
    }

    pub fn queue_patch_effect(
        &mut self,
        session_id: &GameRuntimeSessionId,
        effect: LegacyEffect,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        if session.pending_patch_effects.len() >= 4096 {
            return Err("ASTRA_EMU_PATCH_EFFECT_COUNT".into());
        }
        if !matches!(
            effect,
            LegacyEffect::RuntimeEvent { .. } | LegacyEffect::SetBlackboard { .. }
        ) {
            return Err("ASTRA_EMU_PATCH_EFFECT_KIND".into());
        }
        session.pending_patch_effects.push(effect);
        Ok(())
    }
}

impl ProductRuntimeProvider for AstraEmuRuntimeProvider {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(Self::descriptor_value())
    }

    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if self.instance_id.is_some() {
            return Err("ASTRA_EMU_INSTANCE_DUPLICATE".into());
        }
        self.instance_id = Some(instance_id.clone());
        tracing::info!(
            event = "astra.emu.runtime.instance_created",
            instance_id = %instance_id.0,
            provider_id = PROVIDER_ID
        );
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if !self.sessions.is_empty() {
            return Err("ASTRA_EMU_INSTANCE_ACTIVE_SESSIONS".into());
        }
        if self.instance_id.as_ref() != Some(&instance_id) {
            return Err("ASTRA_EMU_INSTANCE_MISMATCH".into());
        }
        self.instance_id = None;
        tracing::info!(
            event = "astra.emu.runtime.instance_destroyed",
            instance_id = %instance_id.0,
            provider_id = PROVIDER_ID
        );
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&mut self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        let pass = request
            .section_ids
            .iter()
            .any(|id| id == "emu.case_profile");
        Ok(RuntimePrepareReport {
            runtime_id: RUNTIME_ID.into(),
            provider_id: PROVIDER_ID.into(),
            status: if pass { "pass" } else { "blocked" }.into(),
            diagnostics: if pass {
                vec![]
            } else {
                vec!["ASTRA_EMU_CASE_PROFILE_MISSING".into()]
            },
        })
    }

    fn probe(&mut self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        let supported = request.platform.as_deref() != Some("web")
            && request
                .section_ids
                .iter()
                .any(|id| id == "emu.case_profile");
        Ok(RuntimeProbeReport {
            runtime_id: RUNTIME_ID.into(),
            provider_id: PROVIDER_ID.into(),
            status: if supported { "supported" } else { "blocked" }.into(),
            diagnostics: if supported {
                vec![]
            } else {
                vec![if request.platform.as_deref() == Some("web") {
                    "PLATFORM_NOT_IMPLEMENTED:native-family-plugin".into()
                } else {
                    "ASTRA_EMU_CASE_PROFILE_MISSING".into()
                }]
            },
        })
    }

    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        if self.instance_id.is_none() {
            return Err("ASTRA_EMU_INSTANCE_NOT_CREATED".into());
        }
        let section = required_section(
            &request.sections,
            "emu.case_profile",
            "astra.emu.case_profile.v1",
        )?;
        let profile: EmuCaseProfile = postcard::from_bytes(&section.bytes)
            .map_err(|error| format!("ASTRA_EMU_CASE_PROFILE_DECODE:{error}"))?;
        if profile.schema != "astra.emu.case_profile.v1"
            || profile.family_id != self.family.descriptor().family_id.0
        {
            return Err("ASTRA_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        let session_id = GameRuntimeSessionId(format!(
            "{RUNTIME_ID}:{}:{}",
            request.target_id, request.seed
        ));
        if self.sessions.contains_key(&session_id.0) {
            return Err("ASTRA_EMU_SESSION_DUPLICATE".into());
        }
        tracing::info!(
            event = "astra.emu.runtime.session_opening",
            session_id = %session_id.0,
            family_id = %self.family.descriptor().family_id.0,
            provider_id = PROVIDER_ID
        );
        let family_session_id = LegacyRuntimeSessionId(session_id.0.clone());
        let host_ctx = LegacyRuntimeHostCtx {
            case_id: format!("case-{}", &profile.case_fingerprint.to_string()[..16]),
            package_id: request.package_hash.clone(),
            package_hash: parse_package_hash(&request.package_hash)?,
            mount_set_id: profile.mount_set_id.clone(),
            media_service_ids: vec!["astra.media".into()],
            permission_policy_id: profile.permission_policy_id.clone(),
            report_sink_id: "astra.emu.report".into(),
            target: request.target_id.clone(),
            profile: request.profile.clone(),
        };
        self.family
            .open(
                &host_ctx,
                LegacyOpenRequest {
                    requested_session_id: family_session_id.clone(),
                    case_fingerprint: profile.case_fingerprint,
                    script_uri: profile.script_uri,
                    fixed_delta_ns: profile.fixed_delta_ns,
                    session_seed: request.seed,
                    compatibility_profile: profile.compatibility_profile,
                    family_options: profile.family_options,
                },
            )
            .map_err(|error| error.to_string())?;

        let world_setup = (|| -> Result<_, String> {
            let mut world = RuntimeWorld::create(
                RuntimeConfig {
                    seed: request.seed,
                    required_slots: vec![],
                },
                PackageHandle {
                    package_id: request.package_hash.clone(),
                    target: request.target_id.clone(),
                    profile: request.profile.clone(),
                    ..PackageHandle::default()
                },
            )
            .map_err(|error| error.to_string())?;
            let owner = world.create_actor(
                "astra.emu.runtime",
                vec!["gameplay_runtime".into(), "legacy_runtime".into()],
            );
            let state_component = world
                .attach_component(
                    owner,
                    "astra.emu.runtime_state.v1",
                    &EmuRuntimeState {
                        family_id: profile.family_id,
                        status: "active".into(),
                        family_state_hash: Hash256::from_bytes([0; 32]),
                        fixed_step: 0,
                    },
                )
                .map_err(|error| error.to_string())?;
            let output = Arc::new(Mutex::new(None));
            let await_tokens = Arc::new(Mutex::new(BTreeMap::new()));
            world
                .register_action(
                    PROVIDER_ID,
                    ApplyLegacyEffectsAction {
                        output: output.clone(),
                        state_component,
                        await_tokens: await_tokens.clone(),
                    },
                )
                .map_err(|error| error.to_string())?;
            let running = StableId::deterministic_v7(0, 1, request.seed);
            world
                .add_state_machine(StateMachineDefinition {
                    id: StableId::deterministic_v7(0, 2, request.seed),
                    owner,
                    states: vec![StateDefinition {
                        id: running,
                        name: "emu.running".into(),
                        terminal: false,
                    }],
                    transitions: vec![TransitionDefinition {
                        from: running,
                        to: running,
                        guard: GuardExpr::EventIs {
                            kind: "emu.step".into(),
                        },
                        actions: vec![ActionInvocation {
                            action_id: "astra.emu.apply_legacy_effects".into(),
                            input: BTreeMap::new(),
                        }],
                        priority: 0,
                        source_ref: None,
                    }],
                    initial_state: running,
                })
                .map_err(|error| error.to_string())?;
            Ok((world, output, await_tokens))
        })();
        let (world, output, await_tokens) = match world_setup {
            Ok(world) => world,
            Err(setup_error) => {
                let cleanup = self.family.shutdown(&host_ctx, &family_session_id);
                return match cleanup {
                    Ok(_) => Err(setup_error),
                    Err(cleanup_error) => Err(format!(
                        "ASTRA_EMU_OPEN_SETUP_AND_CLEANUP_FAILED:{setup_error};{}",
                        cleanup_error.code()
                    )),
                };
            }
        };
        self.sessions.insert(
            session_id.0.clone(),
            EmuSession {
                world,
                family_session_id,
                host_ctx,
                output,
                await_tokens,
                pending_patch_effects: Vec::new(),
                poisoned: false,
            },
        );
        Ok(RuntimeOpenReport {
            session_id,
            runtime_id: RUNTIME_ID.into(),
            provider_id: PROVIDER_ID.into(),
            diagnostics: vec![],
        })
    }

    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        if input.mode == RuntimeStepMode::Replay {
            return Err("ASTRA_EMU_LIVE_PROVIDER_REPLAY".into());
        }
        if input.action != "emu.step" {
            return Err("ASTRA_EMU_STEP_ACTION".into());
        }
        let payload: EmuStepPayload = serde_json::from_value(input.payload)
            .map_err(|error| format!("ASTRA_EMU_STEP_PAYLOAD:{error}"))?;
        let session = self
            .sessions
            .get_mut(&input.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        let await_results = payload.await_results.clone();
        let step_budget = payload.budget.clone();
        let mut family_output = self
            .family
            .step(
                &session.host_ctx,
                &session.family_session_id,
                LegacyStepInput {
                    tick_index: input.fixed_step,
                    delta_ns: input.delta_ns,
                    session_seed: input.session_seed,
                    mode: match input.mode {
                        RuntimeStepMode::Live => LegacyReplayMode::Live,
                        RuntimeStepMode::RestoreContinuation => {
                            LegacyReplayMode::RestoreContinuation
                        }
                        RuntimeStepMode::Replay => unreachable!(),
                    },
                    input_edges: payload.input_edges,
                    await_results: payload.await_results,
                    provider_results: payload.provider_results,
                    budget: payload.budget,
                },
            )
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        if !session.pending_patch_effects.is_empty() {
            let mut next_sequence = family_output
                .effects
                .iter()
                .map(LegacyEffect::sequence)
                .max()
                .map_or(0, |sequence| sequence.saturating_add(1));
            for mut effect in std::mem::take(&mut session.pending_patch_effects) {
                set_effect_sequence(&mut effect, next_sequence);
                next_sequence = next_sequence.saturating_add(1);
                family_output.effects.push(effect);
            }
            family_output.validate(&step_budget).map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        }
        *session
            .output
            .lock()
            .map_err(|_| "ASTRA_EMU_OUTPUT_LOCK_POISONED")? = Some(family_output);
        let mut ingress = Vec::with_capacity(await_results.len() + 1);
        for result in await_results {
            let token_id = session
                .await_tokens
                .lock()
                .map_err(|_| "ASTRA_EMU_AWAIT_LOCK_POISONED")?
                .remove(&result.token_id)
                .ok_or_else(|| {
                    session.poisoned = true;
                    "ASTRA_EMU_AWAIT_TOKEN_UNKNOWN".to_string()
                })?;
            let mut event = EventPayload::new("await.completed");
            event
                .data
                .insert("status".into(), BlackboardValue::String(result.status));
            event.data.insert(
                "payload_hash".into(),
                BlackboardValue::String(result.payload_hash.to_string()),
            );
            ingress.push(OrderedTickIngress {
                sequence: result.sequence,
                payload: TickIngress::AwaitCompletion(AwaitResult {
                    token_id,
                    sequence: result.sequence,
                    completed_at_step: input.fixed_step,
                    payload: event,
                }),
            });
        }
        let event_sequence = ingress
            .last()
            .map_or(1, |item| item.sequence.saturating_add(1));
        ingress.push(OrderedTickIngress {
            sequence: event_sequence,
            payload: TickIngress::PlayerInput(PlayerInput {
                kind: "emu.step".into(),
                payload: EventPayload::new("emu.step"),
            }),
        });
        let timing = TickInput {
            fixed_step: input.fixed_step,
            delta_ns: input.delta_ns,
            seed: input.session_seed,
        };
        let tick = session
            .world
            .tick(match input.mode {
                RuntimeStepMode::Live => TickRequest::live(timing, ingress),
                RuntimeStepMode::RestoreContinuation => {
                    TickRequest::restore_continuation(timing, ingress)
                }
                RuntimeStepMode::Replay => unreachable!(),
            })
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        if let Some(diagnostic) = tick.diagnostics.first() {
            session.poisoned = true;
            return Err(format!("{}:{}", diagnostic.code, diagnostic.message));
        }
        let mut family_output = session
            .output
            .lock()
            .map_err(|_| "ASTRA_EMU_OUTPUT_LOCK_POISONED")?
            .take()
            .ok_or_else(|| {
                session.poisoned = true;
                "ASTRA_EMU_OUTPUT_MISSING_AFTER_TICK".to_owned()
            })?;
        let render_output = family_output
            .effects
            .iter()
            .position(|effect| {
                matches!(
                    effect,
                    LegacyEffect::Presentation { command, .. }
                        if command == "astra.emu.render_frame.v1"
                )
            })
            .map(|index| family_output.effects.remove(index))
            .map(|effect| match effect {
                LegacyEffect::Presentation {
                    command, payload, ..
                } => RuntimeOutputEnvelope {
                    domain: RuntimeOutputDomain::Presentation,
                    schema: command,
                    version: SchemaVersion::new(1, 0, 0),
                    codec: RuntimeOutputCodec::Postcard,
                    hash: Hash256::from_sha256(&payload),
                    bytes: payload,
                },
                _ => unreachable!("matched render presentation effect"),
            });
        let mut outputs = vec![RuntimeOutputEnvelope::postcard(
            RuntimeOutputDomain::Effect,
            "astra.emu.legacy_step_output.v1",
            SchemaVersion::new(1, 0, 0),
            &family_output,
        )
        .map_err(|error| error.to_string())?];
        if let Some(render_output) = render_output {
            outputs.push(render_output);
        }
        outputs.push(
            RuntimeOutputEnvelope::postcard(
                RuntimeOutputDomain::Trace,
                "astra.emu.legacy_trace.v1",
                SchemaVersion::new(1, 0, 0),
                &family_output.trace,
            )
            .map_err(|error| error.to_string())?,
        );
        outputs.push(
            RuntimeOutputEnvelope::postcard(
                RuntimeOutputDomain::Observation,
                "astra.emu.runtime_observation.v1",
                SchemaVersion::new(1, 0, 0),
                &(tick.state_hash, family_output.state_hash),
            )
            .map_err(|error| error.to_string())?,
        );
        outputs.push(
            RuntimeOutputEnvelope::postcard(
                RuntimeOutputDomain::DirtySaveSection,
                "astra.runtime.dirty_save_section.v1",
                SchemaVersion::new(1, 0, 0),
                &vec!["runtime.world", "emu.family"],
            )
            .map_err(|error| error.to_string())?,
        );
        Ok(RuntimeStepOutput {
            session_id: input.session_id,
            status: format!("{:?}", family_output.status).to_ascii_lowercase(),
            outputs,
            diagnostics: vec![],
        })
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        let session = self
            .sessions
            .get_mut(&request.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        if session
            .output
            .lock()
            .map_err(|_| {
                session.poisoned = true;
                "ASTRA_EMU_OUTPUT_LOCK_POISONED"
            })?
            .is_some()
        {
            session.poisoned = true;
            return Err("ASTRA_EMU_SAVE_DURING_PENDING_EFFECT_TRANSACTION".into());
        }
        let world = session
            .world
            .save(SaveRequest::default())
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        let family = self
            .family
            .save(&session.host_ctx, &session.family_session_id)
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        let await_tokens = session
            .await_tokens
            .lock()
            .map_err(|_| {
                session.poisoned = true;
                "ASTRA_EMU_AWAIT_LOCK_POISONED"
            })?
            .clone();
        let family_bytes = postcard::to_allocvec(&EmuFamilySaveV1 {
            family,
            await_tokens,
            pending_patch_effects: session.pending_patch_effects.clone(),
        })
        .map_err(|error| {
            session.poisoned = true;
            error.to_string()
        })?;
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: vec![
                raw_section("runtime.world", "astra.runtime.save_blob.v2", 2, world.0),
                raw_section(
                    "emu.family",
                    "astra.emu.family_snapshot.v1",
                    1,
                    family_bytes,
                ),
            ],
            diagnostics: vec![],
        })
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        if request.sections.len() != 2 {
            return Err("ASTRA_EMU_RESTORE_SECTION_SET".into());
        }
        let world = required_section(
            &request.sections,
            "runtime.world",
            "astra.runtime.save_blob.v2",
        )?
        .bytes
        .clone();
        let family_bytes = required_section(
            &request.sections,
            "emu.family",
            "astra.emu.family_snapshot.v1",
        )?
        .bytes
        .clone();
        let family_save: EmuFamilySaveV1 =
            postcard::from_bytes(&family_bytes).map_err(|error| error.to_string())?;
        let session = self
            .sessions
            .get_mut(&request.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        let rollback_world = session
            .world
            .save(SaveRequest::default())
            .map_err(|error| error.to_string())?;
        let rollback_family = self
            .family
            .save(&session.host_ctx, &session.family_session_id)
            .map_err(|error| error.to_string())?;
        self.family
            .restore(
                &session.host_ctx,
                &session.family_session_id,
                &family_save.family,
            )
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        if let Err(world_error) = session.world.load(SaveBlob(world)) {
            let family_rollback = self.family.restore(
                &session.host_ctx,
                &session.family_session_id,
                &rollback_family,
            );
            let world_rollback = session.world.load(rollback_world);
            session.poisoned = true;
            if let Err(error) = family_rollback {
                return Err(format!(
                    "ASTRA_EMU_RESTORE_AND_FAMILY_ROLLBACK_FAILED:{world_error};{}",
                    error.code()
                ));
            }
            if let Err(error) = world_rollback {
                return Err(format!(
                    "ASTRA_EMU_RESTORE_AND_WORLD_ROLLBACK_FAILED:{world_error};{error}"
                ));
            }
            return Err(world_error.to_string());
        }
        let pending = session
            .world
            .snapshot()
            .awaits
            .pending()
            .iter()
            .map(|token| token.token_id)
            .collect::<std::collections::BTreeSet<_>>();
        let mapped = family_save
            .await_tokens
            .values()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let mapping_is_valid = mapped.len() == family_save.await_tokens.len()
            && mapped == pending
            && family_save.await_tokens.keys().all(|token| {
                !token.is_empty()
                    && token.len() <= 128
                    && token.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
                    })
            });
        if !mapping_is_valid {
            let family_rollback = self.family.restore(
                &session.host_ctx,
                &session.family_session_id,
                &rollback_family,
            );
            let world_rollback = session.world.load(rollback_world);
            session.poisoned = true;
            if family_rollback.is_err() || world_rollback.is_err() {
                return Err("ASTRA_EMU_AWAIT_MAPPING_INVALID_ROLLBACK_FAILED".into());
            }
            return Err("ASTRA_EMU_AWAIT_MAPPING_INVALID".into());
        }
        *session.await_tokens.lock().map_err(|_| {
            session.poisoned = true;
            "ASTRA_EMU_AWAIT_LOCK_POISONED"
        })? = family_save.await_tokens;
        session.pending_patch_effects = family_save.pending_patch_effects;
        let snapshot = session.world.snapshot();
        Ok(RuntimeRestoreReport {
            session_id: request.session_id,
            restored_fixed_step: snapshot.step,
            session_seed: snapshot.config.seed,
            status: "restored".into(),
            diagnostics: vec![],
        })
    }

    fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        let session = self
            .sessions
            .remove(&session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        self.family
            .shutdown(&session.host_ctx, &session.family_session_id)
            .map_err(|error| error.to_string())?;
        Ok(RuntimeShutdownReport {
            session_id,
            status: "shutdown".into(),
            diagnostics: vec![],
        })
    }
}

fn schema(domain: RuntimeOutputDomain, name: &str) -> RuntimeOutputSchemaDescriptor {
    RuntimeOutputSchemaDescriptor {
        domain,
        schema: name.into(),
        version: SchemaVersion::new(1, 0, 0),
        codec: RuntimeOutputCodec::Postcard,
    }
}
fn required_section<'a>(
    sections: &'a [RuntimeSectionPayload],
    id: &str,
    schema: &str,
) -> Result<&'a RuntimeSectionPayload, String> {
    let mut matches = sections.iter().filter(|section| section.section_id == id);
    let section = matches
        .next()
        .ok_or_else(|| format!("ASTRA_EMU_SECTION_MISSING:{id}"))?;
    if matches.next().is_some() || section.schema != schema || !section.validate_hash() {
        return Err(format!("ASTRA_EMU_SECTION_INVALID:{id}"));
    }
    Ok(section)
}
fn raw_section(id: &str, schema: &str, major: u16, bytes: Vec<u8>) -> RuntimeSectionPayload {
    RuntimeSectionPayload {
        section_id: id.into(),
        schema: schema.into(),
        version: SchemaVersion::new(major, 0, 0),
        codec: RuntimeSectionCodec::Raw,
        hash: Hash256::from_sha256(&bytes),
        bytes,
    }
}
fn wait_kind(wait: &LegacyWaitRequest) -> String {
    match wait {
        LegacyWaitRequest::Frame { .. } => "fvp.frame",
        LegacyWaitRequest::Time { .. } => "fvp.time",
        LegacyWaitRequest::Input { .. } => "fvp.input",
        LegacyWaitRequest::MediaFence { .. } => "fvp.media",
        LegacyWaitRequest::PresentationFence { .. } => "fvp.presentation",
        LegacyWaitRequest::ProviderCompletion { .. } => "fvp.provider",
        LegacyWaitRequest::FamilyOpaque { .. } => "fvp.opaque",
    }
    .into()
}
fn wait_token_id(wait: &LegacyWaitRequest) -> String {
    match wait {
        LegacyWaitRequest::Frame { token_id, .. }
        | LegacyWaitRequest::Time { token_id, .. }
        | LegacyWaitRequest::Input { token_id, .. }
        | LegacyWaitRequest::MediaFence { token_id, .. }
        | LegacyWaitRequest::PresentationFence { token_id, .. }
        | LegacyWaitRequest::ProviderCompletion { token_id, .. }
        | LegacyWaitRequest::FamilyOpaque { token_id, .. } => token_id.clone(),
    }
}
fn set_effect_sequence(effect: &mut LegacyEffect, value: u64) {
    match effect {
        LegacyEffect::RuntimeEvent { sequence, .. }
        | LegacyEffect::Presentation { sequence, .. }
        | LegacyEffect::Audio { sequence, .. }
        | LegacyEffect::TextCapture { sequence, .. }
        | LegacyEffect::SetBlackboard { sequence, .. }
        | LegacyEffect::ScheduleEvent { sequence, .. }
        | LegacyEffect::SnapshotDirty { sequence, .. } => *sequence = value,
    }
}
fn parse_package_hash(value: &str) -> Result<Hash256, String> {
    value
        .parse()
        .map_err(|error| format!("ASTRA_EMU_PACKAGE_HASH:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{LegacyProviderError, LegacyVfsReader};
    use astra_emu_fvp::create_static_fvp_provider;

    struct MemoryVfs {
        script: Vec<u8>,
    }

    impl LegacyVfsReader for MemoryVfs {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" || uri != "script.hcb" {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_NOT_FOUND",
                    "synthetic fixture path is missing",
                ));
            }
            Ok(astra_byte_source::ByteSourceStat {
                len: self.script.len() as u64,
                revision: astra_byte_source::SourceRevision(Hash256::from_sha256(&self.script)),
            })
        }

        fn read_file_range(
            &self,
            mount_set_id: &str,
            uri: &str,
            expected_revision: astra_byte_source::SourceRevision,
            range: astra_byte_source::ByteRange,
            max_bytes: u64,
        ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file(mount_set_id, uri)?;
            range.validate(stat.len, max_bytes).map_err(|error| {
                LegacyProviderError::invalid("TEST_VFS_BOUNDS", error.to_string())
            })?;
            if stat.revision != expected_revision {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_REVISION",
                    "synthetic fixture revision changed",
                ));
            }
            let bytes =
                self.script[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(astra_byte_source::RangeReadResult {
                range,
                revision: stat.revision,
                content_hash: Hash256::from_sha256(&bytes),
                bytes,
            })
        }
    }

    #[test]
    fn fvp_product_provider_full_lifecycle_and_repeated_run_are_deterministic() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let family = create_static_fvp_provider(Arc::new(MemoryVfs { script })).unwrap();
        let mut provider = AstraEmuRuntimeProvider::new(family).unwrap();
        let instance = ProviderInstanceId("emu.test.instance".into());
        provider.create_instance(instance.clone()).unwrap();

        let first = run_once(&mut provider, fingerprint);
        let second = run_once(&mut provider, fingerprint);
        assert_eq!(
            first, second,
            "same package/input identity must replay identically"
        );

        provider.destroy_instance(instance).unwrap();
    }

    fn run_once(provider: &mut AstraEmuRuntimeProvider, fingerprint: Hash256) -> Vec<Hash256> {
        let profile = EmuCaseProfile {
            schema: "astra.emu.case_profile.v1".into(),
            family_id: "fvp".into(),
            case_fingerprint: fingerprint,
            script_uri: "script.hcb".into(),
            fixed_delta_ns: 16_666_667,
            compatibility_profile: "rfvp-v1".into(),
            mount_set_id: "mount.test".into(),
            permission_policy_id: "permission.test".into(),
            family_options: [("fvp.nls".into(), "utf8".into())].into_iter().collect(),
        };
        let bytes = postcard::to_allocvec(&profile).unwrap();
        let package_hash = Hash256::from_sha256(b"package.test").to_string();
        let open = provider
            .open(RuntimeOpenRequest {
                target_id: "windows".into(),
                profile: "fvp-v1".into(),
                locale: "und".into(),
                seed: 17,
                package_hash,
                sections: vec![RuntimeSectionPayload {
                    section_id: "emu.case_profile".into(),
                    schema: "astra.emu.case_profile.v1".into(),
                    version: SchemaVersion::new(1, 0, 0),
                    codec: RuntimeSectionCodec::Postcard,
                    hash: Hash256::from_sha256(&bytes),
                    bytes,
                }],
            })
            .unwrap();
        provider
            .queue_patch_effect(
                &open.session_id,
                LegacyEffect::RuntimeEvent {
                    sequence: 0,
                    event: "patch.synthetic".into(),
                    payload: vec![1, 2, 3],
                },
            )
            .unwrap();
        let output = provider
            .step(RuntimeStepInput {
                session_id: open.session_id.clone(),
                fixed_step: 1,
                delta_ns: 16_666_667,
                session_seed: 17,
                mode: RuntimeStepMode::Live,
                action: "emu.step".into(),
                payload: serde_json::to_value(EmuStepPayload {
                    input_edges: vec![],
                    await_results: vec![],
                    provider_results: vec![],
                    budget: LegacyStepBudget {
                        max_instructions: 32,
                        max_effects: 32,
                        max_trace_entries: 32,
                    },
                })
                .unwrap(),
            })
            .unwrap();
        assert_eq!(output.status, "terminal");
        let family_output = output.outputs[0]
            .decode_postcard::<LegacyStepOutput>(
                RuntimeOutputDomain::Effect,
                "astra.emu.legacy_step_output.v1",
                SchemaVersion::new(1, 0, 0),
            )
            .unwrap();
        assert!(family_output.effects.iter().any(|effect| matches!(
            effect,
            LegacyEffect::RuntimeEvent { event, payload, .. }
                if event == "patch.synthetic" && payload == &[1, 2, 3]
        )));
        let output_hashes = output
            .outputs
            .iter()
            .map(|envelope| Hash256::from_sha256(&envelope.bytes))
            .collect::<Vec<_>>();
        let saved = provider
            .save(RuntimeSaveRequest {
                session_id: open.session_id.clone(),
                slot: "test".into(),
            })
            .unwrap();
        let restored = provider
            .restore(RuntimeRestoreRequest {
                session_id: open.session_id.clone(),
                sections: saved.sections,
            })
            .unwrap();
        assert_eq!(restored.restored_fixed_step, 1);
        provider.shutdown(open.session_id).unwrap();
        output_hashes
    }

    fn terminal_hcb() -> Vec<u8> {
        let mut bytes = 8u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x04, 0, 0, 0]);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[8, 0, 2, b'X', 0]);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }
}
