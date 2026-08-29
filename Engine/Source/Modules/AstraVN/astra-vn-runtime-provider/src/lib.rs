//! Native AstraVN gameplay runtime provider and ABI-safe FFI adapter.
//! TODO(harness-merge): 单文件 3372 行违反 Charter `>600 拆模块`，已规划拆
//! `factory.rs`（Factory/Session wrapper）`session.rs`（NativeVnSession/VnStepAction）
//! `provider.rs`（NativeVnRuntimeProvider impl）`command.rs`（command 转换）
//! `ffi.rs`（FfiProviderInstance 1200+ 行）；本轮仅库内Headless/指纹/Hash优先，拆分延至下次 PR 避免与 `973191ede` 合并冲突。

#[cfg(feature = "ffi")]
use std::sync::OnceLock;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[cfg(feature = "ffi")]
use abi_stable::std_types::{RString, RVec};
use astra_core::SchemaVersion;
use astra_plugin::{ProductRuntimeProvider, ProductRuntimeProviderFactory, ProductRuntimeSession};
#[cfg(feature = "ffi")]
use astra_plugin_abi::{
    FfiRuntimeAudioBus, FfiRuntimeAudioCommand, FfiRuntimeAudioCue, FfiRuntimeAudioEncoding,
    FfiRuntimeAudioPacket, FfiRuntimeAudioSampleFormat, FfiRuntimeAudioSync,
    FfiRuntimeBlackboardMutation, FfiRuntimeBlendMode, FfiRuntimeDirtySection,
    FfiRuntimeEditorMetadataResult, FfiRuntimeEvent, FfiRuntimeInstanceRequest,
    FfiRuntimeIntegrityMode, FfiRuntimeLiveOutput, FfiRuntimeOpenRequest, FfiRuntimeOpenResult,
    FfiRuntimePackageSectionsResult, FfiRuntimePcmBuffer, FfiRuntimePrepareRequest,
    FfiRuntimeProbeRequest, FfiRuntimeProviderRegistration, FfiRuntimeReleaseChecksResult,
    FfiRuntimeReportResult, FfiRuntimeResourceScene, FfiRuntimeResourceTexture,
    FfiRuntimeRestoreRequest, FfiRuntimeRestoreResult, FfiRuntimeSaveRequest, FfiRuntimeSaveResult,
    FfiRuntimeSceneResourceOperation, FfiRuntimeSceneTextureCreate, FfiRuntimeSceneTextureUpdate,
    FfiRuntimeScissor, FfiRuntimeSection, FfiRuntimeSectionCodec, FfiRuntimeSectionResult,
    FfiRuntimeShutdownRequest, FfiRuntimeShutdownResult, FfiRuntimeStepMode, FfiRuntimeStepRequest,
    FfiRuntimeStepResult, FfiRuntimeTextLease, FfiRuntimeTextPresentation, FfiRuntimeTextRegion,
    FfiRuntimeTextureFilter, FfiRuntimeTextureFormat, FfiRuntimeVertex, FfiRuntimeVideoCommand,
    FfiRuntimeVideoMode, FfiRuntimeWait, FfiRuntimeWaitKind, PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA,
    PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ReleaseCheckDescriptor, RuntimeEditorMetadata,
    RuntimeExecutorKind, RuntimeLiveAudioBus, RuntimeLiveAudioCue, RuntimeLiveAudioSync,
    RuntimeLiveCoverage, RuntimeOpenReport, RuntimeOpenRequest, RuntimePackageSectionPlan,
    RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport, RuntimeProbeRequest,
    RuntimeProviderInstanceReport, RuntimeRestoreReport, RuntimeRestoreRequest, RuntimeSaveRequest,
    RuntimeSaveSections, RuntimeSectionCodec, RuntimeSectionPayload, RuntimeSectionRef,
    RuntimeShutdownReport, RuntimeStepInput, RuntimeStepMode, RuntimeStepOutput,
    RuntimeTickIntegrityMode, GAME_RUNTIME_PROVIDER_SLOT, NATIVE_VN_PROVIDER_ID,
    NATIVE_VN_RUNTIME_ID, RUNTIME_EDITOR_METADATA_SCHEMA,
};
#[cfg(feature = "ffi")]
use astra_plugin_abi::{
    RuntimeExecutorConfig, RuntimeLiveAudioCommand, RuntimeLiveAudioEncoding,
    RuntimeLiveAudioSampleFormat, RuntimeLiveBlackboardMutation, RuntimeLiveBlendMode,
    RuntimeLiveDirtySection, RuntimeLivePcmBuffer, RuntimeLiveSceneResourceOperation,
    RuntimeLiveTextureFormat, RuntimeLiveVideoCommandKind, RuntimeLiveVideoMode,
    RuntimeLiveWaitKind,
};
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, ActorId, BlackboardValue, ComponentId, ComponentRecord,
    DeterministicActionContext, EventPayload, GuardExpr, OrderedTickIngress, PackageHandle,
    PlayerInput, RuntimeAction, RuntimeComponentPayload, RuntimeConfig, RuntimeError,
    RuntimeSnapshot, RuntimeWorld, SaveBlob, SaveRequest, StateDefinition, StateMachineDefinition,
    TickIngress, TickInput, TickIntegrityMode, TickRequest, TransitionDefinition,
};
pub use astra_vn_core::*;
use astra_vn_core::{
    CompiledStory as CoreCompiledStory, VnError as CoreVnError,
    VnPlayerCommand as CoreVnPlayerCommand, VnRuntime as CoreVnRuntime,
    VnRuntimeIndex as CoreVnRuntimeIndex,
};
pub use astra_vn_editor::*;
pub use astra_vn_package::*;
pub use astra_vn_save::*;

#[derive(Default)]
pub struct NativeVnRuntimeProvider {
    instance_id: Option<astra_plugin_abi::ProviderInstanceId>,
    sessions: BTreeMap<String, NativeVnSession>,
}

#[derive(Default)]
pub struct NativeVnRuntimeProviderFactory {
    instance_id: Mutex<Option<astra_plugin_abi::ProviderInstanceId>>,
    active_sessions: Arc<AtomicUsize>,
}

struct NativeVnProviderSession {
    provider: NativeVnRuntimeProvider,
    session_id: GameRuntimeSessionId,
    active_sessions: Arc<AtomicUsize>,
    active: bool,
}

impl Drop for NativeVnProviderSession {
    fn drop(&mut self) {
        if self.active {
            self.active_sessions.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl ProductRuntimeProviderFactory for NativeVnRuntimeProviderFactory {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(NativeVnRuntimeProvider::descriptor())
    }

    fn create_instance(
        &self,
        instance_id: astra_plugin_abi::ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_NATIVE_VN_FACTORY_LOCK_POISONED".to_string())?;
        if current.is_some() {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_DUPLICATE: provider instance already created".into(),
            );
        }
        *current = Some(instance_id.clone());
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &self,
        instance_id: astra_plugin_abi::ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if self.active_sessions.load(Ordering::Acquire) != 0 {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_ACTIVE_SESSIONS: provider has active sessions".into(),
            );
        }
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_NATIVE_VN_FACTORY_LOCK_POISONED".to_string())?;
        if current.as_ref() != Some(&instance_id) {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_MISMATCH: provider instance id does not match".into(),
            );
        }
        *current = None;
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Ok(NativeVnRuntimeProvider::default().prepare(request))
    }

    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Ok(NativeVnRuntimeProvider::default().probe(request))
    }

    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String> {
        if self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_NATIVE_VN_FACTORY_LOCK_POISONED".to_string())?
            .is_none()
        {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_MISSING: provider instance is not created".into(),
            );
        }
        let mut provider = NativeVnRuntimeProvider::default();
        let report = ProductRuntimeProvider::open(&mut provider, request)?;
        self.active_sessions.fetch_add(1, Ordering::AcqRel);
        Ok((
            report.clone(),
            Box::new(NativeVnProviderSession {
                provider,
                session_id: report.session_id,
                active_sessions: Arc::clone(&self.active_sessions),
                active: true,
            }),
        ))
    }
}

impl ProductRuntimeSession for NativeVnProviderSession {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        if input.session_id != self.session_id {
            return Err("ASTRA_NATIVE_VN_SESSION_MISMATCH: step session id does not match".into());
        }
        ProductRuntimeProvider::step(&mut self.provider, input)
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        if request.session_id != self.session_id {
            return Err("ASTRA_NATIVE_VN_SESSION_MISMATCH: save session id does not match".into());
        }
        ProductRuntimeProvider::save(&mut self.provider, request)
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        if request.session_id != self.session_id {
            return Err(
                "ASTRA_NATIVE_VN_SESSION_MISMATCH: restore session id does not match".into(),
            );
        }
        ProductRuntimeProvider::restore(&mut self.provider, request)
    }

    fn shutdown(
        mut self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        if session_id != self.session_id {
            return Err(
                "ASTRA_NATIVE_VN_SESSION_MISMATCH: shutdown session id does not match".into(),
            );
        }
        let report = ProductRuntimeProvider::shutdown(&mut self.provider, session_id)?;
        self.active_sessions.fetch_sub(1, Ordering::AcqRel);
        self.active = false;
        Ok(report)
    }
}

impl ProductRuntimeProvider for NativeVnRuntimeProvider {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(NativeVnRuntimeProvider::descriptor())
    }

    fn create_instance(
        &mut self,
        instance_id: astra_plugin_abi::ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if self.instance_id.is_some() {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_DUPLICATE: provider instance already created".into(),
            );
        }
        self.instance_id = Some(instance_id.clone());
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &mut self,
        instance_id: astra_plugin_abi::ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if !self.sessions.is_empty() {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_ACTIVE_SESSIONS: provider has active sessions".into(),
            );
        }
        if self.instance_id.as_ref() != Some(&instance_id) {
            return Err(
                "ASTRA_NATIVE_VN_INSTANCE_MISMATCH: provider instance id does not match".into(),
            );
        }
        self.instance_id = None;
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&mut self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Ok(NativeVnRuntimeProvider::prepare(self, request))
    }

    fn probe(&mut self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Ok(NativeVnRuntimeProvider::probe(self, request))
    }

    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        let compiled_section =
            required_restore_section(&request.sections, "vn.story", "astra.vn.story")
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

struct NativeVnSession {
    world: RuntimeWorld,
    owner: ActorId,
    compiled: Arc<CoreCompiledStory>,
    runtime_index: Arc<CoreVnRuntimeIndex>,
    state: VnRuntimeState,
    pending_control: Arc<Mutex<Option<PreparedVnControl>>>,
    control_result: Arc<Mutex<Option<astra_runtime::AwaitTokenId>>>,
    step_complexity: Option<VnStepComplexityMetrics>,
}

struct VnStepAction {
    pending_control: Arc<Mutex<Option<PreparedVnControl>>>,
    control_result: Arc<Mutex<Option<astra_runtime::AwaitTokenId>>>,
}

#[derive(Clone)]
struct PreparedVnControl {
    events: Vec<(String, String)>,
    create_wait: Option<astra_runtime::AwaitKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VnRuntimeStorageMetrics {
    pub schema: String,
    pub backlog_count: usize,
    pub history_chunk_count: usize,
    pub hot_state_bytes: usize,
    pub tail_chunk_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VnStepComplexityMetrics {
    pub schema: String,
    pub previous_backlog_count: usize,
    pub appended_backlog_entries: usize,
    pub state_cache_hit: bool,
    pub materialized_history_entries: usize,
    pub history_component_writes: usize,
    pub encoded_hot_state_bytes: usize,
    pub mutation_journal_entries: usize,
}

fn materialize_session_state(session: &NativeVnSession) -> Result<VnRuntimeState, CoreVnError> {
    Ok(session.state.clone())
}

fn materialized_save_snapshot(session: &NativeVnSession) -> Result<RuntimeSnapshot, CoreVnError> {
    let state = materialize_session_state(session)?;
    let mut snapshot = session.world.snapshot();
    let mut id_probe = snapshot.id_source.clone();
    let component_id = loop {
        let candidate = ComponentId(id_probe.next_id());
        if snapshot.actors.component(candidate).is_none() {
            break candidate;
        }
    };
    let payload = RuntimeComponentPayload::typed(
        VN_RUNTIME_STATE_SCHEMA,
        SchemaVersion::new(VN_RUNTIME_STATE_SCHEMA_MAJOR, 0, 0),
        state,
    );
    if !snapshot.actors.attach_component(ComponentRecord {
        component_id,
        actor_id: session.owner,
        payload,
    }) {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_SAVE_OWNER_MISSING",
            "VN save materialization owner is missing from the Runtime snapshot",
        ));
    }
    Ok(snapshot)
}

fn consume_materialized_restore_state(
    session: &mut NativeVnSession,
) -> Result<VnRuntimeState, CoreVnError> {
    let schema = VN_RUNTIME_STATE_SCHEMA.to_string();
    let mut candidates = session
        .world
        .snapshot()
        .actors
        .component_ids_for_actor_schema(session.owner, &schema);
    if candidates.len() != 1 {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_STATE_SET",
            "Runtime v3 restore must contain exactly one materialized VN state",
        ));
    }
    let component_id = candidates.remove(0);
    let state: VnRuntimeState = session
        .world
        .read_component(component_id)
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    if !session.world.detach_component(component_id) {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_STATE_DETACH",
            "materialized VN restore state could not be removed after validation",
        ));
    }
    Ok(state)
}

