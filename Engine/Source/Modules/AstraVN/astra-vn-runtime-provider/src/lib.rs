//! Native AstraVN gameplay runtime provider and ABI-safe FFI adapter.

#[cfg(feature = "ffi")]
use std::sync::OnceLock;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[cfg(feature = "ffi")]
use abi_stable::std_types::{RString, RVec};
use astra_core::{Hash128, Hash256, SchemaVersion};
use astra_plugin::ProductRuntimeProvider;
#[cfg(feature = "ffi")]
use astra_plugin_abi::{
    FfiRuntimeProviderRegistration, FfiRuntimeProviderResult, RuntimeProviderCall,
    RuntimeProviderCreateRequest, RuntimeProviderDestroyRequest, RuntimeProviderInstanceReport,
    PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ReleaseCheckDescriptor, RuntimeEditorMetadata,
    RuntimeOpenReport, RuntimeOpenRequest, RuntimeOutputCodec, RuntimeOutputDomain,
    RuntimeOutputEnvelope, RuntimeOutputSchemaDescriptor, RuntimePackageSectionPlan,
    RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport, RuntimeProbeRequest,
    RuntimeRestoreReport, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections,
    RuntimeSectionCodec, RuntimeSectionPayload, RuntimeSectionRef, RuntimeShutdownReport,
    RuntimeStepInput, RuntimeStepOutput, GAME_RUNTIME_PROVIDER_SLOT, NATIVE_VN_PROVIDER_ID,
    NATIVE_VN_RUNTIME_ID, RUNTIME_EDITOR_METADATA_SCHEMA,
};
use astra_runtime::{
    ActionDescriptor, ActionInvocation, ActionTrace, BlackboardValue, ComponentId,
    DeterministicActionContext, EventPayload, GuardExpr, PackageHandle, PlayerInput,
    PresentationCommand as RuntimePresentationCommand, RuntimeAction, RuntimeConfig, RuntimeError,
    RuntimeSnapshot, RuntimeWorld, StateDefinition, StateMachineDefinition, TickInput,
    TransitionDefinition,
};
use astra_vn_core::{
    CompiledStory as CoreCompiledStory, VnError as CoreVnError,
    VnPlayerCommand as CoreVnPlayerCommand, VnRuntime as CoreVnRuntime,
};
use astra_vn_save::{
    VN_POLICY_STATE_SECTION_ID as VN_POLICY_SECTION_ID,
    VN_RUNTIME_STATE_SECTION_ID as VN_RUNTIME_SECTION_ID,
};

const VN_RUNTIME_WORLD_SECTION_ID: &str = "vn.runtime_world";
const VN_RUNTIME_WORLD_SCHEMA: &str = "astra.vn.runtime_world_snapshot.v1";

pub use astra_vn_core::*;
pub use astra_vn_editor::*;
pub use astra_vn_package::*;
pub use astra_vn_save::*;

#[derive(Default)]
pub struct NativeVnRuntimeProvider {
    sessions: BTreeMap<String, NativeVnSession>,
}

fn output_schema(
    domain: RuntimeOutputDomain,
    schema: &str,
    major: u16,
) -> RuntimeOutputSchemaDescriptor {
    RuntimeOutputSchemaDescriptor {
        domain,
        schema: schema.to_string(),
        version: SchemaVersion::new(major, 0, 0),
        codec: RuntimeOutputCodec::Postcard,
    }
}

impl ProductRuntimeProvider for NativeVnRuntimeProvider {
    fn prepare(&mut self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Ok(NativeVnRuntimeProvider::prepare(self, request))
    }

    fn probe(&mut self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Ok(NativeVnRuntimeProvider::probe(self, request))
    }

    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        let compiled_section = required_restore_section(
            &request.sections,
            "vn.compiled_story",
            "astra.vn.compiled_story",
        )
        .map_err(|err| err.to_string())?;
        let compiled: CoreCompiledStory =
            postcard::from_bytes(&compiled_section.bytes).map_err(|err| err.to_string())?;
        let config = VnRunConfig {
            profile: request.profile.clone(),
            locale: request.locale.clone(),
        };
        self.open_compiled_story(compiled, config, request)
            .map_err(|err| err.to_string())
    }

    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        NativeVnRuntimeProvider::step(self, input).map_err(|err| err.to_string())
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        NativeVnRuntimeProvider::save(self, request).map_err(|err| err.to_string())
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        NativeVnRuntimeProvider::restore(self, request).map_err(|err| err.to_string())
    }

    fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        NativeVnRuntimeProvider::shutdown(self, session_id).map_err(|err| err.to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NativeVnStepEffect {
    coverage_reached: std::collections::BTreeSet<String>,
    state_hash_before_advance: String,
    state_hash_after_advance: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NativeVnStepTrace {
    runtime_state_hash: String,
    runtime_event_hash: String,
    runtime_presentation_hash: String,
}

struct NativeVnSession {
    world: RuntimeWorld,
    vn_component: ComponentId,
    policy_component: ComponentId,
    compiled: Arc<CoreCompiledStory>,
    output: Arc<Mutex<Option<VnStepOutput>>>,
}

struct VnStepAction {
    component: ComponentId,
    compiled: Arc<CoreCompiledStory>,
    output: Arc<Mutex<Option<VnStepOutput>>>,
}

impl NativeVnRuntimeProvider {
    pub fn slot() -> &'static str {
        GAME_RUNTIME_PROVIDER_SLOT
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn descriptor() -> ProductRuntimeDescriptor {
        ProductRuntimeDescriptor {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            product_kind: "visual_novel".to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            supported_targets: vec!["game".to_string()],
            capabilities: vec!["runtime.native_vn".to_string()],
            package_sections: native_vn_package_sections(),
            release_checks: native_vn_release_check_ids(),
            output_schemas: vec![
                output_schema(
                    RuntimeOutputDomain::Effect,
                    "astra.vn.runtime_step_effect.v2",
                    2,
                ),
                output_schema(
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v1",
                    1,
                ),
                output_schema(RuntimeOutputDomain::Audio, "astra.vn.audio_command.v1", 1),
                output_schema(RuntimeOutputDomain::Await, "astra.runtime.await_id.v1", 1),
                output_schema(RuntimeOutputDomain::Effect, "astra.vn.timeline_task.v1", 1),
                output_schema(
                    RuntimeOutputDomain::Trace,
                    "astra.vn.runtime_step_trace.v1",
                    1,
                ),
                output_schema(RuntimeOutputDomain::Trace, "astra.vn.runtime_state.v1", 1),
                output_schema(
                    RuntimeOutputDomain::DirtySaveSection,
                    "astra.runtime.dirty_save_section.v1",
                    1,
                ),
            ],
        }
    }

    pub fn prepare(&self, request: RuntimePrepareRequest) -> RuntimePrepareReport {
        tracing::info!(
            event = "vn.provider.prepare.start",
            section_count = request.section_ids.len(),
            "AstraVN runtime provider preparation started"
        );
        let mut diagnostics = Vec::new();
        if request
            .section_ids
            .iter()
            .all(|section| section != "vn.compiled_story")
        {
            diagnostics.push("ASTRA_NATIVE_VN_COMPILED_STORY_MISSING".to_string());
        }
        RuntimePrepareReport {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            status: if diagnostics.is_empty() {
                "pass".to_string()
            } else {
                "blocked".to_string()
            },
            diagnostics,
        }
    }

    pub fn probe(&self, request: RuntimeProbeRequest) -> RuntimeProbeReport {
        let prepare = self.prepare(RuntimePrepareRequest {
            target_id: request.target_id,
            profile: request.profile,
            package_hash: String::new(),
            section_ids: request.section_ids,
        });
        RuntimeProbeReport {
            runtime_id: prepare.runtime_id,
            provider_id: prepare.provider_id,
            status: prepare.status,
            diagnostics: prepare.diagnostics,
        }
    }

    pub fn open_compiled_story(
        &mut self,
        compiled: CoreCompiledStory,
        config: VnRunConfig,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, CoreVnError> {
        tracing::info!(
            event = "vn.provider.session.open.start",
            target_id = %request.target_id,
            seed = request.seed,
            "AstraVN runtime session open started"
        );
        let session_id = GameRuntimeSessionId(format!(
            "{}:{}:{}",
            NATIVE_VN_RUNTIME_ID, request.target_id, request.seed
        ));
        if self.sessions.contains_key(&session_id.0) {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_DUPLICATE",
                "runtime session id is already open",
            ));
        }
        let initial_runtime = CoreVnRuntime::new(compiled.clone(), config)?;
        let mut world = RuntimeWorld::create(
            RuntimeConfig {
                seed: request.seed,
                required_slots: Vec::new(),
            },
            PackageHandle {
                package_id: request.package_hash.clone(),
                target: request.target_id.clone(),
                ..PackageHandle::default()
            },
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?;
        let owner = world.create_actor("astra.vn.runtime", vec!["gameplay_runtime".to_string()]);
        let vn_component = world
            .attach_component(owner, "astra.vn.runtime_state.v1", initial_runtime.state())
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let policy_component = world
            .attach_component(owner, "astra.vn.policy_state.v1", &VnPolicyState::default())
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let compiled = Arc::new(compiled);
        let output = Arc::new(Mutex::new(None));
        world
            .register_action(
                NATIVE_VN_PROVIDER_ID,
                VnStepAction {
                    component: vn_component,
                    compiled: Arc::clone(&compiled),
                    output: Arc::clone(&output),
                },
            )
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let running = astra_core::StableId::deterministic_v7(0, 1, request.seed);
        world
            .add_state_machine(StateMachineDefinition {
                id: astra_core::StableId::deterministic_v7(0, 2, request.seed),
                owner,
                states: vec![StateDefinition {
                    id: running,
                    name: "vn.running".to_string(),
                    terminal: false,
                }],
                transitions: vec![TransitionDefinition {
                    from: running,
                    to: running,
                    guard: GuardExpr::Or {
                        terms: vn_runtime_event_kinds()
                            .into_iter()
                            .map(|kind| GuardExpr::EventIs {
                                kind: kind.to_string(),
                            })
                            .collect(),
                    },
                    actions: vec![ActionInvocation {
                        action_id: "astra.vn.step".to_string(),
                        input: BTreeMap::new(),
                    }],
                    priority: 0,
                    source_ref: None,
                }],
                initial_state: running,
            })
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        self.sessions.insert(
            session_id.0.clone(),
            NativeVnSession {
                world,
                vn_component,
                policy_component,
                compiled,
                output,
            },
        );
        Ok(RuntimeOpenReport {
            session_id,
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            diagnostics: Vec::new(),
        })
    }

    pub fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, CoreVnError> {
        tracing::trace!(
            event = "vn.provider.session.step",
            fixed_step = input.fixed_step,
            "AstraVN runtime session step started"
        );
        let command = {
            let session = self.session(&input.session_id)?;
            let state = session
                .world
                .read_component::<VnRuntimeState>(session.vn_component)
                .map_err(|err| CoreVnError::message(err.to_string()))?;
            runtime_command_from_input(&session.compiled, &state, &input)?
        };
        self.apply_command_at_step(input.session_id, command, input.fixed_step)
    }

    pub fn apply_command(
        &mut self,
        session_id: GameRuntimeSessionId,
        command: CoreVnPlayerCommand,
    ) -> Result<RuntimeStepOutput, CoreVnError> {
        let step = self.session(&session_id)?.world.snapshot().step + 1;
        self.apply_command_at_step(session_id, command, step)
    }

    fn apply_command_at_step(
        &mut self,
        session_id: GameRuntimeSessionId,
        command: CoreVnPlayerCommand,
        fixed_step: u64,
    ) -> Result<RuntimeStepOutput, CoreVnError> {
        let session = self.session_mut(&session_id)?;
        *session
            .output
            .lock()
            .map_err(|_| CoreVnError::message("VN step output lock is poisoned"))? = None;
        let event_kind = vn_event_kind(&command).to_string();
        let command_bytes = postcard::to_allocvec(&command)?;
        let state = session
            .world
            .read_component::<VnRuntimeState>(session.vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        if command_resolves_wait(&command, state.pending_wait.as_ref().map(|wait| wait.kind)) {
            let await_id = state
                .pending_wait
                .as_ref()
                .and_then(|wait| wait.await_id.as_deref())
                .ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_AWAIT_ID_MISSING",
                        "VN wait does not reference its Runtime AwaitToken",
                    )
                })?;
            let token_id = astra_runtime::AwaitTokenId(
                astra_core::StableId::parse(await_id)
                    .map_err(|err| CoreVnError::message(err.to_string()))?,
            );
            session
                .world
                .submit_await_result(astra_runtime::AwaitResult {
                    token_id,
                    sequence: fixed_step,
                    completed_at_step: fixed_step,
                    payload: EventPayload::new("await.resolved"),
                });
        }
        session
            .world
            .apply_input(PlayerInput {
                kind: event_kind.clone(),
                payload: EventPayload {
                    kind: event_kind,
                    data: [("command".to_string(), BlackboardValue::Bytes(command_bytes))]
                        .into_iter()
                        .collect(),
                },
            })
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let seed = session.world.snapshot().config.seed;
        let tick = session
            .world
            .tick(TickInput {
                fixed_step,
                delta_ns: 16_666_667,
                seed,
            })
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        if let Some(diagnostic) = tick.diagnostics.first() {
            return Err(CoreVnError::diagnostic(
                diagnostic.code.clone(),
                diagnostic.message.clone(),
            ));
        }
        let output = session
            .output
            .lock()
            .map_err(|_| CoreVnError::message("VN step output lock is poisoned"))?
            .take()
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_STEP_OUTPUT_MISSING",
                    "astra.vn.step did not produce an output",
                )
            })?;
        let current_state = session
            .world
            .read_component::<VnRuntimeState>(session.vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let presentation = output
            .presentation
            .iter()
            .map(|command| {
                RuntimeOutputEnvelope::postcard(
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v1",
                    SchemaVersion::new(1, 0, 0),
                    command,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let audio = output
            .audio
            .iter()
            .map(|command| {
                RuntimeOutputEnvelope::postcard(
                    RuntimeOutputDomain::Audio,
                    "astra.vn.audio_command.v1",
                    SchemaVersion::new(1, 0, 0),
                    command,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let awaits = output
            .awaits
            .iter()
            .map(|await_id| {
                RuntimeOutputEnvelope::postcard(
                    RuntimeOutputDomain::Await,
                    "astra.runtime.await_id.v1",
                    SchemaVersion::new(1, 0, 0),
                    await_id,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let timeline = output
            .timeline_tasks
            .iter()
            .map(|task| {
                RuntimeOutputEnvelope::postcard(
                    RuntimeOutputDomain::Effect,
                    "astra.vn.timeline_task.v1",
                    SchemaVersion::new(1, 0, 0),
                    task,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let effects = vec![RuntimeOutputEnvelope::postcard(
            RuntimeOutputDomain::Effect,
            "astra.vn.runtime_step_effect.v2",
            SchemaVersion::new(2, 0, 0),
            &NativeVnStepEffect {
                coverage_reached: output.coverage.reached,
                state_hash_before_advance: output.state_hash_before_advance.to_string(),
                state_hash_after_advance: output.state_hash_after_advance.to_string(),
            },
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?];
        let trace = vec![
            RuntimeOutputEnvelope::postcard(
                RuntimeOutputDomain::Trace,
                "astra.vn.runtime_step_trace.v1",
                SchemaVersion::new(1, 0, 0),
                &NativeVnStepTrace {
                    runtime_state_hash: tick.state_hash.to_string(),
                    runtime_event_hash: tick.event_hash.to_string(),
                    runtime_presentation_hash: tick.presentation_hash.to_string(),
                },
            )
            .map_err(|err| CoreVnError::message(err.to_string()))?,
            RuntimeOutputEnvelope::postcard(
                RuntimeOutputDomain::Trace,
                "astra.vn.runtime_state.v1",
                SchemaVersion::new(1, 0, 0),
                &current_state,
            )
            .map_err(|err| CoreVnError::message(err.to_string()))?,
        ];
        let dirty_save_sections = vec![RuntimeOutputEnvelope::postcard(
            RuntimeOutputDomain::DirtySaveSection,
            "astra.runtime.dirty_save_section.v1",
            SchemaVersion::new(1, 0, 0),
            &VN_RUNTIME_SECTION_ID.to_string(),
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?];
        Ok(RuntimeStepOutput {
            session_id,
            status: if presentation.is_empty() {
                "idle".to_string()
            } else {
                "blocked".to_string()
            },
            outputs: effects
                .into_iter()
                .chain(presentation)
                .chain(audio)
                .chain(timeline)
                .chain(awaits)
                .chain(trace)
                .chain(dirty_save_sections)
                .collect(),
            diagnostics: Vec::new(),
        })
    }

    pub fn default_launch_command(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<CoreVnPlayerCommand, CoreVnError> {
        let session = self.session(session_id)?;
        let state = session
            .world
            .read_component::<VnRuntimeState>(session.vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        CoreVnRuntime::from_state((*session.compiled).clone(), state)?
            .default_launch_command()
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                    "compiled story has no launchable state",
                )
            })
    }

    pub fn state(&self, session_id: &GameRuntimeSessionId) -> Result<VnRuntimeState, CoreVnError> {
        let session = self.session(session_id)?;
        session
            .world
            .read_component(session.vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))
    }

    pub fn runtime_snapshot(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<RuntimeSnapshot, CoreVnError> {
        Ok(self.session(session_id)?.world.snapshot())
    }

    pub fn save_slot(
        &self,
        session_id: &GameRuntimeSessionId,
        slot: impl Into<String>,
    ) -> Result<VnSaveBlob, CoreVnError> {
        let state = self.state(session_id)?;
        let state_hash = Hash128::from_blake3(&postcard::to_allocvec(&state)?);
        Ok(VnSaveBlob {
            schema: "astra.vn.save_slot.v1".to_string(),
            slot: slot.into(),
            state_hash,
            state,
        })
    }

    pub fn load_slot(
        &mut self,
        session_id: &GameRuntimeSessionId,
        save: VnSaveBlob,
    ) -> Result<(), CoreVnError> {
        if save.schema != "astra.vn.save_slot.v1" {
            return Err(CoreVnError::diagnostic(
                "ASTRA_VN_SAVE_SCHEMA",
                "AstraVN save slot schema is invalid",
            ));
        }
        let session = self.session_mut(session_id)?;
        session
            .world
            .replace_component(session.vn_component, &save.state)
            .map_err(|err| CoreVnError::message(err.to_string()))
    }

    pub fn save(&self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, CoreVnError> {
        let session = self.session(&request.session_id)?;
        let runtime = session
            .world
            .read_component::<VnRuntimeState>(session.vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let runtime_state = VnRuntimeStateSave {
            schema: "astra.vn.runtime_state_save.v1".to_string(),
            state_hash: Hash128::from_blake3(&postcard::to_allocvec(&runtime)?),
            state: runtime,
        };
        let state_payload = postcard::to_allocvec(&runtime_state)?;
        let policy = session
            .world
            .read_component::<VnPolicyState>(session.policy_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let policy_state = VnPolicyStateSave {
            schema: "astra.vn.policy_state_save.v1".to_string(),
            state_hash: Hash128::from_blake3(&postcard::to_allocvec(&policy)?),
            state: policy,
        };
        let policy_payload = postcard::to_allocvec(&policy_state)?;
        let world_snapshot = session.world.snapshot();
        let world_payload = postcard::to_allocvec(&world_snapshot)?;
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: vec![
                RuntimeSectionPayload {
                    section_id: VN_RUNTIME_SECTION_ID.to_string(),
                    schema: "astra.vn.runtime_state_save.v1".to_string(),
                    version: SchemaVersion::default(),
                    codec: RuntimeSectionCodec::Postcard,
                    hash: Hash256::from_sha256(&state_payload),
                    bytes: state_payload,
                },
                RuntimeSectionPayload {
                    section_id: VN_POLICY_SECTION_ID.to_string(),
                    schema: "astra.vn.policy_state_save.v1".to_string(),
                    version: SchemaVersion::default(),
                    codec: RuntimeSectionCodec::Postcard,
                    hash: Hash256::from_sha256(&policy_payload),
                    bytes: policy_payload,
                },
                RuntimeSectionPayload {
                    section_id: VN_RUNTIME_WORLD_SECTION_ID.to_string(),
                    schema: VN_RUNTIME_WORLD_SCHEMA.to_string(),
                    version: SchemaVersion::default(),
                    codec: RuntimeSectionCodec::Postcard,
                    hash: Hash256::from_sha256(&world_payload),
                    bytes: world_payload,
                },
            ],
            diagnostics: Vec::new(),
        })
    }

    pub fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, CoreVnError> {
        let runtime_section = required_restore_section(
            &request.sections,
            VN_RUNTIME_SECTION_ID,
            "astra.vn.runtime_state_save.v1",
        )?;
        let policy_section = required_restore_section(
            &request.sections,
            VN_POLICY_SECTION_ID,
            "astra.vn.policy_state_save.v1",
        )?;
        let world_section = required_restore_section(
            &request.sections,
            VN_RUNTIME_WORLD_SECTION_ID,
            VN_RUNTIME_WORLD_SCHEMA,
        )?;
        let runtime_save: VnRuntimeStateSave = postcard::from_bytes(&runtime_section.bytes)?;
        let policy_save: VnPolicyStateSave = postcard::from_bytes(&policy_section.bytes)?;
        let world_snapshot: RuntimeSnapshot = postcard::from_bytes(&world_section.bytes)?;
        let runtime_hash = Hash128::from_blake3(&postcard::to_allocvec(&runtime_save.state)?);
        if runtime_hash != runtime_save.state_hash {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_RUNTIME_STATE_HASH",
                "VN runtime state hash does not match the restored state",
            ));
        }
        let policy_hash = Hash128::from_blake3(&postcard::to_allocvec(&policy_save.state)?);
        if policy_hash != policy_save.state_hash {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_POLICY_STATE_HASH",
                "VN policy state hash does not match the restored state",
            ));
        }
        let session = self.session_mut(&request.session_id)?;
        let restored_runtime = world_snapshot
            .actors
            .component(session.vn_component)
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_WORLD_COMPONENT_MISSING",
                    "RuntimeWorld snapshot is missing the VN component",
                )
            })?
            .payload
            .decode::<VnRuntimeState>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let restored_policy = world_snapshot
            .actors
            .component(session.policy_component)
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_WORLD_COMPONENT_MISSING",
                    "RuntimeWorld snapshot is missing the policy component",
                )
            })?
            .payload
            .decode::<VnPolicyState>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        if restored_runtime != runtime_save.state || restored_policy != policy_save.state {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_WORLD_COMPONENT_MISMATCH",
                "RuntimeWorld snapshot does not match VN and policy save sections",
            ));
        }
        session.world.restore_snapshot(world_snapshot);
        session
            .world
            .replace_component(session.vn_component, &runtime_save.state)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        session
            .world
            .replace_component(session.policy_component, &policy_save.state)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        Ok(RuntimeRestoreReport {
            session_id: request.session_id,
            status: "restored".to_string(),
            diagnostics: Vec::new(),
        })
    }

    pub fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, CoreVnError> {
        self.sessions.remove(&session_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISSING",
                "runtime session is not open",
            )
        })?;
        Ok(RuntimeShutdownReport {
            session_id,
            status: "shutdown".to_string(),
            diagnostics: Vec::new(),
        })
    }

    pub fn package_sections(&self) -> RuntimePackageSectionPlan {
        RuntimePackageSectionPlan {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            sections: native_vn_package_sections()
                .into_iter()
                .map(|section_id| RuntimeSectionRef {
                    section_id,
                    schema: "astra.vn.package_section.v1".to_string(),
                })
                .collect(),
        }
    }

    pub fn release_checks(&self) -> Vec<ReleaseCheckDescriptor> {
        native_vn_release_check_ids()
            .into_iter()
            .map(|id| ReleaseCheckDescriptor {
                domain: if id.starts_with("runtime_provider") {
                    "runtime_provider".to_string()
                } else {
                    "visual_novel".to_string()
                },
                id,
                required: true,
            })
            .collect()
    }

    pub fn editor_metadata(&self) -> RuntimeEditorMetadata {
        RuntimeEditorMetadata {
            schema: RUNTIME_EDITOR_METADATA_SCHEMA.to_string(),
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            product_kind: "visual_novel".to_string(),
            project_templates: vec!["native_vn".to_string(), "advanced_vn".to_string()],
            authoring_surfaces: vec![
                "script".to_string(),
                "graph".to_string(),
                "timeline".to_string(),
                "system_pages".to_string(),
            ],
            debug_views: vec![
                "route_graph".to_string(),
                "runtime_state".to_string(),
                "policy_trace".to_string(),
                "presentation_state".to_string(),
            ],
            release_checks: native_vn_release_check_ids(),
        }
    }

    #[cfg(feature = "ffi")]
    pub fn ffi_registration() -> FfiRuntimeProviderRegistration {
        FfiRuntimeProviderRegistration {
            provider_id: RString::from(NATIVE_VN_PROVIDER_ID),
            runtime_id: RString::from(NATIVE_VN_RUNTIME_ID),
            capability: RString::from("runtime.native_vn"),
            phase: RString::from("runtime"),
            packaged: true,
            descriptor_schema: RString::from(PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA),
            descriptor_json: RVec::from(serde_json::to_vec(&Self::descriptor()).unwrap()),
            create_instance: ffi_create_instance,
            destroy_instance: ffi_destroy_instance,
            prepare: ffi_prepare,
            probe: ffi_probe,
            open: ffi_open,
            step: ffi_step,
            save: ffi_save,
            restore: ffi_restore,
            shutdown: ffi_shutdown,
            package_sections: ffi_package_sections,
            release_checks: ffi_release_checks,
            editor_metadata: ffi_editor_metadata,
        }
    }

    fn session(&self, session_id: &GameRuntimeSessionId) -> Result<&NativeVnSession, CoreVnError> {
        self.sessions.get(&session_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISSING",
                "runtime session is not open",
            )
        })
    }

    fn session_mut(
        &mut self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<&mut NativeVnSession, CoreVnError> {
        self.sessions.get_mut(&session_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISSING",
                "runtime session is not open",
            )
        })
    }
}