fn replace_session_state(
    session: &mut NativeVnSession,
    state: VnRuntimeState,
) -> Result<(), CoreVnError> {
    let checkpoint = session.world.snapshot();
    let cached = session.state.clone();
    match replace_session_state_inner(session, state) {
        Ok(()) => Ok(()),
        Err(error) => {
            session.world.restore_snapshot(checkpoint);
            session.state = cached;
            Err(error)
        }
    }
}

fn replace_session_state_inner(
    session: &mut NativeVnSession,
    state: VnRuntimeState,
) -> Result<(), CoreVnError> {
    CoreVnRuntime::from_shared_state_indexed(
        Arc::clone(&session.compiled),
        Arc::clone(&session.runtime_index),
        state.clone(),
    )?;
    session.state = state;
    Ok(())
}

impl NativeVnRuntimeProvider {
    pub fn slot() -> &'static str {
        GAME_RUNTIME_PROVIDER_SLOT
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn storage_metrics(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<VnRuntimeStorageMetrics, CoreVnError> {
        let session = self.session(session_id)?;
        let backlog_count = materialize_session_state(session)?.backlog.len();
        Ok(VnRuntimeStorageMetrics {
            schema: "astra.vn.runtime_storage_metrics.v4".to_string(),
            backlog_count,
            history_chunk_count: 0,
            hot_state_bytes: 0,
            tail_chunk_bytes: 0,
        })
    }

    pub fn step_complexity_metrics(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<VnStepComplexityMetrics, CoreVnError> {
        self.session(session_id)?
            .step_complexity
            .clone()
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_STEP_COMPLEXITY_MISSING",
                    "VN session has not completed a measured runtime step",
                )
            })
    }

    pub fn descriptor() -> ProductRuntimeDescriptor {
        ProductRuntimeDescriptor {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            product_kind: "visual_novel".to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            presentation_lane: astra_plugin_abi::RuntimePresentationLane::Scene2D,
            supported_targets: vec!["game".to_string()],
            capabilities: vec!["runtime.native_vn".to_string()],
            package_sections: native_vn_package_sections(),
            release_checks: native_vn_release_check_ids(),
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
            .all(|section| section != "vn.story")
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
        compiled: impl Into<CoreCompiledStory>,
        config: VnRunConfig,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, CoreVnError> {
        let compiled = Arc::new(compiled.into());
        let runtime_index = Arc::new(CoreVnRuntimeIndex::build(&compiled)?);
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
        let initial_runtime = CoreVnRuntime::new_shared_indexed(
            Arc::clone(&compiled),
            Arc::clone(&runtime_index),
            config,
        )?;
        let integrity_mode = match request.integrity_mode {
            RuntimeTickIntegrityMode::Shipping => TickIntegrityMode::Shipping,
            RuntimeTickIntegrityMode::Evidence => TickIntegrityMode::Evidence,
        };
        let mut world = RuntimeWorld::create_with_integrity(
            RuntimeConfig {
                seed: request.seed,
                required_slots: Vec::new(),
            },
            PackageHandle {
                package_id: request.package_hash.clone(),
                target: request.target_id.clone(),
                ..PackageHandle::default()
            },
            integrity_mode,
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?;
        request
            .executor
            .validate()
            .map_err(|message| CoreVnError::diagnostic("ASTRA_RUNTIME_EXECUTOR_CONFIG", message))?;
        world
            .set_machine_worker_count(match request.executor.kind {
                RuntimeExecutorKind::Serial => 1,
                RuntimeExecutorKind::Parallel => usize::from(request.executor.worker_count),
            })
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let owner = world.create_actor("astra.vn.runtime", vec!["gameplay_runtime".to_string()]);
        let initial_state = initial_runtime.state().clone();
        world
            .attach_component(owner, "astra.vn.policy_state.v1", &VnPolicyState::default())
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let pending_control = Arc::new(Mutex::new(None));
        let control_result = Arc::new(Mutex::new(None));
        world
            .register_action(
                NATIVE_VN_PROVIDER_ID,
                VnStepAction {
                    pending_control: Arc::clone(&pending_control),
                    control_result: Arc::clone(&control_result),
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
                owner,
                compiled,
                runtime_index,
                state: initial_state,
                pending_control,
                control_result,
                step_complexity: None,
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
        let command = match input.action.as_str() {
            "command" => {
                return Err(CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_COMMAND_DISPATCH",
                    "generic command input is not part of the typed runtime ABI",
                ));
            }
            "launch_default" => {
                let session = self.session(&input.session_id)?;
                let state = materialize_session_state(session)?;
                CoreVnRuntime::from_shared_state_indexed(
                    Arc::clone(&session.compiled),
                    Arc::clone(&session.runtime_index),
                    state,
                )?
                .default_launch_command()
                .ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                        "compiled story has no launchable state",
                    )
                })?
            }
            _ => runtime_command_from_input(&input)?,
        };
        self.apply_command_at_step(
            input.session_id,
            command,
            input.fixed_step,
            input.delta_ns,
            input.session_seed,
            input.mode,
        )
    }

    fn apply_command_at_step(
        &mut self,
        session_id: GameRuntimeSessionId,
        command: CoreVnPlayerCommand,
        fixed_step: u64,
        delta_ns: u64,
        session_seed: u64,
        mode: RuntimeStepMode,
    ) -> Result<RuntimeStepOutput, CoreVnError> {
        let session = self.session_mut(&session_id)?;
        let event_kind = vn_event_kind(&command).to_string();
        let previous_state = session.state.clone();
        let pending_wait = previous_state.pending_wait.clone();
        let reading_mode = previous_state.system.reading_mode;
        let previous_backlog_count = previous_state.backlog.len();
        let previous_wait = previous_state.pending_wait.clone();
        let (mut next_state, mut pending_output) = astra_vn_core::reduce_vn_step_indexed_pending(
            Arc::clone(&session.compiled),
            Arc::clone(&session.runtime_index),
            previous_state,
            command.clone(),
        )?;
        if next_state.backlog.len() < previous_backlog_count {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_TRUNCATION",
                "VN reducer attempted to truncate append-only backlog history",
            ));
        }
        let next_revision = next_state.revision.checked_add(1).ok_or_else(|| {
            CoreVnError::message("VN state revision exhausted its deterministic range")
        })?;
        next_state.revision = next_revision;
        let create_wait = if next_state.pending_wait != previous_wait {
            next_state.pending_wait.as_ref().and_then(|wait| {
                let has_runtime_await_id = wait
                    .await_id
                    .as_deref()
                    .is_some_and(|await_id| astra_core::StableId::parse(await_id).is_ok());
                (!has_runtime_await_id)
                    .then(|| astra_runtime::AwaitKind::Custom(format!("vn.{:?}", wait.kind)))
            })
        } else {
            None
        };
        if let Some(wait) = next_state.pending_wait.clone() {
            pending_output.set_wait(wait);
        }
        let control = PreparedVnControl {
            events: pending_output
                .events()
                .iter()
                .map(|event| (event.kind.clone(), event.id.clone()))
                .collect(),
            create_wait,
        };
        *session
            .pending_control
            .lock()
            .map_err(|_| CoreVnError::message("VN control lock is poisoned"))? = Some(control);
        *session
            .control_result
            .lock()
            .map_err(|_| CoreVnError::message("VN control result lock is poisoned"))? = None;
        let mut ingress = Vec::new();
        if command_resolves_wait(
            &command,
            pending_wait.as_ref().map(|wait| wait.kind),
            reading_mode,
            &session.compiled,
        ) {
            let await_id = pending_wait
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
            ingress.push(OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::AwaitCompletion(astra_runtime::AwaitResult {
                    token_id,
                    sequence: fixed_step,
                    completed_at_step: fixed_step,
                    payload: EventPayload::new("await.resolved"),
                }),
            });
        }
        ingress.push(OrderedTickIngress {
            sequence: ingress.len() as u64 + 1,
            payload: TickIngress::PlayerInput(PlayerInput {
                kind: event_kind.clone(),
                payload: EventPayload {
                    kind: event_kind,
                    data: command_event_data(&command),
                },
            }),
        });
        let timing = TickInput {
            fixed_step,
            delta_ns,
            seed: session_seed,
        };
        let request = match mode {
            RuntimeStepMode::Live => TickRequest::live(timing, ingress),
            RuntimeStepMode::RestoreContinuation => {
                TickRequest::restore_continuation(timing, ingress)
            }
        };
        let tick = session
            .world
            .tick(request)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        if let Some(diagnostic) = tick.diagnostics.first() {
            return Err(CoreVnError::diagnostic(
                diagnostic.code.clone(),
                diagnostic.message.clone(),
            ));
        }
        if let Some(token_id) = session
            .control_result
            .lock()
            .map_err(|_| CoreVnError::message("VN control result lock is poisoned"))?
            .take()
        {
            let wait = next_state.pending_wait.as_mut().ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_AWAIT_STATE_MISSING",
                    "Runtime created an await token without VN wait state",
                )
            })?;
            let runtime_await_id = token_id.0.to_string();
            wait.await_id = Some(runtime_await_id.clone());
            pending_output.set_wait(wait.clone());
            pending_output.push_await(runtime_await_id);
        }
        if next_state.pending_wait != previous_wait
            && next_state.pending_wait.as_ref().is_some_and(|wait| {
                wait.await_id
                    .as_deref()
                    .is_none_or(|id| astra_core::StableId::parse(id).is_err())
            })
        {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_AWAIT_ID_MISSING",
                "VN wait was not bound to a Runtime AwaitToken",
            ));
        }
        let appended_backlog_entries = next_state.backlog.len() - previous_backlog_count;
        let output = pending_output.finalize(next_revision);
        let mutation_journal_entries = output.mutations.len();
        session.state = next_state;
        session.step_complexity = Some(VnStepComplexityMetrics {
            schema: "astra.vn.step_complexity_metrics.v3".to_string(),
            previous_backlog_count,
            appended_backlog_entries,
            state_cache_hit: true,
            materialized_history_entries: 0,
            history_component_writes: 0,
            encoded_hot_state_bytes: 0,
            mutation_journal_entries,
        });
        let live_vn_state = runtime_live_vn_state(&session.state);
        let presentation_count = output.presentation.len();
        let audio_command_count = output.audio.len();
        let mut presentations = Vec::with_capacity(presentation_count);
        let mut audio_cues = Vec::with_capacity(audio_command_count);
        let mut audio = output.audio.into_iter();
        for (presentation_index, command) in output.presentation.into_iter().enumerate() {
            let sequence = presentation_index
                .checked_add(1)
                .and_then(|index| u64::try_from(index).ok())
                .ok_or_else(|| CoreVnError::message("VN presentation sequence overflow"))?;
            let has_audio = matches!(&command, PresentationCommand::Stage(StageCommand::Audio(_)));
            if has_audio {
                let audio_command = audio.next().ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_AUDIO_ORDER_MISSING",
                        "typed audio presentation has no matching audio output",
                    )
                })?;
                audio_cues.push(runtime_live_audio_cue(sequence, &audio_command));
            }
            presentations.push(runtime_live_presentation(sequence, command));
        }
        if audio.next().is_some() {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_AUDIO_ORDER_EXTRA",
                "audio output has no matching typed presentation command",
            ));
        }
        let timeline = output
            .timeline_tasks
            .into_iter()
            .map(|task| astra_plugin_abi::RuntimeLiveTimelineTask {
                command_id: task.command_id,
                command: runtime_live_timeline(task.command),
            })
            .collect();
        let vn_step = astra_plugin_abi::RuntimeLiveVnStep {
            coverage_reached: output.coverage.reached.into_iter().collect(),
        };
        Ok(RuntimeStepOutput {
            session_id,
            status: if presentation_count == 0 {
                "idle".to_string()
            } else {
                "blocked".to_string()
            },
            live: astra_plugin_abi::RuntimeLiveOutput {
                state_revision: fixed_step,
                coverage: RuntimeLiveCoverage {
                    presentation_commands: presentation_count as u64,
                    audio_commands: audio_command_count as u64,
                    ..RuntimeLiveCoverage::default()
                },
                audio_cues,
                presentations,
                timeline,
                vn_state: Some(live_vn_state),
                vn_step: Some(vn_step),
                ..astra_plugin_abi::RuntimeLiveOutput::default()
            },
            diagnostics: Vec::new(),
        })
    }

    pub fn default_launch_command(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<CoreVnPlayerCommand, CoreVnError> {
        let session = self.session(session_id)?;
        let state = materialize_session_state(session)?;
        CoreVnRuntime::from_shared_state_indexed(
            Arc::clone(&session.compiled),
            Arc::clone(&session.runtime_index),
            state,
        )?
        .default_launch_command()
        .ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                "compiled story has no launchable state",
            )
        })
    }

    pub fn state(&self, session_id: &GameRuntimeSessionId) -> Result<VnRuntimeState, CoreVnError> {
        materialize_session_state(self.session(session_id)?)
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
        Ok(VnSaveBlob {
            schema: "astra.vn.save_slot.v2".to_string(),
            slot: slot.into(),
            state,
        })
    }

    pub fn load_slot(
        &mut self,
        session_id: &GameRuntimeSessionId,
        save: VnSaveBlob,
    ) -> Result<(), CoreVnError> {
        if save.schema != "astra.vn.save_slot.v2" || save.state.schema != VN_RUNTIME_STATE_SCHEMA {
            return Err(CoreVnError::diagnostic(
                "ASTRA_VN_SAVE_SCHEMA",
                "AstraVN save slot schema is invalid",
            ));
        }
        let session = self.session_mut(session_id)?;
        replace_session_state(session, save.state)
    }

    pub fn save(&self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, CoreVnError> {
        let session = self.session(&request.session_id)?;
        let save = astra_runtime::write_runtime_save(
            materialized_save_snapshot(session)?,
            SaveRequest::default(),
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?;
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: vec![RuntimeSectionPayload {
                section_id: "runtime.world".to_string(),
                schema: "astra.runtime.save_blob.v4".to_string(),
                version: SchemaVersion::new(4, 0, 0),
                codec: RuntimeSectionCodec::Raw,
                hash: astra_core::Hash256::from_sha256(&save.0),
                bytes: save.0,
            }],
            diagnostics: Vec::new(),
        })
    }

    pub fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, CoreVnError> {
        if request.sections.len() != 1 {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_RESTORE_SECTION_SET",
                "restore requires exactly one authoritative runtime.world section",
            ));
        }
        let runtime_section = required_restore_section_with_codec(
            &request.sections,
            "runtime.world",
            "astra.runtime.save_blob.v4",
            RuntimeSectionCodec::Raw,
        )?;
        let session = self.session_mut(&request.session_id)?;
        session
            .world
            .load(SaveBlob(runtime_section.bytes.clone()))
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let state = consume_materialized_restore_state(session)?;
        session.state = state;
        let snapshot = session.world.snapshot();
        Ok(RuntimeRestoreReport {
            session_id: request.session_id,
            restored_fixed_step: snapshot.step,
            session_seed: snapshot.config.seed,
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
            abi_version: PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
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
            open_session: ffi_open,
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
    input: &RuntimeStepInput,
) -> Result<CoreVnPlayerCommand, CoreVnError> {
    match input.action.as_str() {
        "command" => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_COMMAND_DISPATCH",
            "generic command input is not part of the typed runtime ABI",
        )),
        "launch_default" => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_LAUNCH_DISPATCH",
            "default launch must be resolved with authoritative session state",
        )),
        "advance" => Ok(CoreVnPlayerCommand::Advance),
        "choose" => Ok(CoreVnPlayerCommand::Choose {
            option_id: required_input_argument(input, "choose", "option id")?,
        }),
        "open_system" => Ok(CoreVnPlayerCommand::OpenSystem {
            page: required_page(required_input_argument(input, "open_system", "page")?)?,
        }),
        "switch_system_page" => Ok(CoreVnPlayerCommand::SwitchSystemPage {
            page: required_page(required_input_argument(
                input,
                "switch_system_page",
                "page",
            )?)?,
        }),
        "replay_voice" => Ok(CoreVnPlayerCommand::ReplayVoice {
            voice: required_input_argument(input, "replay_voice", "voice")?,
        }),
        "set_auto" => Ok(CoreVnPlayerCommand::SetAuto {
            enabled: required_input_flag(input, "set_auto")?,
        }),
        "set_skip" => Ok(CoreVnPlayerCommand::SetSkip {
            mode: required_skip_mode(required_input_argument(input, "set_skip", "mode")?)?,
        }),
        "set_reading_mode" => Ok(CoreVnPlayerCommand::SetReadingMode {
            mode: required_reading_mode(required_input_argument(
                input,
                "set_reading_mode",
                "mode",
            )?)?,
        }),
        "set_audio_enabled" => Ok(CoreVnPlayerCommand::SetAudioEnabled {
            enabled: required_input_flag(input, "set_audio_enabled")?,
        }),
        "invoke_system_action" => Ok(CoreVnPlayerCommand::InvokeSystemAction {
            action_id: required_input_argument(input, "invoke_system_action", "action id")?,
        }),
        "set_config" => Ok(CoreVnPlayerCommand::SetConfig {
            key: required_input_argument(input, "set_config", "key")?,
            value: input.auxiliary.clone().ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_ACTION_ARGUMENT",
                    "set_config action is missing its value argument",
                )
            })?,
        }),
        "start_replay" => Ok(CoreVnPlayerCommand::StartReplay {
            replay_id: required_input_argument(input, "start_replay", "replay id")?,
        }),
        "preview_gallery" => Ok(CoreVnPlayerCommand::PreviewGallery {
            item_id: required_input_argument(input, "preview_gallery", "gallery item id")?,
        }),
        "jump_route" => Ok(CoreVnPlayerCommand::JumpRoute {
            node_id: required_input_argument(input, "jump_route", "route node id")?,
        }),
        "jump_backlog" => Ok(CoreVnPlayerCommand::JumpBacklog {
            command_id: required_input_argument(input, "jump_backlog", "backlog command id")?,
        }),
        "submit_text" => Ok(CoreVnPlayerCommand::SubmitText {
            input_id: required_input_argument(input, "submit_text", "input id")?,
            value: input.auxiliary.clone().ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_ACTION_ARGUMENT",
                    "submit_text action is missing its value argument",
                )
            })?,
        }),
        "unlock" => Ok(CoreVnPlayerCommand::Unlock {
            kind: required_unlock_kind(required_input_argument(input, "unlock", "unlock kind")?)?,
            id: input.auxiliary.clone().ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_ACTION_ARGUMENT",
                    "unlock action is missing its item id argument",
                )
            })?,
        }),
        "complete_wait" => Ok(CoreVnPlayerCommand::CompleteWait {
            fence: required_input_argument(input, "complete_wait", "fence")?,
        }),
        "system_return" => Ok(CoreVnPlayerCommand::ReturnSystem),
        other => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_UNKNOWN",
            format!("runtime action {other} is not supported"),
        )),
    }
}