fn runtime_command_from_input(
    compiled: &CoreCompiledStory,
    state: &VnRuntimeState,
    input: &RuntimeStepInput,
) -> Result<CoreVnPlayerCommand, CoreVnError> {
    match input.action.as_str() {
        "command" => serde_json::from_value(input.payload.clone())
            .map_err(|err| CoreVnError::message(format!("decode VN player command: {err}"))),
        "launch_default" => CoreVnRuntime::from_state(compiled.clone(), state.clone())?
            .default_launch_command()
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                    "compiled story has no launchable state",
                )
            }),
        "advance" => Ok(CoreVnPlayerCommand::Advance),
        "choose" => Ok(CoreVnPlayerCommand::Choose {
            option_id: required_payload_string(&input.payload, "option_id")?,
        }),
        "complete_wait" => Ok(CoreVnPlayerCommand::CompleteWait {
            fence: required_payload_string(&input.payload, "fence")?,
        }),
        "system_return" => Ok(CoreVnPlayerCommand::ReturnSystem),
        other => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_UNKNOWN",
            format!("runtime action {other} is not supported"),
        )),
    }
}

impl RuntimeAction for VnStepAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor {
            id: "astra.vn.step".to_string(),
            input_schema: "astra.vn.step_action_input.v1".to_string(),
            output_schema: "astra.vn.step_output.v1".to_string(),
        }
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let event = ctx.trigger_event().ok_or_else(|| {
            RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                "ASTRA_VN_STEP_TRIGGER_MISSING",
                "astra.vn.step requires a trigger event",
            ))
        })?;
        let event_kind = event.payload.kind.clone();
        let command_bytes = match event.payload.data.get("command") {
            Some(BlackboardValue::Bytes(bytes)) => bytes.clone(),
            _ => {
                return Err(RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                    "ASTRA_VN_STEP_COMMAND_MISSING",
                    "astra.vn.step trigger does not contain a serialized command",
                )))
            }
        };
        let command: CoreVnPlayerCommand = postcard::from_bytes(&command_bytes)
            .map_err(|err| RuntimeError::message(format!("decode VN step command: {err}")))?;
        let previous_state = ctx.read_component::<VnRuntimeState>(self.component)?;
        let previous_wait = previous_state.pending_wait.clone();
        let (mut state, mut output) =
            astra_vn_core::reduce_vn_step(&self.compiled, &previous_state, command)
                .map_err(|err| RuntimeError::message(err.to_string()))?;
        if state.pending_wait != previous_wait {
            if let Some(wait) = state.pending_wait.as_mut() {
                let token = ctx.create_await(astra_runtime::AwaitKind::Custom(format!(
                    "vn.{:?}",
                    wait.kind
                )));
                wait.await_id = Some(token.token_id.0.to_string());
                output.wait = Some(wait.clone());
                output.awaits.push(token.token_id.0.to_string());
                ctx.push_await(token)?;
            }
        }
        ctx.replace_component(self.component, &state)?;
        for event in &output.events {
            ctx.emit_event(
                astra_runtime::EventSource::StateMachine,
                EventPayload {
                    kind: event.kind.clone(),
                    data: [("id".to_string(), BlackboardValue::String(event.id.clone()))]
                        .into_iter()
                        .collect(),
                },
            );
        }
        for command in &output.presentation {
            ctx.emit_presentation(runtime_presentation(command));
        }
        for command in &output.audio {
            ctx.emit_serialized_effect("audio", "astra.vn.audio_command.v1", command)?;
        }
        for task in &output.timeline_tasks {
            ctx.emit_serialized_effect("timeline", "astra.vn.timeline_task.v1", task)?;
        }
        let mut trace_payload = input.clone();
        trace_payload.insert(
            "event_kind".to_string(),
            BlackboardValue::String(event_kind),
        );
        trace_payload.insert(
            "state_hash_before".to_string(),
            BlackboardValue::String(output.state_hash_before_advance.to_string()),
        );
        trace_payload.insert(
            "state_hash_after".to_string(),
            BlackboardValue::String(output.state_hash_after_advance.to_string()),
        );
        *self
            .output
            .lock()
            .map_err(|_| RuntimeError::message("VN step output lock is poisoned"))? = Some(output);
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: trace_payload,
        })
    }
}