fn required_input_argument(
    input: &RuntimeStepInput,
    action: &str,
    name: &str,
) -> Result<String, CoreVnError> {
    input.argument.clone().ok_or_else(|| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_ARGUMENT",
            format!("{action} action is missing its typed {name} argument"),
        )
    })
}

fn required_input_flag(input: &RuntimeStepInput, action: &str) -> Result<bool, CoreVnError> {
    input.flag.ok_or_else(|| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_FLAG",
            format!("{action} action is missing its typed boolean flag"),
        )
    })
}

fn required_page(value: String) -> Result<SystemPageKind, CoreVnError> {
    let page = SystemPageKind::parse(&value);
    if page == SystemPageKind::Unknown {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_PAGE",
            format!("unknown system page {value}"),
        ));
    }
    Ok(page)
}

fn required_skip_mode(value: String) -> Result<SkipMode, CoreVnError> {
    match value.as_str() {
        "none" => Ok(SkipMode::None),
        "read" => Ok(SkipMode::Read),
        "all" => Ok(SkipMode::All),
        _ => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_SKIP_MODE",
            format!("unknown skip mode {value}"),
        )),
    }
}

fn required_reading_mode(value: String) -> Result<ReadingMode, CoreVnError> {
    match value.as_str() {
        "hidden" => Ok(ReadingMode::Hidden),
        "manual" => Ok(ReadingMode::Manual),
        "fast_forward" => Ok(ReadingMode::FastForward),
        _ => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_READING_MODE",
            format!("unknown reading mode {value}"),
        )),
    }
}

fn required_unlock_kind(value: String) -> Result<SystemUnlockKind, CoreVnError> {
    match value.as_str() {
        "gallery" => Ok(SystemUnlockKind::Gallery),
        "replay" => Ok(SystemUnlockKind::Replay),
        _ => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_UNLOCK_KIND",
            format!("unknown unlock kind {value}"),
        )),
    }
}

fn command_event_data(command: &CoreVnPlayerCommand) -> BTreeMap<String, BlackboardValue> {
    let mut data = BTreeMap::new();
    let string = |value: &str| BlackboardValue::String(value.to_string());
    match command {
        CoreVnPlayerCommand::Launch { story_id, state_id } => {
            data.insert("story_id".to_string(), string(story_id));
            data.insert("state_id".to_string(), string(state_id));
        }
        CoreVnPlayerCommand::Choose { option_id } => {
            data.insert("option_id".to_string(), string(option_id));
        }
        CoreVnPlayerCommand::OpenSystem { page }
        | CoreVnPlayerCommand::SwitchSystemPage { page } => {
            data.insert("page".to_string(), string(page_name(*page)));
        }
        CoreVnPlayerCommand::ReplayVoice { voice } => {
            data.insert("voice".to_string(), string(voice));
        }
        CoreVnPlayerCommand::SetAuto { enabled }
        | CoreVnPlayerCommand::SetAudioEnabled { enabled } => {
            data.insert("enabled".to_string(), BlackboardValue::Bool(*enabled));
        }
        CoreVnPlayerCommand::SetSkip { mode } => {
            data.insert("mode".to_string(), string(skip_mode_name(*mode)));
        }
        CoreVnPlayerCommand::SetReadingMode { mode } => {
            data.insert("mode".to_string(), string(reading_mode_name(*mode)));
        }
        CoreVnPlayerCommand::InvokeSystemAction { action_id } => {
            data.insert("action_id".to_string(), string(action_id));
        }
        CoreVnPlayerCommand::SetConfig { key, value } => {
            data.insert("key".to_string(), string(key));
            data.insert("value".to_string(), string(value));
        }
        CoreVnPlayerCommand::StartReplay { replay_id } => {
            data.insert("replay_id".to_string(), string(replay_id));
        }
        CoreVnPlayerCommand::PreviewGallery { item_id } => {
            data.insert("item_id".to_string(), string(item_id));
        }
        CoreVnPlayerCommand::JumpRoute { node_id } => {
            data.insert("node_id".to_string(), string(node_id));
        }
        CoreVnPlayerCommand::JumpBacklog { command_id } => {
            data.insert("command_id".to_string(), string(command_id));
        }
        CoreVnPlayerCommand::SubmitText { input_id, value } => {
            data.insert("input_id".to_string(), string(input_id));
            data.insert("value".to_string(), string(value));
        }
        CoreVnPlayerCommand::Unlock { kind, id } => {
            data.insert("kind".to_string(), string(unlock_kind_name(*kind)));
            data.insert("id".to_string(), string(id));
        }
        CoreVnPlayerCommand::CompleteWait { fence } => {
            data.insert("fence".to_string(), string(fence));
        }
        CoreVnPlayerCommand::Advance | CoreVnPlayerCommand::ReturnSystem => {}
    }
    data
}

fn page_name(page: SystemPageKind) -> &'static str {
    match page {
        SystemPageKind::Title => "title",
        SystemPageKind::QuickPanel => "quick_panel",
        SystemPageKind::Save => "save",
        SystemPageKind::Load => "load",
        SystemPageKind::Config => "config",
        SystemPageKind::Gallery => "gallery",
        SystemPageKind::Replay => "replay",
        SystemPageKind::VoiceReplay => "voice_replay",
        SystemPageKind::RouteChart => "route_chart",
        SystemPageKind::Backlog => "backlog",
        SystemPageKind::LocalizationPreview => "localization_preview",
        SystemPageKind::Custom => "custom",
        SystemPageKind::Unknown => "unknown",
    }
}

fn skip_mode_name(mode: SkipMode) -> &'static str {
    match mode {
        SkipMode::None => "none",
        SkipMode::Read => "read",
        SkipMode::All => "all",
    }
}

fn reading_mode_name(mode: ReadingMode) -> &'static str {
    match mode {
        ReadingMode::Hidden => "hidden",
        ReadingMode::Manual => "manual",
        ReadingMode::FastForward => "fast_forward",
    }
}

fn unlock_kind_name(kind: SystemUnlockKind) -> &'static str {
    match kind {
        SystemUnlockKind::Gallery => "gallery",
        SystemUnlockKind::Replay => "replay",
    }
}