fn runtime_presentation(command: &PresentationCommand) -> RuntimePresentationCommand {
    match command {
        PresentationCommand::Dialogue {
            key,
            speaker,
            voice,
            window,
        } => RuntimePresentationCommand::Custom {
            kind: "vn.dialogue".to_string(),
            data: [
                ("key".to_string(), BlackboardValue::String(key.clone())),
                (
                    "speaker".to_string(),
                    speaker
                        .clone()
                        .map(BlackboardValue::String)
                        .unwrap_or(BlackboardValue::Null),
                ),
                (
                    "voice".to_string(),
                    voice
                        .clone()
                        .map(BlackboardValue::String)
                        .unwrap_or(BlackboardValue::Null),
                ),
                (
                    "window".to_string(),
                    window
                        .clone()
                        .map(BlackboardValue::String)
                        .unwrap_or(BlackboardValue::Null),
                ),
            ]
            .into_iter()
            .collect(),
        },
        PresentationCommand::Choice { key, options } => RuntimePresentationCommand::Custom {
            kind: "vn.choice".to_string(),
            data: [
                ("key".to_string(), BlackboardValue::String(key.clone())),
                (
                    "options".to_string(),
                    BlackboardValue::List(
                        options
                            .iter()
                            .map(|option| BlackboardValue::String(option.id.clone()))
                            .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        },
        PresentationCommand::SystemPage { page } => RuntimePresentationCommand::Custom {
            kind: "vn.system_page".to_string(),
            data: [(
                "page".to_string(),
                BlackboardValue::String(format!("{page:?}")),
            )]
            .into_iter()
            .collect(),
        },
        PresentationCommand::Stage {
            command,
            attributes,
        } => RuntimePresentationCommand::Custom {
            kind: format!("vn.stage.{command}"),
            data: attributes
                .iter()
                .map(|(key, value)| (key.clone(), BlackboardValue::String(value.clone())))
                .collect(),
        },
        PresentationCommand::Marker { id } => {
            RuntimePresentationCommand::Marker { name: id.clone() }
        }
    }
}

fn vn_event_kind(command: &CoreVnPlayerCommand) -> &'static str {
    match command {
        CoreVnPlayerCommand::Launch { .. } => "vn.launch",
        CoreVnPlayerCommand::Advance => "player.advance",
        CoreVnPlayerCommand::Choose { .. } => "choice.selected",
        CoreVnPlayerCommand::OpenSystem { .. } => "system.open",
        CoreVnPlayerCommand::ReturnSystem => "system.return",
        CoreVnPlayerCommand::ReplayVoice { .. } => "voice.replay",
        CoreVnPlayerCommand::SetAuto { .. } => "system.auto",
        CoreVnPlayerCommand::SetSkip { .. } => "system.skip",
        CoreVnPlayerCommand::SetConfig { .. } => "system.config",
        CoreVnPlayerCommand::Unlock { .. } => "system.unlock",
        CoreVnPlayerCommand::CompleteWait { .. } => "await.completed",
    }
}

fn vn_runtime_event_kinds() -> [&'static str; 11] {
    [
        "vn.launch",
        "player.advance",
        "choice.selected",
        "system.open",
        "system.return",
        "voice.replay",
        "system.auto",
        "system.skip",
        "system.config",
        "system.unlock",
        "await.completed",
    ]
}

fn command_resolves_wait(command: &CoreVnPlayerCommand, wait: Option<VnWaitKind>) -> bool {
    matches!(
        (command, wait),
        (CoreVnPlayerCommand::Advance, Some(VnWaitKind::Dialogue))
            | (CoreVnPlayerCommand::Choose { .. }, Some(VnWaitKind::Choice))
            | (
                CoreVnPlayerCommand::ReturnSystem,
                Some(VnWaitKind::SystemPage)
            )
            | (
                CoreVnPlayerCommand::CompleteWait { .. },
                Some(
                    VnWaitKind::Fence
                        | VnWaitKind::Timer
                        | VnWaitKind::TimelineComplete
                        | VnWaitKind::MovieEnd
                        | VnWaitKind::VoiceEnd
                )
            )
    )
}

fn required_payload_string(payload: &serde_json::Value, key: &str) -> Result<String, CoreVnError> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_ACTION_PAYLOAD",
                format!("runtime action payload is missing {key}"),
            )
        })
}