fn runtime_live_vn_state(state: &VnRuntimeState) -> astra_plugin_abi::RuntimeLiveVnState {
    let active_page = state.system_stack.last().map(|frame| frame.page);
    let expose_route_history =
        active_page == Some(SystemPageKind::RouteChart) || state.cursor.is_none();
    astra_plugin_abi::RuntimeLiveVnState {
        backlog_count: state.backlog.len(),
        revision: state.revision,
        instance_id: state.instance_id.clone(),
        profile: state.profile.clone(),
        locale: state.locale.clone(),
        cursor: state.cursor.clone().map(runtime_live_vn_cursor),
        system_stack: state
            .system_stack
            .iter()
            .cloned()
            .map(|frame| astra_plugin_abi::RuntimeLiveVnSystemFrame {
                return_to: runtime_live_vn_cursor(frame.return_to),
                return_wait: frame.return_wait.map(runtime_live_vn_wait),
                return_choice: frame.return_choice.map(runtime_live_vn_choice),
                page: runtime_live_system_page(frame.page),
            })
            .collect(),
        system: astra_plugin_abi::RuntimeLiveVnSystemState {
            auto_enabled: state.system.auto_enabled,
            skip_mode: match state.system.skip_mode {
                SkipMode::None => astra_plugin_abi::RuntimeLiveVnSkipMode::None,
                SkipMode::Read => astra_plugin_abi::RuntimeLiveVnSkipMode::Read,
                SkipMode::All => astra_plugin_abi::RuntimeLiveVnSkipMode::All,
            },
            config: state
                .system
                .config
                .iter()
                .map(|(key, value)| astra_plugin_abi::RuntimeLiveVnStringEntry {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
            gallery_unlocks: state.system.gallery_unlocks.iter().cloned().collect(),
            replay_unlocks: state.system.replay_unlocks.iter().cloned().collect(),
            reading_mode: match state.system.reading_mode {
                ReadingMode::Hidden => astra_plugin_abi::RuntimeLiveVnReadingMode::Hidden,
                ReadingMode::Manual => astra_plugin_abi::RuntimeLiveVnReadingMode::Manual,
                ReadingMode::FastForward => astra_plugin_abi::RuntimeLiveVnReadingMode::FastForward,
            },
            audio_enabled: state.system.audio_enabled,
            skip_allowed: state.system.skip_allowed,
        },
        pending_choice: state.pending_choice.clone().map(runtime_live_vn_choice),
        backlog: state
            .backlog
            .iter()
            .skip(if active_page == Some(SystemPageKind::Backlog) {
                0
            } else {
                state.backlog.len().saturating_sub(1)
            })
            .cloned()
            .map(|entry| astra_plugin_abi::RuntimeLiveVnBacklogEntry {
                command_id: entry.command_id,
                key: entry.key,
                speaker: entry.speaker,
                voice: entry.voice,
                story_id: entry.story_id,
                state_id: entry.state_id,
                route_position: entry.route_position,
                read: entry.read,
                window: entry.layout.window,
            })
            .collect(),
        voice_replay: state
            .voice_replay
            .iter()
            .filter(|_| active_page == Some(SystemPageKind::VoiceReplay))
            .map(
                |(id, entry)| astra_plugin_abi::RuntimeLiveVnVoiceReplayEntry {
                    id: id.clone(),
                    voice: entry.voice.clone(),
                    line_key: entry.line_key.clone(),
                    speaker: entry.speaker.clone(),
                },
            )
            .collect(),
        route_coverage: state
            .route_coverage
            .iter()
            .filter(|_| expose_route_history)
            .cloned()
            .collect(),
        route_flags: state
            .route_flags
            .iter()
            .filter(|_| expose_route_history)
            .map(|(id, flag)| astra_plugin_abi::RuntimeLiveVnRouteFlag {
                id: id.clone(),
                kind: match flag.kind {
                    VnRouteFlagKind::Launch => astra_plugin_abi::RuntimeLiveVnRouteFlagKind::Launch,
                    VnRouteFlagKind::Choice => astra_plugin_abi::RuntimeLiveVnRouteFlagKind::Choice,
                    VnRouteFlagKind::Jump => astra_plugin_abi::RuntimeLiveVnRouteFlagKind::Jump,
                    VnRouteFlagKind::Branch => astra_plugin_abi::RuntimeLiveVnRouteFlagKind::Branch,
                    VnRouteFlagKind::Call => astra_plugin_abi::RuntimeLiveVnRouteFlagKind::Call,
                    VnRouteFlagKind::Return => astra_plugin_abi::RuntimeLiveVnRouteFlagKind::Return,
                },
                source: flag.source.clone(),
                target: flag.target.clone(),
                count: flag.count,
            })
            .collect(),
        pending_wait: state.pending_wait.clone().map(runtime_live_vn_wait),
    }
}

fn runtime_live_vn_cursor(cursor: VnCommandCursor) -> astra_plugin_abi::RuntimeLiveVnCursor {
    astra_plugin_abi::RuntimeLiveVnCursor {
        story_id: cursor.story_id,
        state_id: cursor.state_id,
        scene_id: cursor.scene_id,
        command_id: cursor.command_id,
        ordinal: cursor.ordinal,
    }
}

fn runtime_live_vn_choice(choice: PendingChoice) -> astra_plugin_abi::RuntimeLiveVnPendingChoice {
    astra_plugin_abi::RuntimeLiveVnPendingChoice {
        choice_id: choice.choice_id,
        key: choice.key,
        options: choice
            .options
            .into_iter()
            .map(runtime_live_choice_option)
            .collect(),
        enabled_option_ids: choice.enabled_option_ids.into_iter().collect(),
    }
}

fn runtime_live_vn_wait(wait: VnWaitState) -> astra_plugin_abi::RuntimeLiveVnWait {
    astra_plugin_abi::RuntimeLiveVnWait {
        kind: match wait.kind {
            VnWaitKind::Dialogue => astra_plugin_abi::RuntimeLiveVnWaitKind::Dialogue,
            VnWaitKind::Choice => astra_plugin_abi::RuntimeLiveVnWaitKind::Choice,
            VnWaitKind::SystemPage => astra_plugin_abi::RuntimeLiveVnWaitKind::SystemPage,
            VnWaitKind::Fence => astra_plugin_abi::RuntimeLiveVnWaitKind::Fence,
            VnWaitKind::Timer => astra_plugin_abi::RuntimeLiveVnWaitKind::Timer,
            VnWaitKind::TimelineComplete => {
                astra_plugin_abi::RuntimeLiveVnWaitKind::TimelineComplete
            }
            VnWaitKind::MovieEnd => astra_plugin_abi::RuntimeLiveVnWaitKind::MovieEnd,
            VnWaitKind::VoiceEnd => astra_plugin_abi::RuntimeLiveVnWaitKind::VoiceEnd,
            VnWaitKind::Input => astra_plugin_abi::RuntimeLiveVnWaitKind::Input,
        },
        fence: wait.fence,
        command_id: wait.command_id,
        await_id: wait.await_id,
    }
}

impl RuntimeAction for VnStepAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.vn.step",
            "astra.vn.step_action_input.v1",
            "astra.vn.step_output.v1",
            ActionExecutionClass::Serial,
            ActionAccess::new(
                [ActionResourceKey::EventQueue],
                [
                    ActionResourceKey::AwaitQueue,
                    ActionResourceKey::EventQueue,
                    ActionResourceKey::StableIdSource,
                ],
            ),
            200_000,
        )
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
        let control = self
            .pending_control
            .lock()
            .map_err(|_| RuntimeError::message("VN control lock is poisoned"))?
            .take()
            .ok_or_else(|| {
                RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                    "ASTRA_NATIVE_VN_CONTROL_MISSING",
                    "NativeVN provider did not prepare a control transaction",
                ))
            })?;
        for (kind, id) in control.events {
            ctx.emit_event(
                astra_runtime::EventSource::StateMachine,
                EventPayload {
                    kind,
                    data: [("id".to_string(), BlackboardValue::String(id))]
                        .into_iter()
                        .collect(),
                },
            );
        }
        if let Some(kind) = control.create_wait {
            let token = ctx.create_await(kind);
            let token_id = token.token_id;
            ctx.push_await(token)?;
            *self
                .control_result
                .lock()
                .map_err(|_| RuntimeError::message("VN control result lock is poisoned"))? =
                Some(token_id);
        }
        let mut trace_payload = if ctx.evidence_mode() {
            input.clone()
        } else {
            BTreeMap::new()
        };
        trace_payload.insert(
            "event_kind".to_string(),
            BlackboardValue::String(event_kind),
        );
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: trace_payload,
        })
    }
}

fn runtime_live_audio_cue(sequence: u64, command: &VnAudioCommand) -> RuntimeLiveAudioCue {
    RuntimeLiveAudioCue {
        sequence,
        command_id: command.command_id.clone(),
        bus: match command.cue.bus {
            VnAudioBus::Voice => RuntimeLiveAudioBus::Voice,
            VnAudioBus::Bgm => RuntimeLiveAudioBus::Bgm,
            VnAudioBus::Se => RuntimeLiveAudioBus::Se,
            VnAudioBus::Movie => RuntimeLiveAudioBus::Movie,
        },
        asset: command.cue.asset.clone(),
        looped: command.cue.looped,
        fade_ms: command.cue.fade_ms,
        sync: match &command.cue.sync {
            VnAudioSync::None => RuntimeLiveAudioSync::None,
            VnAudioSync::Text => RuntimeLiveAudioSync::Text,
            VnAudioSync::Fence(fence) => RuntimeLiveAudioSync::Fence(fence.clone()),
        },
    }
}

fn runtime_live_presentation(
    sequence: u64,
    command: PresentationCommand,
) -> astra_plugin_abi::RuntimeLivePresentationCommand {
    use astra_plugin_abi::RuntimeLivePresentationKind as Live;
    let command = match command {
        PresentationCommand::Dialogue {
            key,
            speaker,
            voice,
            window,
        } => Live::Dialogue {
            key,
            speaker,
            voice,
            window,
        },
        PresentationCommand::Choice { key, options } => Live::Choice {
            key,
            options: options
                .into_iter()
                .map(runtime_live_choice_option)
                .collect(),
        },
        PresentationCommand::SystemPage { page } => Live::SystemPage {
            page: runtime_live_system_page(page),
        },
        PresentationCommand::SystemOption { option } => Live::SystemOption {
            option: runtime_live_choice_option(option),
        },
        PresentationCommand::Stage(command) => Live::Stage(runtime_live_stage(command)),
        PresentationCommand::Extension(command) => {
            Live::Extension(astra_plugin_abi::RuntimeLiveExtensionCommand {
                command: command.command,
                provider_id: command.provider_id,
                schema: command.schema,
                fields: command
                    .fields
                    .into_iter()
                    .map(|(name, value)| {
                        (
                            name,
                            match value {
                                ExtensionValue::String(value) => {
                                    astra_plugin_abi::RuntimeLiveExtensionValue::String(value)
                                }
                                ExtensionValue::Integer(value) => {
                                    astra_plugin_abi::RuntimeLiveExtensionValue::Integer(value)
                                }
                                ExtensionValue::Fixed(value) => {
                                    astra_plugin_abi::RuntimeLiveExtensionValue::Fixed(
                                        value.millionths,
                                    )
                                }
                                ExtensionValue::Boolean(value) => {
                                    astra_plugin_abi::RuntimeLiveExtensionValue::Boolean(value)
                                }
                                ExtensionValue::Symbol(value) => {
                                    astra_plugin_abi::RuntimeLiveExtensionValue::Symbol(value)
                                }
                                ExtensionValue::AssetUri(value) => {
                                    astra_plugin_abi::RuntimeLiveExtensionValue::AssetUri(value)
                                }
                            },
                        )
                    })
                    .collect(),
            })
        }
        PresentationCommand::Marker { id } => Live::Marker { id },
    };
    astra_plugin_abi::RuntimeLivePresentationCommand { sequence, command }
}

fn runtime_live_choice_option(option: ChoiceOption) -> astra_plugin_abi::RuntimeLiveChoiceOption {
    astra_plugin_abi::RuntimeLiveChoiceOption {
        id: option.id,
        key: option.key,
        target: option.target,
        enabled_when: option.enabled_when.map(|condition| {
            astra_plugin_abi::RuntimeLiveVariableCondition {
                scope: condition.scope,
                key: condition.key,
                operation: match condition.op {
                    BranchOp::Eq => astra_plugin_abi::RuntimeLiveComparison::Equal,
                    BranchOp::NotEq => astra_plugin_abi::RuntimeLiveComparison::NotEqual,
                    BranchOp::Less => astra_plugin_abi::RuntimeLiveComparison::Less,
                    BranchOp::LessEq => astra_plugin_abi::RuntimeLiveComparison::LessEqual,
                    BranchOp::Greater => astra_plugin_abi::RuntimeLiveComparison::Greater,
                    BranchOp::GreaterEq => astra_plugin_abi::RuntimeLiveComparison::GreaterEqual,
                },
                value: condition.value,
            }
        }),
    }
}

fn runtime_live_system_page(page: SystemPageKind) -> astra_plugin_abi::RuntimeLiveSystemPage {
    use astra_plugin_abi::RuntimeLiveSystemPage as Live;
    match page {
        SystemPageKind::Title => Live::Title,
        SystemPageKind::QuickPanel => Live::QuickPanel,
        SystemPageKind::Save => Live::Save,
        SystemPageKind::Load => Live::Load,
        SystemPageKind::Config => Live::Config,
        SystemPageKind::Gallery => Live::Gallery,
        SystemPageKind::Replay => Live::Replay,
        SystemPageKind::VoiceReplay => Live::VoiceReplay,
        SystemPageKind::RouteChart => Live::RouteChart,
        SystemPageKind::Backlog => Live::Backlog,
        SystemPageKind::LocalizationPreview => Live::LocalizationPreview,
        SystemPageKind::Custom => Live::Custom,
        SystemPageKind::Unknown => Live::Unknown,
    }
}