fn required_restore_section<'a>(
    sections: &'a [RuntimeSectionPayload],
    section_id: &str,
    schema: &str,
) -> Result<&'a RuntimeSectionPayload, CoreVnError> {
    let mut matches = sections
        .iter()
        .filter(|section| section.section_id == section_id);
    let section = matches.next().ok_or_else(|| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_SECTION_MISSING",
            format!("restore section {section_id} is missing"),
        )
    })?;
    if matches.next().is_some() {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_SECTION_DUPLICATE",
            format!("restore section {section_id} is duplicated"),
        ));
    }
    if section.schema != schema || section.codec != RuntimeSectionCodec::Postcard {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_SECTION_SCHEMA",
            format!("restore section {section_id} has an incompatible schema or codec"),
        ));
    }
    if !section.validate_hash() {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_SECTION_HASH",
            format!("restore section {section_id} failed hash validation"),
        ));
    }
    Ok(section)
}

fn native_vn_package_sections() -> Vec<String> {
    [
        "vn.compiled_story",
        "vn.profile_manifest",
        "vn.policy_bundle_manifest",
        "vn.extension_manifest",
        "vn.standard_command_manifest",
        "vn.presentation_provider_manifest",
        "vn.commercial_baseline_manifest",
        "vn.system_story_manifest",
        "vn.system_ui_profile_manifest",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn native_vn_release_check_ids() -> Vec<String> {
    [
        "runtime_provider.native_vn",
        "vn.commercial_baseline",
        "vn.system_ui_profile",
        "vn.advanced_presentation",
        "player.full_playable",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_prepare(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimePrepareRequest| {
        NativeVnRuntimeProvider::default().prepare(request)
    })
}

#[cfg(feature = "ffi")]
static FFI_INSTANCES: OnceLock<Mutex<BTreeMap<String, NativeVnRuntimeProvider>>> = OnceLock::new();

#[cfg(feature = "ffi")]
fn ffi_instances() -> &'static Mutex<BTreeMap<String, NativeVnRuntimeProvider>> {
    FFI_INSTANCES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_create_instance(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_result(payload, |request: RuntimeProviderCreateRequest| {
        let mut instances = ffi_instances()
            .lock()
            .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
        if instances.contains_key(&request.instance_id.0) {
            return Err("provider instance id is already active".to_string());
        }
        instances.insert(
            request.instance_id.0.clone(),
            NativeVnRuntimeProvider::default(),
        );
        Ok(RuntimeProviderInstanceReport {
            instance_id: request.instance_id,
            status: "created".to_string(),
            diagnostics: Vec::new(),
        })
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_destroy_instance(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_result(payload, |request: RuntimeProviderDestroyRequest| {
        let mut instances = ffi_instances()
            .lock()
            .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
        let instance = instances
            .get(&request.instance_id.0)
            .ok_or_else(|| "provider instance is not active".to_string())?;
        if instance.session_count() != 0 {
            return Err("provider instance still has active sessions".to_string());
        }
        instances.remove(&request.instance_id.0);
        Ok(RuntimeProviderInstanceReport {
            instance_id: request.instance_id,
            status: "destroyed".to_string(),
            diagnostics: Vec::new(),
        })
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_probe(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimeProbeRequest| {
        NativeVnRuntimeProvider::default().probe(request)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_open(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_instance_json(payload, |provider, request: RuntimeOpenRequest| {
        let compiled_section = required_restore_section(
            &request.sections,
            "vn.compiled_story",
            "astra.vn.compiled_story",
        )?;
        let compiled: CoreCompiledStory = postcard::from_bytes(&compiled_section.bytes)?;
        let config = VnRunConfig {
            profile: request.profile.clone(),
            locale: request.locale.clone(),
        };
        provider.open_compiled_story(compiled, config, request)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_step(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_instance_json(payload, NativeVnRuntimeProvider::step)
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_save(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_instance_json(payload, |provider, request: RuntimeSaveRequest| {
        NativeVnRuntimeProvider::save(provider, request)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_restore(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_instance_json(payload, NativeVnRuntimeProvider::restore)
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_shutdown(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_instance_json(payload, |provider, session_id: GameRuntimeSessionId| {
        provider.shutdown(session_id)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_package_sections(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_value(NativeVnRuntimeProvider::default().package_sections())
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_release_checks(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_value(NativeVnRuntimeProvider::default().release_checks())
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_editor_metadata(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_value(NativeVnRuntimeProvider::default().editor_metadata())
}

#[cfg(feature = "ffi")]
fn ffi_json<T, U>(payload: RVec<u8>, f: impl FnOnce(T) -> U) -> FfiRuntimeProviderResult
where
    T: serde::de::DeserializeOwned,
    U: serde::Serialize,
{
    match serde_json::from_slice::<T>(payload.as_slice()) {
        Ok(request) => ffi_json_value(f(request)),
        Err(err) => ffi_error("ASTRA_NATIVE_VN_FFI_DECODE", err.to_string()),
    }
}

#[cfg(feature = "ffi")]
fn ffi_json_result<T, U, E>(
    payload: RVec<u8>,
    f: impl FnOnce(T) -> Result<U, E>,
) -> FfiRuntimeProviderResult
where
    T: serde::de::DeserializeOwned,
    U: serde::Serialize,
    E: std::fmt::Display,
{
    match serde_json::from_slice::<T>(payload.as_slice()) {
        Ok(request) => match f(request) {
            Ok(value) => ffi_json_value(value),
            Err(err) => ffi_error("ASTRA_NATIVE_VN_FFI_CALL", err.to_string()),
        },
        Err(err) => ffi_error("ASTRA_NATIVE_VN_FFI_DECODE", err.to_string()),
    }
}

#[cfg(feature = "ffi")]
fn ffi_instance_json<T, U>(
    payload: RVec<u8>,
    f: impl FnOnce(&mut NativeVnRuntimeProvider, T) -> Result<U, CoreVnError>,
) -> FfiRuntimeProviderResult
where
    T: serde::de::DeserializeOwned,
    U: serde::Serialize,
{
    ffi_json_result(payload, |call: RuntimeProviderCall| {
        let request = serde_json::from_slice::<T>(&call.payload)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let mut instances = ffi_instances().lock().map_err(|_| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_INSTANCE_LOCK",
                "provider instance registry lock is poisoned",
            )
        })?;
        let provider = instances.get_mut(&call.instance_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_INSTANCE_MISSING",
                "provider instance is not active",
            )
        })?;
        f(provider, request)
    })
}

#[cfg(feature = "ffi")]
fn ffi_json_value(value: impl serde::Serialize) -> FfiRuntimeProviderResult {
    match serde_json::to_vec(&value) {
        Ok(payload) => FfiRuntimeProviderResult {
            ok: true,
            payload: RVec::from(payload),
            diagnostics: RVec::new(),
        },
        Err(err) => ffi_error("ASTRA_NATIVE_VN_FFI_ENCODE", err.to_string()),
    }
}

#[cfg(feature = "ffi")]
fn ffi_error(code: &'static str, message: String) -> FfiRuntimeProviderResult {
    FfiRuntimeProviderResult {
        ok: false,
        payload: RVec::new(),
        diagnostics: RVec::from(vec![RString::from(format!("{code}: {message}"))]),
    }
}