fn runtime_live_timeline(command: TimelineCommand) -> astra_plugin_abi::RuntimeLiveTimelineCommand {
    match command {
        TimelineCommand::Start(spec) => astra_plugin_abi::RuntimeLiveTimelineCommand::Start(
            astra_plugin_abi::RuntimeLiveTimelineSpec {
                id: spec.id,
                join: match spec.join {
                    VnTimelineJoinPolicy::FireAndForget => {
                        astra_plugin_abi::RuntimeLiveTimelineJoin::FireAndForget
                    }
                    VnTimelineJoinPolicy::Block => astra_plugin_abi::RuntimeLiveTimelineJoin::Block,
                    VnTimelineJoinPolicy::ReplaceTarget => {
                        astra_plugin_abi::RuntimeLiveTimelineJoin::ReplaceTarget
                    }
                },
                tracks: spec
                    .tracks
                    .into_iter()
                    .map(|track| astra_plugin_abi::RuntimeLiveTimelineTrack {
                        target: track.target,
                        property: track.property,
                        keyframes: track
                            .keyframes
                            .into_iter()
                            .map(|keyframe| astra_plugin_abi::RuntimeLiveTimelineKeyframe {
                                time_ms: keyframe.time_ms,
                                value_millionths: keyframe.value.millionths,
                            })
                            .collect(),
                    })
                    .collect(),
                fence: spec.fence,
                fallback: spec.fallback,
                budget_us: spec.budget_us,
            },
        ),
        TimelineCommand::Cancel { id, reason } => {
            astra_plugin_abi::RuntimeLiveTimelineCommand::Cancel { id, reason }
        }
    }
}

fn runtime_live_stage(command: StageCommand) -> astra_plugin_abi::RuntimeLiveStageCommand {
    use astra_plugin_abi::RuntimeLiveStageCommand as Live;
    match command {
        StageCommand::Preload { asset } => Live::Preload { asset },
        StageCommand::Configure {
            viewport,
            safe_area,
        } => Live::Configure {
            width: viewport.width,
            height: viewport.height,
            safe_area_width: safe_area.width,
            safe_area_height: safe_area.height,
        },
        StageCommand::DeclareLayer {
            id,
            kind,
            z,
            blend,
            clip,
            input,
        } => Live::DeclareLayer {
            id,
            kind: match kind {
                StageLayerKind::Background => {
                    astra_plugin_abi::RuntimeLiveStageLayerKind::Background
                }
                StageLayerKind::Sprite => astra_plugin_abi::RuntimeLiveStageLayerKind::Sprite,
                StageLayerKind::Video => astra_plugin_abi::RuntimeLiveStageLayerKind::Video,
                StageLayerKind::Text => astra_plugin_abi::RuntimeLiveStageLayerKind::Text,
                StageLayerKind::Cg => astra_plugin_abi::RuntimeLiveStageLayerKind::Cg,
                StageLayerKind::Ui => astra_plugin_abi::RuntimeLiveStageLayerKind::Ui,
                StageLayerKind::Effect => astra_plugin_abi::RuntimeLiveStageLayerKind::Effect,
            },
            z,
            blend: match blend {
                StageBlendMode::Normal => astra_plugin_abi::RuntimeLiveStageBlend::Normal,
                StageBlendMode::Add => astra_plugin_abi::RuntimeLiveStageBlend::Add,
                StageBlendMode::Multiply => astra_plugin_abi::RuntimeLiveStageBlend::Multiply,
                StageBlendMode::Screen => astra_plugin_abi::RuntimeLiveStageBlend::Screen,
            },
            clip: clip.map(|clip| match clip {
                StageClipPolicy::Stage => astra_plugin_abi::RuntimeLiveStageClip::Stage,
                StageClipPolicy::SafeArea => astra_plugin_abi::RuntimeLiveStageClip::SafeArea,
            }),
            input,
        },
        StageCommand::Background {
            asset,
            layer,
            preset,
            duration_ms,
            interrupt,
        } => Live::Background {
            asset,
            layer,
            preset,
            duration_ms,
            interrupt: runtime_live_interrupt(interrupt),
        },
        StageCommand::Show {
            id,
            asset,
            pose,
            layer,
            placement,
            fit,
            opacity,
            preset,
            interrupt,
        } => Live::Show {
            id,
            asset,
            pose,
            layer,
            placement: match placement {
                StagePlacement::Left => astra_plugin_abi::RuntimeLiveStagePlacement::Left,
                StagePlacement::Center => astra_plugin_abi::RuntimeLiveStagePlacement::Center,
                StagePlacement::Right => astra_plugin_abi::RuntimeLiveStagePlacement::Right,
            },
            fit: match fit {
                StageFitMode::ContainHeight => astra_plugin_abi::RuntimeLiveStageFit::ContainHeight,
                StageFitMode::Native => astra_plugin_abi::RuntimeLiveStageFit::Native,
            },
            opacity_millionths: opacity.millionths,
            preset,
            interrupt: runtime_live_interrupt(interrupt),
        },
        StageCommand::Hide {
            id,
            preset,
            duration_ms,
            interrupt,
        } => Live::Hide {
            id,
            preset,
            duration_ms,
            interrupt: runtime_live_interrupt(interrupt),
        },
        StageCommand::ClearLayer {
            layer,
            duration_ms,
            interrupt,
        } => Live::ClearLayer {
            layer,
            duration_ms,
            interrupt: runtime_live_interrupt(interrupt),
        },
        StageCommand::SetLayerVisibility { layer, visible } => {
            Live::SetLayerVisibility { layer, visible }
        }
        StageCommand::Backdrop { color } => Live::Backdrop { color },
        StageCommand::Shade { color, opacity } => Live::Shade {
            color,
            opacity_millionths: opacity.millionths,
        },
        StageCommand::SetSkipAllowed { allowed } => Live::SetSkipAllowed { allowed },
        StageCommand::Move {
            id,
            x,
            y,
            duration_ms,
            preset,
            interrupt,
        } => Live::Move {
            id,
            x_millionths: x.millionths,
            y_millionths: y.millionths,
            duration_ms,
            preset,
            interrupt: runtime_live_interrupt(interrupt),
        },
        StageCommand::Camera {
            target,
            x,
            y,
            zoom,
            rotation,
            duration_ms,
            preset,
        } => Live::Camera {
            target,
            x_millionths: x.millionths,
            y_millionths: y.millionths,
            zoom_millionths: zoom.millionths,
            rotation_millionths: rotation.millionths,
            duration_ms,
            preset,
        },
        StageCommand::Movie {
            layer,
            asset,
            alpha,
            loop_mode,
            end,
            fence,
            fallback,
            interrupt,
        } => Live::Movie {
            layer,
            asset,
            alpha_millionths: alpha.millionths,
            loop_mode: match loop_mode {
                MovieLoopMode::Once => astra_plugin_abi::RuntimeLiveMovieLoop::Once,
                MovieLoopMode::Loop => astra_plugin_abi::RuntimeLiveMovieLoop::Loop,
            },
            end: match end {
                VnMovieEndBehavior::Continue => astra_plugin_abi::RuntimeLiveMovieEnd::Continue,
                VnMovieEndBehavior::Wait => astra_plugin_abi::RuntimeLiveMovieEnd::Wait,
                VnMovieEndBehavior::Hold => astra_plugin_abi::RuntimeLiveMovieEnd::Hold,
            },
            fence,
            fallback,
            interrupt: runtime_live_interrupt(interrupt),
        },
        StageCommand::Audio(cue) => Live::Audio(astra_plugin_abi::RuntimeLiveAudioCueCommand {
            id: cue.id,
            bus: match cue.bus {
                VnAudioBus::Voice => RuntimeLiveAudioBus::Voice,
                VnAudioBus::Bgm => RuntimeLiveAudioBus::Bgm,
                VnAudioBus::Se => RuntimeLiveAudioBus::Se,
                VnAudioBus::Movie => RuntimeLiveAudioBus::Movie,
            },
            asset: cue.asset,
            looped: cue.looped,
            fade_ms: cue.fade_ms,
            sync: match cue.sync {
                VnAudioSync::None => RuntimeLiveAudioSync::None,
                VnAudioSync::Text => RuntimeLiveAudioSync::Text,
                VnAudioSync::Fence(fence) => RuntimeLiveAudioSync::Fence(fence),
            },
        }),
        StageCommand::AudioControl(control) => {
            Live::AudioControl(astra_plugin_abi::RuntimeLiveAudioControl {
                id: control.id,
                action: match control.action {
                    VnAudioControlAction::Pause => {
                        astra_plugin_abi::RuntimeLiveAudioControlAction::Pause
                    }
                    VnAudioControlAction::Resume => {
                        astra_plugin_abi::RuntimeLiveAudioControlAction::Resume
                    }
                    VnAudioControlAction::Stop => {
                        astra_plugin_abi::RuntimeLiveAudioControlAction::Stop
                    }
                    VnAudioControlAction::FadeStop { duration_ms, fence } => {
                        astra_plugin_abi::RuntimeLiveAudioControlAction::FadeStop {
                            duration_ms,
                            fence,
                        }
                    }
                },
                target: control.target,
            })
        }
        StageCommand::SetAudioBusEnabled { bus, enabled } => Live::SetAudioBusEnabled {
            bus: match bus {
                VnAudioBus::Voice => RuntimeLiveAudioBus::Voice,
                VnAudioBus::Bgm => RuntimeLiveAudioBus::Bgm,
                VnAudioBus::Se => RuntimeLiveAudioBus::Se,
                VnAudioBus::Movie => RuntimeLiveAudioBus::Movie,
            },
            enabled,
        },
        StageCommand::Transition {
            preset,
            duration_ms,
            descriptor_id,
        } => Live::Transition {
            preset,
            duration_ms,
            descriptor_id,
        },
        StageCommand::Shake {
            target,
            strength,
            duration_ms,
        } => Live::Shake {
            target,
            strength_millionths: strength.millionths,
            duration_ms,
        },
        StageCommand::Timeline(command) => Live::Timeline(runtime_live_timeline(command)),
        StageCommand::Effect {
            target,
            lip_sync,
            filter,
            fallback,
            budget_us,
        } => Live::Effect {
            target,
            lip_sync,
            filter,
            fallback,
            budget_us,
        },
    }
}

fn runtime_live_interrupt(
    interrupt: PresentationInterruptPolicy,
) -> astra_plugin_abi::RuntimeLiveInterruptPolicy {
    match interrupt {
        PresentationInterruptPolicy::Queue => astra_plugin_abi::RuntimeLiveInterruptPolicy::Queue,
        PresentationInterruptPolicy::ReplaceFromCurrent => {
            astra_plugin_abi::RuntimeLiveInterruptPolicy::ReplaceFromCurrent
        }
        PresentationInterruptPolicy::SnapThenStart => {
            astra_plugin_abi::RuntimeLiveInterruptPolicy::SnapThenStart
        }
        PresentationInterruptPolicy::Reject => astra_plugin_abi::RuntimeLiveInterruptPolicy::Reject,
    }
}

fn vn_event_kind(command: &CoreVnPlayerCommand) -> &'static str {
    match command {
        CoreVnPlayerCommand::Launch { .. } => "vn.launch",
        CoreVnPlayerCommand::Advance => "player.advance",
        CoreVnPlayerCommand::Choose { .. } => "choice.selected",
        CoreVnPlayerCommand::OpenSystem { .. } => "system.open",
        CoreVnPlayerCommand::SwitchSystemPage { .. } => "system.switch",
        CoreVnPlayerCommand::ReturnSystem => "system.return",
        CoreVnPlayerCommand::ReplayVoice { .. } => "voice.replay",
        CoreVnPlayerCommand::SetAuto { .. } => "system.auto",
        CoreVnPlayerCommand::SetSkip { .. } => "system.skip",
        CoreVnPlayerCommand::SetReadingMode { .. } => "system.reading_mode",
        CoreVnPlayerCommand::SetAudioEnabled { .. } => "system.audio_enabled",
        CoreVnPlayerCommand::InvokeSystemAction { .. } => "system.action",
        CoreVnPlayerCommand::SetConfig { .. } => "system.config",
        CoreVnPlayerCommand::StartReplay { .. } => "system.replay.start",
        CoreVnPlayerCommand::PreviewGallery { .. } => "system.gallery.preview",
        CoreVnPlayerCommand::JumpRoute { .. } => "system.route.jump",
        CoreVnPlayerCommand::JumpBacklog { .. } => "system.backlog.jump",
        CoreVnPlayerCommand::SubmitText { .. } => "system.text.submit",
        CoreVnPlayerCommand::Unlock { .. } => "system.unlock",
        CoreVnPlayerCommand::CompleteWait { .. } => "await.completed",
    }
}

fn vn_runtime_event_kinds() -> [&'static str; 20] {
    [
        "vn.launch",
        "player.advance",
        "choice.selected",
        "system.open",
        "system.switch",
        "system.return",
        "voice.replay",
        "system.auto",
        "system.skip",
        "system.reading_mode",
        "system.audio_enabled",
        "system.action",
        "system.config",
        "system.replay.start",
        "system.gallery.preview",
        "system.route.jump",
        "system.backlog.jump",
        "system.text.submit",
        "system.unlock",
        "await.completed",
    ]
}

fn command_resolves_wait(
    command: &CoreVnPlayerCommand,
    wait: Option<VnWaitKind>,
    reading_mode: astra_vn_core::ReadingMode,
    compiled: &astra_vn_core::CompiledStory,
) -> bool {
    if matches!(command, CoreVnPlayerCommand::Advance)
        && matches!(wait, Some(VnWaitKind::Dialogue | VnWaitKind::Input))
    {
        return reading_mode != astra_vn_core::ReadingMode::Hidden;
    }
    matches!(
        (command, wait),
        (CoreVnPlayerCommand::Choose { .. }, Some(VnWaitKind::Choice))
            | (
                CoreVnPlayerCommand::ReturnSystem,
                Some(VnWaitKind::SystemPage)
            )
            | (
                CoreVnPlayerCommand::SwitchSystemPage { .. },
                Some(VnWaitKind::SystemPage)
            )
            | (
                CoreVnPlayerCommand::SetReadingMode {
                    mode: astra_vn_core::ReadingMode::FastForward,
                },
                Some(VnWaitKind::Dialogue | VnWaitKind::Input)
            )
            | (
                CoreVnPlayerCommand::StartReplay { .. }
                    | CoreVnPlayerCommand::JumpRoute { .. }
                    | CoreVnPlayerCommand::JumpBacklog { .. },
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
    ) || matches!(
        (command, wait),
        (
            CoreVnPlayerCommand::InvokeSystemAction { action_id },
            Some(VnWaitKind::SystemPage)
        ) if compiled
            .system_story_manifest
            .actions
            .get(action_id)
            .is_some_and(|action| action.effects.iter().any(|effect| matches!(
                effect,
                astra_vn_core::SystemActionEffect::Jump { .. }
                    | astra_vn_core::SystemActionEffect::SwitchSystemPage { .. }
                    | astra_vn_core::SystemActionEffect::ReturnSystem
            )))
    )
}

fn required_restore_section<'a>(
    sections: &'a [RuntimeSectionPayload],
    section_id: &str,
    schema: &str,
) -> Result<&'a RuntimeSectionPayload, CoreVnError> {
    required_restore_section_with_codec(sections, section_id, schema, RuntimeSectionCodec::Postcard)
}

fn required_restore_section_with_codec<'a>(
    sections: &'a [RuntimeSectionPayload],
    section_id: &str,
    schema: &str,
    codec: RuntimeSectionCodec,
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
    if section.schema != schema || section.codec != codec {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_SECTION_SCHEMA",
            format!("restore section {section_id} has an incompatible schema or codec"),
        ));
    }
    Ok(section)
}

fn native_vn_package_sections() -> Vec<String> {
    [
        "vn.compiled_project",
        "vn.story",
        "vn.ui_blueprint_bundle",
        "vn.ui_binding_manifest",
        "vn.ui_source_map",
        "vn.ui_controller_manifest",
        "vn.ui_theme_manifest",
        "vn.ui_backend_manifest",
        "vn.ui_component_manifest",
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
extern "C" fn ffi_prepare(request: FfiRuntimePrepareRequest) -> FfiRuntimeReportResult {
    let report = NativeVnRuntimeProvider::default().prepare(RuntimePrepareRequest {
        target_id: request.target_id.to_string(),
        profile: request.profile.to_string(),
        package_hash: request.package_id.to_string(),
        section_ids: request
            .section_ids
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    });
    ffi_report_ok(
        NATIVE_VN_RUNTIME_ID,
        NATIVE_VN_PROVIDER_ID,
        report.status,
        report.diagnostics,
    )
}

#[cfg(feature = "ffi")]
type FfiSession = Arc<Mutex<Option<Box<dyn ProductRuntimeSession>>>>;

#[cfg(feature = "ffi")]
struct FfiProviderInstance {
    factory: Arc<NativeVnRuntimeProviderFactory>,
    next_session_handle: u64,
    sessions: BTreeMap<u64, FfiSession>,
}

#[cfg(feature = "ffi")]
static FFI_INSTANCES: OnceLock<Mutex<BTreeMap<String, FfiProviderInstance>>> = OnceLock::new();

#[cfg(feature = "ffi")]
fn ffi_instances() -> &'static Mutex<BTreeMap<String, FfiProviderInstance>> {
    FFI_INSTANCES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_create_instance(request: FfiRuntimeInstanceRequest) -> FfiRuntimeReportResult {
    let result = (|| -> Result<RuntimeProviderInstanceReport, String> {
        let mut instances = ffi_instances()
            .lock()
            .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
        let instance_id = request.instance_id.to_string();
        if instances.contains_key(&instance_id) {
            return Err("provider instance id is already active".to_string());
        }
        let factory = Arc::new(NativeVnRuntimeProviderFactory::default());
        let report =
            factory.create_instance(astra_plugin_abi::ProviderInstanceId(instance_id.clone()))?;
        instances.insert(
            instance_id,
            FfiProviderInstance {
                factory,
                next_session_handle: 1,
                sessions: BTreeMap::new(),
            },
        );
        Ok(report)
    })();
    match result {
        Ok(report) => ffi_report_ok(
            NATIVE_VN_RUNTIME_ID,
            NATIVE_VN_PROVIDER_ID,
            report.status,
            report.diagnostics,
        ),
        Err(error) => ffi_report_error(error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_destroy_instance(request: FfiRuntimeInstanceRequest) -> FfiRuntimeReportResult {
    let result = (|| -> Result<RuntimeProviderInstanceReport, String> {
        let mut instances = ffi_instances()
            .lock()
            .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
        let instance_id = request.instance_id.to_string();
        let instance = instances
            .get(&instance_id)
            .ok_or_else(|| "provider instance is not active".to_string())?;
        if !instance.sessions.is_empty() {
            return Err("provider instance still has active sessions".to_string());
        }
        let report = instance
            .factory
            .destroy_instance(astra_plugin_abi::ProviderInstanceId(instance_id.clone()))?;
        instances.remove(&instance_id);
        Ok(report)
    })();
    match result {
        Ok(report) => ffi_report_ok(
            NATIVE_VN_RUNTIME_ID,
            NATIVE_VN_PROVIDER_ID,
            report.status,
            report.diagnostics,
        ),
        Err(error) => ffi_report_error(error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_probe(request: FfiRuntimeProbeRequest) -> FfiRuntimeReportResult {
    let report = NativeVnRuntimeProvider::default().probe(RuntimeProbeRequest {
        target_id: request.target_id.to_string(),
        profile: request.profile.to_string(),
        platform: request
            .platform
            .into_option()
            .map(|value| value.to_string()),
        section_ids: request
            .section_ids
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    });
    ffi_report_ok(
        NATIVE_VN_RUNTIME_ID,
        NATIVE_VN_PROVIDER_ID,
        report.status,
        report.diagnostics,
    )
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_open(request: FfiRuntimeOpenRequest) -> FfiRuntimeOpenResult {
    let instance_id = request.instance_id.to_string();
    let result = (|| -> Result<(RuntimeOpenReport, u64), String> {
        let factory = {
            let instances = ffi_instances()
                .lock()
                .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
            Arc::clone(
                &instances
                    .get(&instance_id)
                    .ok_or_else(|| "provider instance is not active".to_string())?
                    .factory,
            )
        };
        let (report, session) = factory
            .open(ffi_open_request(request)?)
            .map_err(|error| error.to_string())?;
        let mut instances = ffi_instances()
            .lock()
            .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
        let instance = instances
            .get_mut(&instance_id)
            .ok_or_else(|| "provider instance was destroyed while opening a session".to_string())?;
        let handle = instance.next_session_handle;
        instance.next_session_handle = handle
            .checked_add(1)
            .ok_or_else(|| "provider session handle space is exhausted".to_string())?;
        instance
            .sessions
            .insert(handle, Arc::new(Mutex::new(Some(session))));
        Ok((report, handle))
    })();
    match result {
        Ok((report, session_handle)) => FfiRuntimeOpenResult {
            ok: true,
            session_handle,
            session_id: report.session_id.0.into(),
            runtime_id: report.runtime_id.into(),
            provider_id: report.provider_id.into(),
            diagnostics: RVec::from(
                report
                    .diagnostics
                    .into_iter()
                    .map(RString::from)
                    .collect::<Vec<_>>(),
            ),
        },
        Err(error) => ffi_open_error(error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_step(request: FfiRuntimeStepRequest) -> FfiRuntimeStepResult {
    let session_id = request.session_id.to_string();
    let instance_id = request.instance_id.to_string();
    let session_handle = request.session_handle;
    let result = (|| -> Result<RuntimeStepOutput, String> {
        let input = ffi_step_input(request)?;
        let session = find_ffi_session(&instance_id, session_handle)?;
        let mut guard = session
            .lock()
            .map_err(|_| "provider session lock is poisoned".to_string())?;
        let session = guard
            .as_deref_mut()
            .ok_or_else(|| "provider session is already closed".to_string())?;
        session.step(input)
    })();
    match result {
        Ok(output) => ffi_step_output(output),
        Err(error) => ffi_step_error(session_id, error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_save(request: FfiRuntimeSaveRequest) -> FfiRuntimeSaveResult {
    let session_id = request.session_id.to_string();
    let result = (|| -> Result<RuntimeSaveSections, String> {
        let session = find_ffi_session(&request.instance_id.to_string(), request.session_handle)?;
        let mut guard = session
            .lock()
            .map_err(|_| "provider session lock is poisoned".to_string())?;
        let session = guard
            .as_deref_mut()
            .ok_or_else(|| "provider session is already closed".to_string())?;
        session.save(RuntimeSaveRequest {
            session_id: GameRuntimeSessionId(request.session_id.to_string()),
            slot: request.slot.to_string(),
        })
    })();
    match result {
        Ok(value) => ffi_save_result(value),
        Err(error) => ffi_save_error(session_id, error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_restore(request: FfiRuntimeRestoreRequest) -> FfiRuntimeRestoreResult {
    let session_id = request.session_id.to_string();
    let result = (|| -> Result<RuntimeRestoreReport, String> {
        let session = find_ffi_session(&request.instance_id.to_string(), request.session_handle)?;
        let mut guard = session
            .lock()
            .map_err(|_| "provider session lock is poisoned".to_string())?;
        let session = guard
            .as_deref_mut()
            .ok_or_else(|| "provider session is already closed".to_string())?;
        session.restore(RuntimeRestoreRequest {
            session_id: GameRuntimeSessionId(request.session_id.to_string()),
            sections: request
                .sections
                .into_iter()
                .map(ffi_runtime_section)
                .collect(),
        })
    })();
    match result {
        Ok(value) => FfiRuntimeRestoreResult {
            ok: true,
            session_id: value.session_id.0.into(),
            restored_fixed_step: value.restored_fixed_step,
            session_seed: value.session_seed,
            status: value.status.into(),
            diagnostics: RVec::from(
                value
                    .diagnostics
                    .into_iter()
                    .map(RString::from)
                    .collect::<Vec<_>>(),
            ),
        },
        Err(error) => ffi_restore_error(session_id, error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_shutdown(request: FfiRuntimeShutdownRequest) -> FfiRuntimeShutdownResult {
    let session_id = GameRuntimeSessionId(request.session_id.to_string());
    let session_id_for_error = session_id.0.clone();
    let result = (|| -> Result<RuntimeShutdownReport, String> {
        let session = remove_ffi_session(&request.instance_id.to_string(), request.session_handle)?;
        let mut guard = session
            .lock()
            .map_err(|_| "provider session lock is poisoned".to_string())?;
        let session = guard
            .take()
            .ok_or_else(|| "provider session is already closed".to_string())?;
        session.shutdown(session_id)
    })();
    match result {
        Ok(value) => FfiRuntimeShutdownResult {
            ok: true,
            session_id: value.session_id.0.into(),
            status: value.status.into(),
            diagnostics: RVec::from(
                value
                    .diagnostics
                    .into_iter()
                    .map(RString::from)
                    .collect::<Vec<_>>(),
            ),
        },
        Err(error) => ffi_shutdown_error(session_id_for_error, error),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_package_sections() -> FfiRuntimePackageSectionsResult {
    FfiRuntimePackageSectionsResult {
        ok: true,
        sections: RVec::from(
            NativeVnRuntimeProvider::default()
                .package_sections()
                .sections
                .into_iter()
                .map(|section| RString::from(section.section_id))
                .collect::<Vec<_>>(),
        ),
        diagnostics: RVec::new(),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_release_checks() -> FfiRuntimeReleaseChecksResult {
    FfiRuntimeReleaseChecksResult {
        ok: true,
        checks: RVec::from(
            NativeVnRuntimeProvider::default()
                .release_checks()
                .into_iter()
                .map(|check| RString::from(check.id))
                .collect::<Vec<_>>(),
        ),
        diagnostics: RVec::new(),
    }
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_editor_metadata() -> FfiRuntimeEditorMetadataResult {
    let metadata = NativeVnRuntimeProvider::default().editor_metadata();
    FfiRuntimeEditorMetadataResult {
        ok: true,
        schema: metadata.schema.into(),
        runtime_id: metadata.runtime_id.into(),
        product_kind: metadata.product_kind.into(),
        project_templates: RVec::from(
            metadata
                .project_templates
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
        authoring_surfaces: RVec::from(
            metadata
                .authoring_surfaces
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
        debug_views: RVec::from(
            metadata
                .debug_views
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
        release_checks: RVec::from(
            metadata
                .release_checks
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
        diagnostics: RVec::new(),
    }
}

#[cfg(feature = "ffi")]
fn find_ffi_session(instance_id: &str, session_handle: u64) -> Result<FfiSession, String> {
    let instances = ffi_instances()
        .lock()
        .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
    let instance = instances
        .get(instance_id)
        .ok_or_else(|| "provider instance is not active".to_string())?;
    instance
        .sessions
        .get(&session_handle)
        .cloned()
        .ok_or_else(|| "provider session handle is not active".to_string())
}

#[cfg(feature = "ffi")]
fn remove_ffi_session(instance_id: &str, session_handle: u64) -> Result<FfiSession, String> {
    let mut instances = ffi_instances()
        .lock()
        .map_err(|_| "provider instance registry lock is poisoned".to_string())?;
    let instance = instances
        .get_mut(instance_id)
        .ok_or_else(|| "provider instance is not active".to_string())?;
    instance
        .sessions
        .remove(&session_handle)
        .ok_or_else(|| "provider session handle is not active".to_string())
}

#[cfg(feature = "ffi")]
fn ffi_report_ok(
    runtime_id: &str,
    provider_id: &str,
    status: String,
    diagnostics: Vec<String>,
) -> FfiRuntimeReportResult {
    FfiRuntimeReportResult {
        ok: true,
        runtime_id: runtime_id.into(),
        provider_id: provider_id.into(),
        status: status.into(),
        diagnostics: RVec::from(
            diagnostics
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
    }
}

#[cfg(feature = "ffi")]
fn ffi_report_error(message: String) -> FfiRuntimeReportResult {
    FfiRuntimeReportResult {
        ok: false,
        runtime_id: NATIVE_VN_RUNTIME_ID.into(),
        provider_id: NATIVE_VN_PROVIDER_ID.into(),
        status: "error".into(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}

#[cfg(feature = "ffi")]
fn ffi_open_request(request: FfiRuntimeOpenRequest) -> Result<RuntimeOpenRequest, String> {
    if request.worker_count == 0 {
        return Err("runtime executor worker count must be greater than zero".to_string());
    }
    let executor = if request.worker_count == 1 {
        RuntimeExecutorConfig::serial()
    } else {
        RuntimeExecutorConfig::parallel(request.worker_count)
    };
    executor.validate().map_err(str::to_string)?;
    Ok(RuntimeOpenRequest {
        target_id: request.target_id.to_string(),
        profile: request.profile.to_string(),
        locale: request.locale.to_string(),
        seed: request.seed,
        integrity_mode: match request.integrity_mode {
            FfiRuntimeIntegrityMode::Shipping => RuntimeTickIntegrityMode::Shipping,
            FfiRuntimeIntegrityMode::Evidence => RuntimeTickIntegrityMode::Evidence,
        },
        executor,
        package_hash: request.package_id.to_string(),
        sections: request
            .sections
            .into_iter()
            .map(ffi_runtime_section)
            .collect(),
    })
}

#[cfg(feature = "ffi")]
fn ffi_runtime_section(section: FfiRuntimeSection) -> RuntimeSectionPayload {
    let bytes = section.bytes.into_vec();
    RuntimeSectionPayload {
        section_id: section.section_id.to_string(),
        schema: section.schema.to_string(),
        version: SchemaVersion::new(
            section.version_major,
            section.version_minor,
            section.version_patch,
        ),
        codec: match section.codec {
            FfiRuntimeSectionCodec::Postcard => RuntimeSectionCodec::Postcard,
            FfiRuntimeSectionCodec::Raw => RuntimeSectionCodec::Raw,
            FfiRuntimeSectionCodec::Zstd => RuntimeSectionCodec::Zstd,
        },
        hash: astra_core::Hash256::from_sha256(&bytes),
        bytes,
    }
}

#[cfg(feature = "ffi")]
fn ffi_step_input(request: FfiRuntimeStepRequest) -> Result<RuntimeStepInput, String> {
    let action = request.action.to_string();
    let argument = request
        .argument
        .into_option()
        .map(|argument| argument.to_string());
    let auxiliary = request
        .auxiliary
        .into_option()
        .map(|auxiliary| auxiliary.to_string());
    let flag = request.flag.into_option();
    Ok(RuntimeStepInput {
        session_id: GameRuntimeSessionId(request.session_id.to_string()),
        fixed_step: request.fixed_step,
        delta_ns: request.delta_ns,
        session_seed: request.session_seed,
        mode: match request.mode {
            FfiRuntimeStepMode::Live => RuntimeStepMode::Live,
            FfiRuntimeStepMode::RestoreContinuation => RuntimeStepMode::RestoreContinuation,
        },
        action,
        argument,
        auxiliary,
        flag,
        input_edges: request
            .input_edges
            .into_iter()
            .map(|edge| astra_plugin_abi::RuntimeInputEdge {
                control: edge.control.to_string(),
                pressed: edge.pressed,
                value: edge.value,
                sequence: edge.sequence,
            })
            .collect(),
        await_results: request
            .await_results
            .into_iter()
            .map(|result| astra_plugin_abi::RuntimeAwaitResult {
                token_id: result.token_id.to_string(),
                status: result.status.to_string(),
                payload_len: result.payload_len,
                sequence: result.sequence,
            })
            .collect(),
        provider_results: request
            .provider_results
            .into_iter()
            .map(|result| astra_plugin_abi::RuntimeProviderResult {
                request_id: result.request_id.to_string(),
                provider_id: result.provider_id.to_string(),
                status: result.status.to_string(),
                payload_len: result.payload_len,
                sequence: result.sequence,
            })
            .collect(),
        budget: astra_plugin_abi::RuntimeStepBudget {
            max_instructions: request.max_instructions,
            max_effects: request.max_effects,
            max_trace_entries: request.max_trace_entries,
        },
    })
}

#[cfg(feature = "ffi")]
fn ffi_step_output(output: RuntimeStepOutput) -> FfiRuntimeStepResult {
    let session_id = output.session_id.0.clone();
    let mut diagnostics = output.diagnostics;
    let live = match ffi_live_output(output.live) {
        Ok((value, live_diagnostics)) => {
            diagnostics.extend(live_diagnostics);
            value
        }
        Err(error) => return ffi_step_error(session_id, error),
    };
    FfiRuntimeStepResult {
        ok: true,
        session_id: output.session_id.0.into(),
        status: output.status.into(),
        live,
        diagnostics: RVec::from(
            diagnostics
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
    }
}

#[cfg(feature = "ffi")]
fn ffi_live_output(
    value: astra_plugin_abi::RuntimeLiveOutput,
) -> Result<(FfiRuntimeLiveOutput, Vec<String>), String> {
    let mut scenes = Vec::new();
    let mut resource_scenes = Vec::new();
    let mut audio = Vec::new();
    let mut audio_commands = Vec::new();
    let mut audio_cues = Vec::new();
    let mut text = Vec::new();
    let mut text_presentations = Vec::new();
    let mut video = Vec::new();
    let mut waits = Vec::new();
    let mut events = Vec::new();
    let mut blackboard = Vec::new();
    let mut dirty_sections = Vec::new();
    for transaction in value.scenes {
        scenes.push(ffi_live_scene(transaction))
    }
    for scene in value.resource_scenes {
        {
            resource_scenes.push(FfiRuntimeResourceScene {
                sequence: scene.sequence,
                width: scene.width,
                height: scene.height,
                textures: RVec::from(
                    scene
                        .textures
                        .into_iter()
                        .map(|texture| FfiRuntimeResourceTexture {
                            texture_id: texture.texture_id,
                            resource_uri: texture.resource_uri.into(),
                            codec: texture.codec.into(),
                            revision: texture.revision,
                            decoded_width: texture.decoded_width,
                            decoded_height: texture.decoded_height,
                            decoded_format: ffi_texture_format(texture.decoded_format),
                        })
                        .collect::<Vec<_>>(),
                ),
                draws: RVec::from(
                    scene
                        .draws
                        .into_iter()
                        .map(ffi_live_draw)
                        .collect::<Vec<_>>(),
                ),
            })
        }
    }
    for packet in value.audio {
        audio.push(FfiRuntimeAudioPacket {
            sequence: packet.sequence,
            stream_id: packet.stream_id,
            sample_rate: packet.sample_rate,
            channels: packet.channels,
            pcm: match packet.pcm {
                RuntimeLivePcmBuffer::I16(samples) => FfiRuntimePcmBuffer::I16(samples.into_ffi()),
                RuntimeLivePcmBuffer::F32(samples) => FfiRuntimePcmBuffer::F32(samples.into_ffi()),
            },
        })
    }
    for command in value.audio_commands {
        {
            audio_commands.push(ffi_live_audio_command(command))
        }
    }
    for cue in value.audio_cues {
        {
            let sync = match cue.sync {
                RuntimeLiveAudioSync::None => FfiRuntimeAudioSync::None,
                RuntimeLiveAudioSync::Text => FfiRuntimeAudioSync::Text,
                RuntimeLiveAudioSync::Fence(fence_id) => FfiRuntimeAudioSync::Fence {
                    fence_id: fence_id.into(),
                },
            };
            audio_cues.push(FfiRuntimeAudioCue {
                sequence: cue.sequence,
                command_id: cue.command_id.into(),
                bus: match cue.bus {
                    RuntimeLiveAudioBus::Voice => FfiRuntimeAudioBus::Voice,
                    RuntimeLiveAudioBus::Bgm => FfiRuntimeAudioBus::Bgm,
                    RuntimeLiveAudioBus::Se => FfiRuntimeAudioBus::Se,
                    RuntimeLiveAudioBus::Movie => FfiRuntimeAudioBus::Movie,
                },
                asset: cue.asset.into(),
                looped: cue.looped,
                fade_ms: cue.fade_ms,
                sync,
            });
        }
    }
    for lease in value.text {
        text.push(FfiRuntimeTextLease {
            sequence: lease.sequence,
            lease_id: lease.lease_id.into(),
            byte_len: lease.byte_len,
            source_ref: lease.source_ref.into(),
        })
    }
    for presentation in value.text_presentations {
        {
            text_presentations.push(FfiRuntimeTextPresentation {
                sequence: presentation.sequence,
                lease_id: presentation.lease_id.into(),
                layout_id: presentation.layout_id.into(),
                language: presentation.language.into(),
                font_families: RVec::from(
                    presentation
                        .font_families
                        .into_iter()
                        .map(RString::from)
                        .collect::<Vec<_>>(),
                ),
                body: ffi_live_text_region(presentation.body),
                speaker: presentation.speaker.map(ffi_live_text_region).into(),
                rgba: presentation.rgba,
            })
        }
    }
    for command in value.video {
        video.push(match command.command {
            RuntimeLiveVideoCommandKind::Play {
                playback_id,
                resource_uri,
                mode,
                stage_width,
                stage_height,
            } => FfiRuntimeVideoCommand::Play {
                sequence: command.sequence,
                playback_id: playback_id.into(),
                resource_uri: resource_uri.into(),
                mode: match mode {
                    RuntimeLiveVideoMode::ModalWithAudio => FfiRuntimeVideoMode::ModalWithAudio,
                    RuntimeLiveVideoMode::LayerNoAudio => FfiRuntimeVideoMode::LayerNoAudio,
                },
                stage_width,
                stage_height,
            },
            RuntimeLiveVideoCommandKind::Stop { playback_id } => FfiRuntimeVideoCommand::Stop {
                sequence: command.sequence,
                playback_id: playback_id.into(),
            },
        })
    }
    for wait in value.waits {
        let kind = match wait.kind {
            RuntimeLiveWaitKind::Frame { frames } => FfiRuntimeWaitKind::Frame { frames },
            RuntimeLiveWaitKind::Time { milliseconds } => FfiRuntimeWaitKind::Time { milliseconds },
            RuntimeLiveWaitKind::Input { keys } => FfiRuntimeWaitKind::Input {
                keys: RVec::from(keys.into_iter().map(RString::from).collect::<Vec<_>>()),
            },
            RuntimeLiveWaitKind::MediaFence { media_id } => FfiRuntimeWaitKind::MediaFence {
                media_id: media_id.into(),
            },
            RuntimeLiveWaitKind::PresentationFence { fence_id } => {
                FfiRuntimeWaitKind::PresentationFence {
                    fence_id: fence_id.into(),
                }
            }
            RuntimeLiveWaitKind::ProviderCompletion { request_id } => {
                FfiRuntimeWaitKind::ProviderCompletion {
                    request_id: request_id.into(),
                }
            }
        };
        waits.push(FfiRuntimeWait {
            sequence: wait.sequence,
            token_id: wait.token_id.into(),
            kind,
        });
    }
    for event in value.events {
        events.push(FfiRuntimeEvent {
            sequence: event.sequence,
            event: event.event.into(),
            value: event.value.into(),
        })
    }
    for RuntimeLiveBlackboardMutation {
        sequence,
        key,
        value,
    } in value.blackboard
    {
        blackboard.push(FfiRuntimeBlackboardMutation {
            sequence,
            key: key.into(),
            value: value.into(),
        })
    }
    for RuntimeLiveDirtySection {
        sequence,
        section_id,
    } in value.dirty_sections
    {
        dirty_sections.push(FfiRuntimeDirtySection {
            sequence,
            section_id: section_id.into(),
        })
    }
    Ok((
        FfiRuntimeLiveOutput {
            layers: RVec::new(),
            scenes: RVec::from(scenes),
            resource_scenes: RVec::from(resource_scenes),
            audio: RVec::from(audio),
            audio_commands: RVec::from(audio_commands),
            audio_cues: RVec::from(audio_cues),
            text: RVec::from(text),
            text_presentations: RVec::from(text_presentations),
            presentations: value
                .presentations
                .into_iter()
                .map(|command| command.into_ffi())
                .collect::<Vec<_>>()
                .into(),
            timeline: value
                .timeline
                .into_iter()
                .map(|task| task.into_ffi())
                .collect::<Vec<_>>()
                .into(),
            vn_state: value.vn_state.map(|state| state.into_ffi()).into(),
            vn_step: value.vn_step.map(|step| step.into_ffi()).into(),
            video: RVec::from(video),
            waits: RVec::from(waits),
            events: RVec::from(events),
            blackboard: RVec::from(blackboard),
            dirty_sections: RVec::from(dirty_sections),
            state_revision: value.state_revision,
            instructions: value.coverage.instructions,
            syscalls: value.coverage.syscalls,
            presentation_commands: value.coverage.presentation_commands,
            audio_command_count: value.coverage.audio_commands,
            text_events: value.coverage.text_events,
            capture_bytes: value.coverage.capture_bytes,
            operation_bytes: value.coverage.operation_bytes,
            scene_moved_bytes: value.coverage.scene_moved_bytes,
            scene_copied_bytes: value.coverage.scene_copied_bytes,
            pcm_moved_bytes: value.coverage.pcm_moved_bytes,
            pcm_copied_bytes: value.coverage.pcm_copied_bytes,
        },
        value.diagnostics,
    ))
}

#[cfg(feature = "ffi")]
fn ffi_live_scene(
    transaction: astra_plugin_abi::RuntimeLiveSceneTransaction,
) -> astra_plugin_abi::FfiRuntimeSceneTransaction {
    astra_plugin_abi::FfiRuntimeSceneTransaction {
        sequence: transaction.sequence,
        width: transaction.width,
        height: transaction.height,
        compositing: match transaction.compositing {
            astra_plugin_abi::RuntimeLiveSceneCompositing::LinearSrgb => {
                astra_plugin_abi::FfiRuntimeSceneCompositing::LinearSrgb
            }
            astra_plugin_abi::RuntimeLiveSceneCompositing::EncodedSrgb => {
                astra_plugin_abi::FfiRuntimeSceneCompositing::EncodedSrgb
            }
        },
        resources: RVec::from(
            transaction
                .resources
                .into_iter()
                .map(|operation| match operation {
                    RuntimeLiveSceneResourceOperation::CreateTexture {
                        texture_id,
                        generation,
                        width,
                        height,
                        format,
                        pixels,
                    } => FfiRuntimeSceneResourceOperation::Create(FfiRuntimeSceneTextureCreate {
                        texture_id,
                        generation,
                        width,
                        height,
                        format: ffi_texture_format(format),
                        pixels: pixels.into_ffi(),
                    }),
                    RuntimeLiveSceneResourceOperation::UpdateTexture {
                        texture_id,
                        generation,
                        x,
                        y,
                        width,
                        height,
                        format,
                        pixels,
                    } => FfiRuntimeSceneResourceOperation::Update(FfiRuntimeSceneTextureUpdate {
                        texture_id,
                        generation,
                        x,
                        y,
                        width,
                        height,
                        format: ffi_texture_format(format),
                        pixels: pixels.into_ffi(),
                    }),
                    RuntimeLiveSceneResourceOperation::DestroyTexture {
                        texture_id,
                        generation,
                    } => FfiRuntimeSceneResourceOperation::Destroy {
                        texture_id,
                        generation,
                    },
                })
                .collect::<Vec<_>>(),
        ),
        draws: RVec::from(
            transaction
                .draws
                .into_iter()
                .map(|draw| astra_plugin_abi::FfiRuntimeDraw {
                    texture_id: draw.texture_id,
                    vertices: draw.vertices.map(|vertex| FfiRuntimeVertex {
                        x: vertex.x,
                        y: vertex.y,
                        u: vertex.u,
                        v: vertex.v,
                        r: vertex.color[0],
                        g: vertex.color[1],
                        b: vertex.color[2],
                        a: vertex.color[3],
                    }),
                    blend: match draw.blend {
                        RuntimeLiveBlendMode::Alpha => FfiRuntimeBlendMode::Alpha,
                        RuntimeLiveBlendMode::Additive => FfiRuntimeBlendMode::Additive,
                        RuntimeLiveBlendMode::Opaque => FfiRuntimeBlendMode::Opaque,
                        RuntimeLiveBlendMode::Multiply => FfiRuntimeBlendMode::Multiply,
                        RuntimeLiveBlendMode::Screen => FfiRuntimeBlendMode::Screen,
                    },
                    texture_filter: match draw.texture_filter {
                        astra_plugin_abi::RuntimeLiveTextureFilter::Nearest => {
                            FfiRuntimeTextureFilter::Nearest
                        }
                        astra_plugin_abi::RuntimeLiveTextureFilter::Linear => {
                            FfiRuntimeTextureFilter::Linear
                        }
                    },
                    scissor: draw
                        .scissor
                        .map(|scissor| FfiRuntimeScissor {
                            x: scissor.x,
                            y: scissor.y,
                            width: scissor.width,
                            height: scissor.height,
                        })
                        .into(),
                })
                .collect::<Vec<_>>(),
        ),
        reset_resources: transaction.reset_resources,
    }
}

#[cfg(feature = "ffi")]
fn ffi_live_draw(draw: astra_plugin_abi::RuntimeLiveDraw) -> astra_plugin_abi::FfiRuntimeDraw {
    astra_plugin_abi::FfiRuntimeDraw {
        texture_id: draw.texture_id,
        vertices: draw.vertices.map(|vertex| FfiRuntimeVertex {
            x: vertex.x,
            y: vertex.y,
            u: vertex.u,
            v: vertex.v,
            r: vertex.color[0],
            g: vertex.color[1],
            b: vertex.color[2],
            a: vertex.color[3],
        }),
        blend: match draw.blend {
            RuntimeLiveBlendMode::Alpha => FfiRuntimeBlendMode::Alpha,
            RuntimeLiveBlendMode::Additive => FfiRuntimeBlendMode::Additive,
            RuntimeLiveBlendMode::Opaque => FfiRuntimeBlendMode::Opaque,
            RuntimeLiveBlendMode::Multiply => FfiRuntimeBlendMode::Multiply,
            RuntimeLiveBlendMode::Screen => FfiRuntimeBlendMode::Screen,
        },
        texture_filter: match draw.texture_filter {
            astra_plugin_abi::RuntimeLiveTextureFilter::Nearest => FfiRuntimeTextureFilter::Nearest,
            astra_plugin_abi::RuntimeLiveTextureFilter::Linear => FfiRuntimeTextureFilter::Linear,
        },
        scissor: draw
            .scissor
            .map(|scissor| FfiRuntimeScissor {
                x: scissor.x,
                y: scissor.y,
                width: scissor.width,
                height: scissor.height,
            })
            .into(),
    }
}

#[cfg(feature = "ffi")]
fn ffi_live_text_region(region: astra_plugin_abi::RuntimeLiveTextRegion) -> FfiRuntimeTextRegion {
    FfiRuntimeTextRegion {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
        font_size: region.font_size,
        line_height: region.line_height,
        max_lines: region.max_lines,
    }
}

#[cfg(feature = "ffi")]
fn ffi_live_audio_command(command: RuntimeLiveAudioCommand) -> FfiRuntimeAudioCommand {
    match command {
        RuntimeLiveAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding,
            resource_uri,
        } => FfiRuntimeAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding: match encoding {
                RuntimeLiveAudioEncoding::Unknown => FfiRuntimeAudioEncoding::Unknown,
                RuntimeLiveAudioEncoding::Wav => FfiRuntimeAudioEncoding::Wav,
                RuntimeLiveAudioEncoding::Ogg => FfiRuntimeAudioEncoding::Ogg,
                RuntimeLiveAudioEncoding::Mp3 => FfiRuntimeAudioEncoding::Mp3,
                RuntimeLiveAudioEncoding::Flac => FfiRuntimeAudioEncoding::Flac,
            },
            resource_uri: resource_uri.into(),
        },
        RuntimeLiveAudioCommand::CreateStream {
            sequence,
            stream_id,
            sample_rate,
            channels,
            sample_format,
        } => FfiRuntimeAudioCommand::CreateStream {
            sequence,
            stream_id,
            sample_rate,
            channels,
            sample_format: match sample_format {
                RuntimeLiveAudioSampleFormat::I16 => FfiRuntimeAudioSampleFormat::I16,
                RuntimeLiveAudioSampleFormat::F32 => FfiRuntimeAudioSampleFormat::F32,
            },
        },
        RuntimeLiveAudioCommand::SubmitI16 {
            sequence,
            stream_id,
            samples,
        } => FfiRuntimeAudioCommand::SubmitI16 {
            sequence,
            stream_id,
            samples: samples.into_ffi(),
        },
        RuntimeLiveAudioCommand::SubmitF32 {
            sequence,
            stream_id,
            samples,
        } => FfiRuntimeAudioCommand::SubmitF32 {
            sequence,
            stream_id,
            samples: samples.into_ffi(),
        },
        RuntimeLiveAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => FfiRuntimeAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        },
        RuntimeLiveAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        } => FfiRuntimeAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        },
        RuntimeLiveAudioCommand::Pause {
            sequence,
            stream_id,
        } => FfiRuntimeAudioCommand::Pause {
            sequence,
            stream_id,
        },
        RuntimeLiveAudioCommand::Resume {
            sequence,
            stream_id,
        } => FfiRuntimeAudioCommand::Resume {
            sequence,
            stream_id,
        },
        RuntimeLiveAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        } => FfiRuntimeAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        },
        RuntimeLiveAudioCommand::DestroyStream {
            sequence,
            stream_id,
        } => FfiRuntimeAudioCommand::DestroyStream {
            sequence,
            stream_id,
        },
        RuntimeLiveAudioCommand::MasterVolume { sequence, volume } => {
            FfiRuntimeAudioCommand::MasterVolume { sequence, volume }
        }
    }
}

#[cfg(feature = "ffi")]
fn ffi_texture_format(format: RuntimeLiveTextureFormat) -> FfiRuntimeTextureFormat {
    match format {
        RuntimeLiveTextureFormat::Rgba8 => FfiRuntimeTextureFormat::Rgba8,
        RuntimeLiveTextureFormat::LumaAlpha8 => FfiRuntimeTextureFormat::LumaAlpha8,
    }
}

#[cfg(feature = "ffi")]
fn ffi_section_result(section: RuntimeSectionPayload) -> FfiRuntimeSectionResult {
    FfiRuntimeSectionResult {
        section_id: section.section_id.into(),
        schema: section.schema.into(),
        version_major: section.version.major,
        version_minor: section.version.minor,
        version_patch: section.version.patch,
        codec: match section.codec {
            RuntimeSectionCodec::Postcard => FfiRuntimeSectionCodec::Postcard,
            RuntimeSectionCodec::Raw => FfiRuntimeSectionCodec::Raw,
            RuntimeSectionCodec::Zstd => FfiRuntimeSectionCodec::Zstd,
        },
        bytes: RVec::from(section.bytes),
    }
}

#[cfg(feature = "ffi")]
fn ffi_save_result(value: RuntimeSaveSections) -> FfiRuntimeSaveResult {
    FfiRuntimeSaveResult {
        ok: true,
        session_id: value.session_id.0.into(),
        sections: RVec::from(
            value
                .sections
                .into_iter()
                .map(ffi_section_result)
                .collect::<Vec<_>>(),
        ),
        diagnostics: RVec::from(
            value
                .diagnostics
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
    }
}

#[cfg(feature = "ffi")]
fn ffi_save_error(session_id: String, message: String) -> FfiRuntimeSaveResult {
    FfiRuntimeSaveResult {
        ok: false,
        session_id: session_id.into(),
        sections: RVec::new(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}

#[cfg(feature = "ffi")]
fn ffi_restore_error(session_id: String, message: String) -> FfiRuntimeRestoreResult {
    FfiRuntimeRestoreResult {
        ok: false,
        session_id: session_id.into(),
        restored_fixed_step: 0,
        session_seed: 0,
        status: "error".into(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}

#[cfg(feature = "ffi")]
fn ffi_shutdown_error(session_id: String, message: String) -> FfiRuntimeShutdownResult {
    FfiRuntimeShutdownResult {
        ok: false,
        session_id: session_id.into(),
        status: "error".into(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}

#[cfg(feature = "ffi")]
fn ffi_open_error(message: String) -> FfiRuntimeOpenResult {
    FfiRuntimeOpenResult {
        ok: false,
        session_handle: 0,
        session_id: RString::new(),
        runtime_id: NATIVE_VN_RUNTIME_ID.into(),
        provider_id: NATIVE_VN_PROVIDER_ID.into(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}

#[cfg(feature = "ffi")]
fn ffi_step_error(session_id: String, message: String) -> FfiRuntimeStepResult {
    FfiRuntimeStepResult {
        ok: false,
        session_id: session_id.into(),
        status: "error".into(),
        live: FfiRuntimeLiveOutput::empty(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}
