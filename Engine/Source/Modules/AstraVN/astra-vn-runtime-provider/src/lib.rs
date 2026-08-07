//! Native AstraVN gameplay runtime provider and ABI-safe FFI adapter.

#[cfg(feature = "ffi")]
use std::sync::OnceLock;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

#[cfg(feature = "ffi")]
use abi_stable::std_types::{RString, RVec};
use astra_core::{Hash128, SchemaVersion};
use astra_plugin::{ProductRuntimeProvider, ProductRuntimeProviderFactory, ProductRuntimeSession};
#[cfg(feature = "ffi")]
use astra_plugin_abi::{
    FfiRuntimeAudioBus, FfiRuntimeAudioCommand, FfiRuntimeAudioCommandKind, FfiRuntimeAudioCue,
    FfiRuntimeAudioEncoding, FfiRuntimeAudioPacket, FfiRuntimeAudioSampleFormat,
    FfiRuntimeAudioSyncKind, FfiRuntimeBlackboardMutation, FfiRuntimeBlendMode,
    FfiRuntimeDirtySection, FfiRuntimeEditorMetadataResult, FfiRuntimeEvent,
    FfiRuntimeInstanceRequest, FfiRuntimeIntegrityMode, FfiRuntimeLiveOutput,
    FfiRuntimeOpenRequest, FfiRuntimeOpenResult, FfiRuntimePackageSectionsResult,
    FfiRuntimePcmBuffer, FfiRuntimePersistedOutput, FfiRuntimePrepareRequest,
    FfiRuntimeProbeRequest, FfiRuntimeProviderRegistration, FfiRuntimeReleaseChecksResult,
    FfiRuntimeReportResult, FfiRuntimeResourceScene, FfiRuntimeResourceTexture,
    FfiRuntimeRestoreRequest, FfiRuntimeRestoreResult, FfiRuntimeSaveRequest, FfiRuntimeSaveResult,
    FfiRuntimeSceneResourceOperation, FfiRuntimeSceneTextureCreate, FfiRuntimeSceneTextureUpdate,
    FfiRuntimeScissor, FfiRuntimeSection, FfiRuntimeSectionCodec, FfiRuntimeSectionResult,
    FfiRuntimeShutdownRequest, FfiRuntimeShutdownResult, FfiRuntimeStepMode, FfiRuntimeStepRequest,
    FfiRuntimeStepResult, FfiRuntimeTextLease, FfiRuntimeTextPresentation, FfiRuntimeTextRegion,
    FfiRuntimeTextureFormat, FfiRuntimeVertex, FfiRuntimeVideoCommand, FfiRuntimeVideoCommandKind,
    FfiRuntimeVideoMode, FfiRuntimeWait, FfiRuntimeWaitKind, PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA,
    PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ReleaseCheckDescriptor, RuntimeEditorMetadata,
    RuntimeExecutorKind, RuntimeLiveAudioBus, RuntimeLiveAudioCue, RuntimeLiveAudioSync,
    RuntimeLiveCoverage, RuntimeOpenReport, RuntimeOpenRequest, RuntimeOutputDomain,
    RuntimeOutputSchemaDescriptor, RuntimePackageSectionPlan, RuntimePersistedCodec,
    RuntimePersistedOutput, RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport,
    RuntimeProbeRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionCodec,
    RuntimeSectionPayload, RuntimeSectionRef, RuntimeShutdownReport, RuntimeStepInput,
    RuntimeStepMode, RuntimeStepOutput, RuntimeTickIntegrityMode, GAME_RUNTIME_PROVIDER_SLOT,
    NATIVE_VN_PROVIDER_ID, NATIVE_VN_RUNTIME_ID, RUNTIME_EDITOR_METADATA_SCHEMA,
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
    PlayerInput, PresentationCommand as RuntimePresentationCommand, RuntimeAction,
    RuntimeComponentPayload, RuntimeConfig, RuntimeError, RuntimeEvent, RuntimeSnapshot,
    RuntimeWorld, SaveBlob, SaveRequest, StateDefinition, StateMachineDefinition, TickIngress,
    TickInput, TickIntegrityMode, TickRequest, TransitionDefinition,
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

const VN_DISABLED_STATE_HASH: Hash128 = Hash128::from_bytes([0; 16]);

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

fn output_schema(
    domain: RuntimeOutputDomain,
    schema: &str,
    major: u16,
) -> RuntimeOutputSchemaDescriptor {
    RuntimeOutputSchemaDescriptor {
        domain,
        schema: schema.to_string(),
        version: SchemaVersion::new(major, 0, 0),
        codec: RuntimePersistedCodec::Postcard,
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
    owner: ActorId,
    vn_component: ComponentId,
    compiled: Arc<CoreCompiledStory>,
    runtime_index: Arc<CoreVnRuntimeIndex>,
    output: Arc<Mutex<Option<VnStepOutput>>>,
    state_cache: Arc<Mutex<Option<VnStepStateCache>>>,
    step_complexity: Arc<Mutex<Option<VnStepComplexityMetrics>>>,
}

struct VnStepAction {
    owner: ActorId,
    component: ComponentId,
    compiled: Arc<CoreCompiledStory>,
    runtime_index: Arc<CoreVnRuntimeIndex>,
    output: Arc<Mutex<Option<VnStepOutput>>>,
    state_cache: Arc<Mutex<Option<VnStepStateCache>>>,
    step_complexity: Arc<Mutex<Option<VnStepComplexityMetrics>>>,
}

#[derive(Clone)]
struct VnStepStateCache {
    state_hash: Hash128,
    state: VnRuntimeState,
}

const VN_RUNTIME_HOT_STATE_SCHEMA: &str = "astra.vn.runtime_hot_state.v3";
const VN_RUNTIME_HISTORY_CHUNK_SCHEMA: &str = "astra.vn.runtime_history_chunk.v3";
const VN_HISTORY_CHUNK_CAPACITY: usize = 64;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct VnRuntimeHotStateV3 {
    schema: String,
    state: VnRuntimeState,
    read_state_bits: Vec<u64>,
    route_coverage_bits: Vec<u64>,
    backlog_count: usize,
    tail_chunk: ComponentId,
    backlog_root: Hash128,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct VnRuntimeHistoryChunkV3 {
    schema: String,
    previous: Option<ComponentId>,
    previous_root: Hash128,
    entries: Vec<astra_vn_core::BacklogEntry>,
    root: Hash128,
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

fn empty_backlog_root() -> Hash128 {
    Hash128::from_blake3(b"astra.vn.backlog.root.v3")
}

fn backlog_chunk_root(
    previous_root: Hash128,
    entries: &[astra_vn_core::BacklogEntry],
) -> Result<Hash128, CoreVnError> {
    Ok(Hash128::from_blake3(&postcard::to_allocvec(&(
        "astra.vn.backlog.chunk.v3",
        previous_root,
        entries,
    ))?))
}

fn hot_state_from_runtime(
    state: &VnRuntimeState,
    index: &CoreVnRuntimeIndex,
    tail_chunk: ComponentId,
    backlog_root: Hash128,
) -> Result<VnRuntimeHotStateV3, CoreVnError> {
    let mut hot = state.clone();
    hot.backlog.clear();
    hot.read_state.clear();
    hot.voice_replay.clear();
    hot.route_coverage.clear();
    Ok(VnRuntimeHotStateV3 {
        schema: VN_RUNTIME_HOT_STATE_SCHEMA.to_string(),
        state: hot,
        read_state_bits: index.encode_read_state(&state.read_state)?,
        route_coverage_bits: index.encode_route_coverage(&state.route_coverage)?,
        backlog_count: state.backlog.len(),
        tail_chunk,
        backlog_root,
    })
}

fn validate_hot_state(hot: &VnRuntimeHotStateV3) -> Result<(), CoreVnError> {
    if hot.schema != VN_RUNTIME_HOT_STATE_SCHEMA
        || !hot.state.backlog.is_empty()
        || !hot.state.read_state.is_empty()
        || !hot.state.voice_replay.is_empty()
        || !hot.state.route_coverage.is_empty()
    {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_HOT_STATE_INVALID",
            "VN hot state contains an invalid schema or duplicated cold history",
        ));
    }
    Ok(())
}

fn materialize_runtime_state(
    hot: &VnRuntimeHotStateV3,
    index: &CoreVnRuntimeIndex,
    mut read_chunk: impl FnMut(ComponentId) -> Result<VnRuntimeHistoryChunkV3, CoreVnError>,
) -> Result<VnRuntimeState, CoreVnError> {
    validate_hot_state(hot)?;
    let mut chunk_id = Some(hot.tail_chunk);
    let mut reversed = Vec::new();
    let mut visited = BTreeSet::new();
    while let Some(id) = chunk_id {
        if !visited.insert(id) {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_CYCLE",
                "VN history chunk chain contains a cycle",
            ));
        }
        let chunk = read_chunk(id)?;
        if chunk.schema != VN_RUNTIME_HISTORY_CHUNK_SCHEMA
            || chunk.entries.len() > VN_HISTORY_CHUNK_CAPACITY
        {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_CHUNK_INVALID",
                "VN history chunk schema or entry count is invalid",
            ));
        }
        chunk_id = chunk.previous;
        reversed.push(chunk);
    }
    reversed.reverse();
    let mut root = empty_backlog_root();
    let mut backlog = Vec::with_capacity(hot.backlog_count);
    for chunk in reversed {
        if chunk.previous_root != root {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_ROOT_MISMATCH",
                "VN history chunk previous root does not match the chain",
            ));
        }
        let expected = backlog_chunk_root(root, &chunk.entries)?;
        if chunk.root != expected {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_ROOT_MISMATCH",
                "VN history chunk root does not match its entries",
            ));
        }
        root = expected;
        backlog.extend(chunk.entries);
    }
    if backlog.len() != hot.backlog_count || root != hot.backlog_root {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_HISTORY_ROOT_MISMATCH",
            "VN hot state does not match the materialized history chain",
        ));
    }
    let mut state = hot.state.clone();
    state.backlog = backlog;
    state.read_state = index.decode_read_state(&hot.read_state_bits)?;
    state.route_coverage = index.decode_route_coverage(&hot.route_coverage_bits)?;
    state.voice_replay = state
        .backlog
        .iter()
        .filter_map(|entry| {
            entry.voice.as_ref().map(|voice| {
                (
                    voice.clone(),
                    astra_vn_core::VoiceReplayEntry {
                        voice: voice.clone(),
                        line_key: entry.key.clone(),
                        speaker: entry.speaker.clone(),
                    },
                )
            })
        })
        .collect();
    Ok(state)
}

fn materialize_session_state(session: &NativeVnSession) -> Result<VnRuntimeState, CoreVnError> {
    if let Some(state) = session
        .state_cache
        .lock()
        .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))?
        .as_ref()
        .map(|cached| cached.state.clone())
    {
        return Ok(state);
    }
    let bytes = session
        .world
        .read_component_postcard_bytes(session.vn_component)
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    let hot: VnRuntimeHotStateV3 = postcard::from_bytes(&bytes)
        .map_err(|error| CoreVnError::message(format!("decode VN hot runtime state: {error}")))?;
    materialize_runtime_state(&hot, &session.runtime_index, |component_id| {
        session
            .world
            .read_component(component_id)
            .map_err(|error| CoreVnError::message(error.to_string()))
    })
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
    let payload = RuntimeComponentPayload::postcard(
        VN_RUNTIME_STATE_SCHEMA,
        SchemaVersion::new(VN_RUNTIME_STATE_SCHEMA_MAJOR, 0, 0),
        &state,
    )
    .map_err(|error| CoreVnError::message(error.to_string()))?;
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
    let hot_state = materialize_session_state(session)?;
    if state != hot_state {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_RESTORE_STATE_MISMATCH",
            "materialized VN state does not match the hot/cold Runtime state",
        ));
    }
    Ok(state)
}

fn replace_session_state(
    session: &mut NativeVnSession,
    state: VnRuntimeState,
) -> Result<(), CoreVnError> {
    let checkpoint = session.world.snapshot();
    let cached = session
        .state_cache
        .lock()
        .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))?
        .clone();
    match replace_session_state_inner(session, state) {
        Ok(()) => Ok(()),
        Err(error) => {
            session.world.restore_snapshot(checkpoint);
            *session
                .state_cache
                .lock()
                .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))? =
                cached;
            Err(error)
        }
    }
}

fn replace_session_state_inner(
    session: &mut NativeVnSession,
    state: VnRuntimeState,
) -> Result<(), CoreVnError> {
    let old_hot: VnRuntimeHotStateV3 = session
        .world
        .read_component(session.vn_component)
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    validate_hot_state(&old_hot)?;
    let mut chunk_id = Some(old_hot.tail_chunk);
    let mut old_chunks = Vec::new();
    let mut visited = BTreeSet::new();
    while let Some(id) = chunk_id {
        if !visited.insert(id) {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_CYCLE",
                "VN history chunk chain contains a cycle",
            ));
        }
        let chunk: VnRuntimeHistoryChunkV3 = session
            .world
            .read_component(id)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        chunk_id = chunk.previous;
        old_chunks.push(id);
    }

    let mut previous = None;
    let mut previous_root = empty_backlog_root();
    let mut tail = None;
    if state.backlog.is_empty() {
        let root = backlog_chunk_root(previous_root, &[])?;
        let chunk = VnRuntimeHistoryChunkV3 {
            schema: VN_RUNTIME_HISTORY_CHUNK_SCHEMA.to_string(),
            previous,
            previous_root,
            entries: vec![],
            root,
        };
        tail = Some(
            session
                .world
                .attach_component(session.owner, VN_RUNTIME_HISTORY_CHUNK_SCHEMA, &chunk)
                .map_err(|error| CoreVnError::message(error.to_string()))?,
        );
        previous_root = root;
    } else {
        for entries in state.backlog.chunks(VN_HISTORY_CHUNK_CAPACITY) {
            let root = backlog_chunk_root(previous_root, entries)?;
            let chunk = VnRuntimeHistoryChunkV3 {
                schema: VN_RUNTIME_HISTORY_CHUNK_SCHEMA.to_string(),
                previous,
                previous_root,
                entries: entries.to_vec(),
                root,
            };
            let id = session
                .world
                .attach_component(session.owner, VN_RUNTIME_HISTORY_CHUNK_SCHEMA, &chunk)
                .map_err(|error| CoreVnError::message(error.to_string()))?;
            previous = Some(id);
            tail = Some(id);
            previous_root = root;
        }
    }
    let hot = hot_state_from_runtime(
        &state,
        &session.runtime_index,
        tail.expect("history installation always creates a tail chunk"),
        previous_root,
    )?;
    session
        .world
        .replace_component(session.vn_component, &hot)
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    for id in old_chunks {
        if !session.world.detach_component(id) {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_DETACH_FAILED",
                "VN history chunk disappeared during save-slot replacement",
            ));
        }
    }
    let state_hash = Hash128::from_blake3(&postcard::to_allocvec(&hot)?);
    *session
        .state_cache
        .lock()
        .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))? =
        Some(VnStepStateCache { state_hash, state });
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
        let hot_bytes = session
            .world
            .read_component_postcard_bytes(session.vn_component)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let hot: VnRuntimeHotStateV3 = postcard::from_bytes(&hot_bytes)
            .map_err(|error| CoreVnError::message(format!("decode VN hot state: {error}")))?;
        validate_hot_state(&hot)?;
        let mut chunk_count = 0_usize;
        let mut chunk_id = Some(hot.tail_chunk);
        let mut visited = BTreeSet::new();
        let mut tail_chunk_bytes = 0_usize;
        while let Some(id) = chunk_id {
            if !visited.insert(id) {
                return Err(CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_HISTORY_CYCLE",
                    "VN history chunk chain contains a cycle",
                ));
            }
            let bytes = session
                .world
                .read_component_postcard_bytes(id)
                .map_err(|error| CoreVnError::message(error.to_string()))?;
            if chunk_count == 0 {
                tail_chunk_bytes = bytes.len();
            }
            let chunk: VnRuntimeHistoryChunkV3 = postcard::from_bytes(&bytes)
                .map_err(|error| CoreVnError::message(format!("decode VN history: {error}")))?;
            chunk_id = chunk.previous;
            chunk_count += 1;
        }
        Ok(VnRuntimeStorageMetrics {
            schema: "astra.vn.runtime_storage_metrics.v3".to_string(),
            backlog_count: hot.backlog_count,
            history_chunk_count: chunk_count,
            hot_state_bytes: hot_bytes.len(),
            tail_chunk_bytes,
        })
    }

    pub fn step_complexity_metrics(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<VnStepComplexityMetrics, CoreVnError> {
        self.session(session_id)?
            .step_complexity
            .lock()
            .map_err(|_| CoreVnError::message("VN step complexity lock is poisoned"))?
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
                    "astra.vn.presentation_command.v2",
                    2,
                ),
                output_schema(RuntimeOutputDomain::Audio, "astra.vn.audio_command.v2", 2),
                output_schema(RuntimeOutputDomain::Effect, "astra.vn.timeline_task.v1", 1),
                output_schema(
                    RuntimeOutputDomain::Trace,
                    "astra.vn.runtime_step_trace.v1",
                    1,
                ),
                output_schema(
                    RuntimeOutputDomain::Trace,
                    VN_RUNTIME_VIEW_STATE_SCHEMA,
                    VN_RUNTIME_VIEW_STATE_SCHEMA_MAJOR,
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
        let empty_previous_root = empty_backlog_root();
        let empty_root = backlog_chunk_root(empty_previous_root, &[])?;
        let initial_chunk = VnRuntimeHistoryChunkV3 {
            schema: VN_RUNTIME_HISTORY_CHUNK_SCHEMA.to_string(),
            previous: None,
            previous_root: empty_previous_root,
            entries: Vec::new(),
            root: empty_root,
        };
        let tail_chunk = world
            .attach_component(owner, VN_RUNTIME_HISTORY_CHUNK_SCHEMA, &initial_chunk)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let initial_hot =
            hot_state_from_runtime(&initial_state, &runtime_index, tail_chunk, empty_root)?;
        let vn_component = world
            .attach_component(owner, VN_RUNTIME_HOT_STATE_SCHEMA, &initial_hot)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        world
            .attach_component(owner, "astra.vn.policy_state.v1", &VnPolicyState::default())
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let output = Arc::new(Mutex::new(None));
        let initial_state_hash = if integrity_mode == TickIntegrityMode::Evidence {
            Hash128::from_blake3(&postcard::to_allocvec(&initial_hot)?)
        } else {
            VN_DISABLED_STATE_HASH
        };
        let state_cache = Arc::new(Mutex::new(Some(VnStepStateCache {
            state_hash: initial_state_hash,
            state: initial_state,
        })));
        let step_complexity = Arc::new(Mutex::new(None));
        world
            .register_action(
                NATIVE_VN_PROVIDER_ID,
                VnStepAction {
                    owner,
                    component: vn_component,
                    compiled: Arc::clone(&compiled),
                    runtime_index: Arc::clone(&runtime_index),
                    output: Arc::clone(&output),
                    state_cache: Arc::clone(&state_cache),
                    step_complexity: Arc::clone(&step_complexity),
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
                vn_component,
                compiled,
                runtime_index,
                output,
                state_cache,
                step_complexity,
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
        *session
            .output
            .lock()
            .map_err(|_| CoreVnError::message("VN step output lock is poisoned"))? = None;
        let event_kind = vn_event_kind(&command).to_string();
        let cached_command_state = session
            .state_cache
            .lock()
            .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))?
            .as_ref()
            .map(|cached| {
                (
                    cached.state.pending_wait.clone(),
                    cached.state.system.reading_mode,
                )
            });
        let (pending_wait, reading_mode) = if let Some(cached) = cached_command_state {
            cached
        } else {
            let state = materialize_session_state(session)?;
            (state.pending_wait, state.system.reading_mode)
        };
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
        let runtime_view_state = {
            let cache = session
                .state_cache
                .lock()
                .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))?;
            let cached = cache.as_ref().ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_VIEW_STATE_MISSING",
                    "VN step did not retain its validated state for the Player view",
                )
            })?;
            runtime_view_state(&cached.state, cached.state_hash)
        };
        let mut media = Vec::with_capacity(output.presentation.len() + output.audio.len());
        let mut audio_cues = Vec::with_capacity(output.audio.len());
        let mut audio = output.audio.iter();
        for (presentation_index, command) in output.presentation.iter().enumerate() {
            let sequence = presentation_index
                .checked_add(1)
                .and_then(|index| u64::try_from(index).ok())
                .ok_or_else(|| CoreVnError::message("VN presentation sequence overflow"))?;
            media.push(
                RuntimePersistedOutput::postcard(
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v2",
                    SchemaVersion::new(2, 0, 0),
                    command,
                )
                .map_err(|err| CoreVnError::message(err.to_string()))?,
            );
            if matches!(command, PresentationCommand::Stage(StageCommand::Audio(_))) {
                let audio_command = audio.next().ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_AUDIO_ORDER_MISSING",
                        "typed audio presentation has no matching audio output",
                    )
                })?;
                audio_cues.push(runtime_live_audio_cue(sequence, audio_command));
            }
        }
        if audio.next().is_some() {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_AUDIO_ORDER_EXTRA",
                "audio output has no matching typed presentation command",
            ));
        }
        let timeline = output
            .timeline_tasks
            .iter()
            .map(|task| {
                RuntimePersistedOutput::postcard(
                    RuntimeOutputDomain::Effect,
                    "astra.vn.timeline_task.v1",
                    SchemaVersion::new(1, 0, 0),
                    task,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let effects = vec![RuntimePersistedOutput::postcard(
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
            RuntimePersistedOutput::postcard(
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
            RuntimePersistedOutput::postcard(
                RuntimeOutputDomain::Trace,
                VN_RUNTIME_VIEW_STATE_SCHEMA,
                SchemaVersion::new(VN_RUNTIME_VIEW_STATE_SCHEMA_MAJOR, 0, 0),
                &runtime_view_state,
            )
            .map_err(|err| CoreVnError::message(err.to_string()))?,
        ];
        Ok(RuntimeStepOutput {
            session_id,
            status: if output.presentation.is_empty() {
                "idle".to_string()
            } else {
                "blocked".to_string()
            },
            live: astra_plugin_abi::RuntimeLiveOutput {
                state_revision: fixed_step,
                coverage: RuntimeLiveCoverage {
                    presentation_commands: output.presentation.len() as u64,
                    audio_commands: output.audio.len() as u64,
                    ..RuntimeLiveCoverage::default()
                },
                audio_cues,
                ..astra_plugin_abi::RuntimeLiveOutput::default()
            },
            persisted: effects
                .into_iter()
                .chain(media)
                .chain(timeline)
                .chain(trace)
                .collect(),
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

    pub fn runtime_hashes(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<(Hash128, Hash128, Hash128), CoreVnError> {
        let world = &self.session(session_id)?.world;
        if world.tick_integrity_mode() == TickIntegrityMode::Shipping {
            return Ok((
                VN_DISABLED_STATE_HASH,
                VN_DISABLED_STATE_HASH,
                VN_DISABLED_STATE_HASH,
            ));
        }
        Ok((
            world.state_hash(),
            world.event_hash(),
            world.presentation_hash(),
        ))
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
        let actual_hash = Hash128::from_blake3(&postcard::to_allocvec(&save.state)?);
        if actual_hash != save.state_hash {
            return Err(CoreVnError::diagnostic(
                "ASTRA_VN_SAVE_STATE_HASH",
                "AstraVN save slot state hash does not match its payload",
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
        *session
            .state_cache
            .lock()
            .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))? = None;
        let state = consume_materialized_restore_state(session)?;
        let bytes = session
            .world
            .read_component_postcard_bytes(session.vn_component)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let state_hash = Hash128::from_blake3(&bytes);
        *session
            .state_cache
            .lock()
            .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))? =
            Some(VnStepStateCache { state_hash, state });
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

fn command_from_event(event: &RuntimeEvent) -> Result<CoreVnPlayerCommand, RuntimeError> {
    let data = &event.payload.data;
    let value = |key: &str| -> Result<String, RuntimeError> {
        match data.get(key) {
            Some(BlackboardValue::String(value)) => Ok(value.clone()),
            _ => Err(command_event_error(
                "ASTRA_VN_STEP_ARGUMENT_MISSING",
                format!("event {} is missing typed field {key}", event.payload.kind),
            )),
        }
    };
    let flag = |key: &str| -> Result<bool, RuntimeError> {
        match data.get(key) {
            Some(BlackboardValue::Bool(value)) => Ok(*value),
            _ => Err(command_event_error(
                "ASTRA_VN_STEP_FLAG_MISSING",
                format!("event {} is missing typed flag {key}", event.payload.kind),
            )),
        }
    };
    match event.payload.kind.as_str() {
        "vn.launch" => Ok(CoreVnPlayerCommand::Launch {
            story_id: value("story_id")?,
            state_id: value("state_id")?,
        }),
        "player.advance" => Ok(CoreVnPlayerCommand::Advance),
        "choice.selected" => Ok(CoreVnPlayerCommand::Choose {
            option_id: value("option_id")?,
        }),
        "system.open" => Ok(CoreVnPlayerCommand::OpenSystem {
            page: event_page(&value("page")?)?,
        }),
        "system.switch" => Ok(CoreVnPlayerCommand::SwitchSystemPage {
            page: event_page(&value("page")?)?,
        }),
        "system.return" => Ok(CoreVnPlayerCommand::ReturnSystem),
        "voice.replay" => Ok(CoreVnPlayerCommand::ReplayVoice {
            voice: value("voice")?,
        }),
        "system.auto" => Ok(CoreVnPlayerCommand::SetAuto {
            enabled: flag("enabled")?,
        }),
        "system.skip" => Ok(CoreVnPlayerCommand::SetSkip {
            mode: event_skip_mode(&value("mode")?)?,
        }),
        "system.reading_mode" => Ok(CoreVnPlayerCommand::SetReadingMode {
            mode: event_reading_mode(&value("mode")?)?,
        }),
        "system.audio_enabled" => Ok(CoreVnPlayerCommand::SetAudioEnabled {
            enabled: flag("enabled")?,
        }),
        "system.action" => Ok(CoreVnPlayerCommand::InvokeSystemAction {
            action_id: value("action_id")?,
        }),
        "system.config" => Ok(CoreVnPlayerCommand::SetConfig {
            key: value("key")?,
            value: value("value")?,
        }),
        "system.replay.start" => Ok(CoreVnPlayerCommand::StartReplay {
            replay_id: value("replay_id")?,
        }),
        "system.gallery.preview" => Ok(CoreVnPlayerCommand::PreviewGallery {
            item_id: value("item_id")?,
        }),
        "system.route.jump" => Ok(CoreVnPlayerCommand::JumpRoute {
            node_id: value("node_id")?,
        }),
        "system.backlog.jump" => Ok(CoreVnPlayerCommand::JumpBacklog {
            command_id: value("command_id")?,
        }),
        "system.text.submit" => Ok(CoreVnPlayerCommand::SubmitText {
            input_id: value("input_id")?,
            value: value("value")?,
        }),
        "system.unlock" => Ok(CoreVnPlayerCommand::Unlock {
            kind: event_unlock_kind(&value("kind")?)?,
            id: value("id")?,
        }),
        "await.completed" => Ok(CoreVnPlayerCommand::CompleteWait {
            fence: value("fence")?,
        }),
        other => Err(command_event_error(
            "ASTRA_VN_STEP_EVENT_UNKNOWN",
            format!("unsupported typed VN event {other}"),
        )),
    }
}

fn command_event_error(code: &str, message: String) -> RuntimeError {
    RuntimeError::diagnostic(astra_core::Diagnostic::blocking(code, message))
}

fn event_page(value: &str) -> Result<SystemPageKind, RuntimeError> {
    let page = SystemPageKind::parse(value);
    (page != SystemPageKind::Unknown)
        .then_some(page)
        .ok_or_else(|| command_event_error("ASTRA_VN_STEP_PAGE_UNKNOWN", value.to_string()))
}

fn event_skip_mode(value: &str) -> Result<SkipMode, RuntimeError> {
    match value {
        "none" => Ok(SkipMode::None),
        "read" => Ok(SkipMode::Read),
        "all" => Ok(SkipMode::All),
        _ => Err(command_event_error(
            "ASTRA_VN_STEP_SKIP_MODE_UNKNOWN",
            value.to_string(),
        )),
    }
}

fn event_reading_mode(value: &str) -> Result<ReadingMode, RuntimeError> {
    match value {
        "hidden" => Ok(ReadingMode::Hidden),
        "manual" => Ok(ReadingMode::Manual),
        "fast_forward" => Ok(ReadingMode::FastForward),
        _ => Err(command_event_error(
            "ASTRA_VN_STEP_READING_MODE_UNKNOWN",
            value.to_string(),
        )),
    }
}

fn event_unlock_kind(value: &str) -> Result<SystemUnlockKind, RuntimeError> {
    match value {
        "gallery" => Ok(SystemUnlockKind::Gallery),
        "replay" => Ok(SystemUnlockKind::Replay),
        _ => Err(command_event_error(
            "ASTRA_VN_STEP_UNLOCK_KIND_UNKNOWN",
            value.to_string(),
        )),
    }
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

fn runtime_view_state(
    state: &VnRuntimeState,
    authoritative_state_hash: Hash128,
) -> VnRuntimeViewState {
    let active_page = state.system_stack.last().map(|frame| frame.page);
    let backlog = if active_page == Some(SystemPageKind::Backlog) {
        state.backlog.clone()
    } else {
        state.backlog.last().cloned().into_iter().collect()
    };
    let voice_replay = if active_page == Some(SystemPageKind::VoiceReplay) {
        state.voice_replay.clone()
    } else {
        BTreeMap::new()
    };
    let expose_route_history =
        active_page == Some(SystemPageKind::RouteChart) || state.cursor.is_none();
    VnRuntimeViewState {
        schema: VN_RUNTIME_VIEW_STATE_SCHEMA.to_string(),
        authoritative_state_hash,
        backlog_count: state.backlog.len(),
        state: VnRuntimeState {
            schema: state.schema.clone(),
            instance_id: state.instance_id.clone(),
            profile: state.profile.clone(),
            locale: state.locale.clone(),
            cursor: state.cursor.clone(),
            call_stack: state.call_stack.clone(),
            system_stack: state.system_stack.clone(),
            system: state.system.clone(),
            pending_choice: state.pending_choice.clone(),
            variables: state.variables.clone(),
            backlog,
            read_state: Default::default(),
            voice_replay,
            route_coverage: if expose_route_history {
                state.route_coverage.clone()
            } else {
                Default::default()
            },
            route_flags: if expose_route_history {
                state.route_flags.clone()
            } else {
                Default::default()
            },
            wait_sequence: state.wait_sequence,
            pending_wait: state.pending_wait.clone(),
        },
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
                [ActionResourceKey::ActorStore],
                [
                    ActionResourceKey::ActorStore,
                    ActionResourceKey::AwaitQueue,
                    ActionResourceKey::EventQueue,
                    ActionResourceKey::Presentation,
                    ActionResourceKey::MutationLog,
                    ActionResourceKey::EffectTrace,
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
        let evidence_mode = ctx.evidence_mode();
        let profile = tracing::enabled!(tracing::Level::TRACE);
        let command_started = profile.then(Instant::now);
        let event = ctx.trigger_event().ok_or_else(|| {
            RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                "ASTRA_VN_STEP_TRIGGER_MISSING",
                "astra.vn.step requires a trigger event",
            ))
        })?;
        let event_kind = event.payload.kind.clone();
        let command = command_from_event(event)?;
        let command_mapping_ns = profile_elapsed_ns(command_started);
        let state_started = profile.then(Instant::now);
        let previous_state_bytes = ctx.read_component_postcard_bytes(self.component)?;
        let previous_hot: VnRuntimeHotStateV3 = postcard::from_bytes(&previous_state_bytes)
            .map_err(|error| {
                RuntimeError::message(format!("decode VN hot runtime state: {error}"))
            })?;
        validate_hot_state(&previous_hot)
            .map_err(|error| RuntimeError::message(error.to_string()))?;
        let cached_state = self
            .state_cache
            .lock()
            .map_err(|_| RuntimeError::message("VN step state cache lock is poisoned"))?
            .take();
        let previous_state_hash = if evidence_mode {
            cached_state.as_ref().map_or_else(
                || Hash128::from_blake3(&previous_state_bytes),
                |cached| cached.state_hash,
            )
        } else {
            VN_DISABLED_STATE_HASH
        };
        let state_cache_hit = cached_state.is_some();
        let materialized_history_entries = if state_cache_hit {
            0
        } else {
            previous_hot.backlog_count
        };
        let decoded_state = if cached_state.is_none() {
            Some(
                materialize_runtime_state(&previous_hot, &self.runtime_index, |component_id| {
                    ctx.read_component(component_id)
                        .map_err(|error| CoreVnError::message(error.to_string()))
                })
                .map_err(|error| RuntimeError::message(error.to_string()))?,
            )
        } else {
            None
        };
        let previous_wait = if let Some(cached) = &cached_state {
            cached.state.pending_wait.clone()
        } else {
            decoded_state
                .as_ref()
                .expect("cache miss must materialize the authoritative VN state")
                .pending_wait
                .clone()
        };
        let state_decode_ns = profile_elapsed_ns(state_started);
        let reduce_started = profile.then(Instant::now);
        let (mut state, mut output) = if let Some(cached) = cached_state {
            astra_vn_core::reduce_vn_step_indexed_prehashed_pending(
                Arc::clone(&self.compiled),
                Arc::clone(&self.runtime_index),
                cached.state,
                previous_state_hash,
                command,
            )
        } else {
            let state = decoded_state.expect("cache miss must materialize authoritative VN state");
            astra_vn_core::reduce_vn_step_indexed_prehashed_pending(
                Arc::clone(&self.compiled),
                Arc::clone(&self.runtime_index),
                state,
                previous_state_hash,
                command,
            )
        }
        .map_err(|err| RuntimeError::message(err.to_string()))?;
        let reduce_ns = profile_elapsed_ns(reduce_started);
        let await_started = profile.then(Instant::now);
        if state.pending_wait != previous_wait {
            if let Some(wait) = state.pending_wait.as_mut() {
                output.set_wait(wait.clone());
                let has_runtime_await_id = wait
                    .await_id
                    .as_deref()
                    .is_some_and(|await_id| astra_core::StableId::parse(await_id).is_ok());
                if !has_runtime_await_id {
                    wait.await_id.as_ref().ok_or_else(|| {
                        RuntimeError::message(
                            "VN reducer created a wait without an authored await identity",
                        )
                    })?;
                    let token = ctx.create_await(astra_runtime::AwaitKind::Custom(format!(
                        "vn.{:?}",
                        wait.kind
                    )));
                    let runtime_await_id = token.token_id.0.to_string();
                    wait.await_id = Some(runtime_await_id.clone());
                    output.set_wait(wait.clone());
                    output.push_await(token.token_id.0.to_string());
                    ctx.push_await(token)?;
                }
            }
        }
        let await_ns = profile_elapsed_ns(await_started);
        let replace_started = profile.then(Instant::now);
        if state.backlog.len() < previous_hot.backlog_count {
            return Err(RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                "ASTRA_NATIVE_VN_HISTORY_TRUNCATION",
                "VN reducer attempted to truncate append-only backlog history",
            )));
        }
        let mut tail_id = previous_hot.tail_chunk;
        let mut tail: VnRuntimeHistoryChunkV3 = ctx.read_component(tail_id)?;
        if tail.schema != VN_RUNTIME_HISTORY_CHUNK_SCHEMA
            || tail.entries.len() > VN_HISTORY_CHUNK_CAPACITY
            || tail.root != previous_hot.backlog_root
        {
            return Err(RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                "ASTRA_NATIVE_VN_HISTORY_TAIL_INVALID",
                "VN history tail does not match the authoritative hot state",
            )));
        }
        let mut remaining = &state.backlog[previous_hot.backlog_count..];
        let appended_backlog_entries = remaining.len();
        let mut history_component_writes = 0_usize;
        let mut backlog_root = previous_hot.backlog_root;
        while !remaining.is_empty() {
            let available = VN_HISTORY_CHUNK_CAPACITY.saturating_sub(tail.entries.len());
            if available > 0 {
                let take = available.min(remaining.len());
                tail.entries.extend_from_slice(&remaining[..take]);
                remaining = &remaining[take..];
                tail.root = backlog_chunk_root(tail.previous_root, &tail.entries)
                    .map_err(|error| RuntimeError::message(error.to_string()))?;
                backlog_root = tail.root;
                if evidence_mode {
                    ctx.replace_component(tail_id, &tail)?;
                } else {
                    ctx.replace_component_owned(tail_id, &tail)?;
                }
                history_component_writes = history_component_writes
                    .checked_add(1)
                    .ok_or_else(|| RuntimeError::message("VN history write count overflowed"))?;
            }
            if !remaining.is_empty() {
                let take = VN_HISTORY_CHUNK_CAPACITY.min(remaining.len());
                let entries = remaining[..take].to_vec();
                remaining = &remaining[take..];
                let root = backlog_chunk_root(backlog_root, &entries)
                    .map_err(|error| RuntimeError::message(error.to_string()))?;
                let next = VnRuntimeHistoryChunkV3 {
                    schema: VN_RUNTIME_HISTORY_CHUNK_SCHEMA.to_string(),
                    previous: Some(tail_id),
                    previous_root: backlog_root,
                    entries,
                    root,
                };
                tail_id = ctx.attach_component(
                    self.owner,
                    VN_RUNTIME_HISTORY_CHUNK_SCHEMA,
                    BlackboardValue::Null,
                )?;
                let encoded = postcard::to_allocvec(&next)
                    .map_err(|error| RuntimeError::message(error.to_string()))?;
                let encoded = encoded.into();
                if evidence_mode {
                    ctx.replace_component_encoded_postcard(tail_id, encoded)?;
                } else {
                    ctx.replace_component_owned_postcard(tail_id, encoded)?;
                }
                history_component_writes = history_component_writes
                    .checked_add(1)
                    .ok_or_else(|| RuntimeError::message("VN history write count overflowed"))?;
                tail = next;
                backlog_root = root;
            }
        }
        let next_hot = hot_state_from_runtime(&state, &self.runtime_index, tail_id, backlog_root)
            .map_err(|error| RuntimeError::message(error.to_string()))?;
        // Only the bounded hot state and at most one 64-entry tail chunk are
        // rewritten during a normal dialogue advance. Full history is already
        // represented by immutable linked components in the Runtime snapshot.
        let encoded_state: Arc<[u8]> = postcard::to_allocvec(&next_hot)
            .map_err(|error| RuntimeError::message(format!("encode VN hot state: {error}")))?
            .into();
        let encoded_state_bytes = encoded_state.len();
        // Evidence binds the state bytes to a digest. Shipping only moves the
        // owned postcard allocation into the component store.
        let encoded_state = if evidence_mode {
            astra_runtime::ValidatedRuntimeComponentEncoding::postcard_blake3(encoded_state)
        } else {
            astra_runtime::ValidatedRuntimeComponentEncoding::postcard_owned(encoded_state)
        };
        let authoritative_state_hash = encoded_state.state_hash();
        let output = output.finalize(authoritative_state_hash);
        let mutation_journal_entries = output.mutations.len();
        let (_, next_state_hash) =
            ctx.replace_component_validated_postcard(self.component, encoded_state)?;
        let replace_component_ns = profile_elapsed_ns(replace_started);
        let output_started = profile.then(Instant::now);
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
        if evidence_mode {
            for command in &output.presentation {
                ctx.emit_presentation(runtime_presentation(command)?);
            }
        }
        if evidence_mode {
            for command in &output.audio {
                ctx.emit_serialized_effect("audio", "astra.vn.audio_command.v2", command)?;
            }
        }
        for task in &output.timeline_tasks {
            ctx.emit_serialized_effect("timeline", "astra.vn.timeline_task.v2", task)?;
        }
        let output_emit_ns = profile_elapsed_ns(output_started);
        let trace_started = profile.then(Instant::now);
        let mut trace_payload = if evidence_mode {
            input.clone()
        } else {
            BTreeMap::new()
        };
        trace_payload.insert(
            "event_kind".to_string(),
            BlackboardValue::String(event_kind),
        );
        if evidence_mode {
            trace_payload.insert(
                "state_hash_before".to_string(),
                BlackboardValue::String(output.state_hash_before_advance.to_string()),
            );
            trace_payload.insert(
                "state_hash_after".to_string(),
                BlackboardValue::String(output.state_hash_after_advance.to_string()),
            );
        }
        *self
            .output
            .lock()
            .map_err(|_| RuntimeError::message("VN step output lock is poisoned"))? = Some(output);
        *self
            .state_cache
            .lock()
            .map_err(|_| RuntimeError::message("VN step state cache lock is poisoned"))? =
            Some(VnStepStateCache {
                state_hash: next_state_hash,
                state,
            });
        *self
            .step_complexity
            .lock()
            .map_err(|_| RuntimeError::message("VN step complexity lock is poisoned"))? =
            Some(VnStepComplexityMetrics {
                schema: "astra.vn.step_complexity_metrics.v1".to_string(),
                previous_backlog_count: previous_hot.backlog_count,
                appended_backlog_entries,
                state_cache_hit,
                materialized_history_entries,
                history_component_writes,
                encoded_hot_state_bytes: encoded_state_bytes,
                mutation_journal_entries,
            });
        let trace_store_ns = profile_elapsed_ns(trace_started);
        tracing::trace!(
            event = "vn.step.performance",
            command_mapping_ns,
            state_decode_ns,
            reduce_ns,
            await_ns,
            replace_component_ns,
            encoded_state_bytes,
            output_emit_ns,
            trace_store_ns,
            "measured NativeVN RuntimeAction phases"
        );
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: trace_payload,
        })
    }
}

fn profile_elapsed_ns(started: Option<Instant>) -> u64 {
    started.map_or(0, |started| {
        u64::try_from(started.elapsed().as_nanos())
            .expect("NativeVN performance phase duration must fit in u64 nanoseconds")
    })
}

fn runtime_presentation(
    command: &PresentationCommand,
) -> Result<RuntimePresentationCommand, RuntimeError> {
    let converted = match command {
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
        PresentationCommand::SystemOption { option } => RuntimePresentationCommand::Custom {
            kind: "vn.system_option.v1".to_string(),
            data: [
                ("id".to_string(), BlackboardValue::String(option.id.clone())),
                (
                    "key".to_string(),
                    BlackboardValue::String(option.key.clone()),
                ),
                (
                    "target".to_string(),
                    BlackboardValue::String(option.target.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        },
        PresentationCommand::Stage(stage) => RuntimePresentationCommand::Custom {
            kind: format!("vn.stage.{}.v2", stage.kind()),
            data: [(
                "typed_payload".to_string(),
                BlackboardValue::Bytes(postcard::to_allocvec(stage).map_err(|err| {
                    RuntimeError::message(format!("encode typed VN stage command: {err}"))
                })?),
            )]
            .into_iter()
            .collect(),
        },
        PresentationCommand::Extension(extension) => RuntimePresentationCommand::Custom {
            kind: format!("vn.extension.{}", extension.command),
            data: [
                (
                    "provider_id".to_string(),
                    BlackboardValue::String(extension.provider_id.clone()),
                ),
                (
                    "schema".to_string(),
                    BlackboardValue::String(extension.schema.clone()),
                ),
                (
                    "typed_payload".to_string(),
                    BlackboardValue::Bytes(postcard::to_allocvec(extension).map_err(|err| {
                        RuntimeError::message(format!("encode typed VN extension command: {err}"))
                    })?),
                ),
            ]
            .into_iter()
            .collect(),
        },
        PresentationCommand::Marker { id } => {
            RuntimePresentationCommand::Marker { name: id.clone() }
        }
    };
    Ok(converted)
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
    let persisted = match output
        .persisted
        .into_iter()
        .map(ffi_persisted_output)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(value) => value,
        Err(error) => return ffi_step_error(session_id, error),
    };
    FfiRuntimeStepResult {
        ok: true,
        session_id: output.session_id.0.into(),
        status: output.status.into(),
        live,
        persisted: RVec::from(persisted),
        diagnostics: RVec::from(
            diagnostics
                .into_iter()
                .map(RString::from)
                .collect::<Vec<_>>(),
        ),
    }
}

#[cfg(feature = "ffi")]
fn ffi_persisted_output(
    output: RuntimePersistedOutput,
) -> Result<FfiRuntimePersistedOutput, String> {
    let codec = match output.codec {
        RuntimePersistedCodec::Postcard => FfiRuntimeSectionCodec::Postcard,
    };
    let bytes = output.bytes().as_ref().to_vec();
    Ok(FfiRuntimePersistedOutput {
        domain: runtime_output_domain_id(output.domain),
        schema: output.schema.into(),
        version_major: output.version.major,
        version_minor: output.version.minor,
        version_patch: output.version.patch,
        codec,
        bytes: RVec::from(bytes),
    })
}

#[cfg(feature = "ffi")]
fn runtime_output_domain_id(domain: astra_plugin_abi::RuntimeOutputDomain) -> u8 {
    match domain {
        astra_plugin_abi::RuntimeOutputDomain::Effect => 0,
        astra_plugin_abi::RuntimeOutputDomain::Presentation => 1,
        astra_plugin_abi::RuntimeOutputDomain::Audio => 2,
        astra_plugin_abi::RuntimeOutputDomain::Await => 3,
        astra_plugin_abi::RuntimeOutputDomain::Observation => 4,
        astra_plugin_abi::RuntimeOutputDomain::Trace => 5,
        astra_plugin_abi::RuntimeOutputDomain::DirtySaveSection => 6,
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
                RuntimeLivePcmBuffer::I16(samples) => FfiRuntimePcmBuffer::I16(RVec::from(samples)),
                RuntimeLivePcmBuffer::F32(samples) => FfiRuntimePcmBuffer::F32(RVec::from(samples)),
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
            let (sync_kind, sync_fence) = match cue.sync {
                RuntimeLiveAudioSync::None => (FfiRuntimeAudioSyncKind::None, RString::new()),
                RuntimeLiveAudioSync::Text => (FfiRuntimeAudioSyncKind::Text, RString::new()),
                RuntimeLiveAudioSync::Fence(fence) => {
                    (FfiRuntimeAudioSyncKind::Fence, fence.into())
                }
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
                sync_kind,
                sync_fence,
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
        video.push(FfiRuntimeVideoCommand {
            sequence: command.sequence,
            playback_id: match &command.command {
                RuntimeLiveVideoCommandKind::Play { playback_id, .. }
                | RuntimeLiveVideoCommandKind::Stop { playback_id } => playback_id.clone().into(),
            },
            resource_uri: match &command.command {
                RuntimeLiveVideoCommandKind::Play { resource_uri, .. } => {
                    resource_uri.clone().into()
                }
                RuntimeLiveVideoCommandKind::Stop { .. } => RString::new(),
            },
            mode: match &command.command {
                RuntimeLiveVideoCommandKind::Play { mode, .. } => match mode {
                    RuntimeLiveVideoMode::ModalWithAudio => FfiRuntimeVideoMode::ModalWithAudio,
                    RuntimeLiveVideoMode::LayerNoAudio => FfiRuntimeVideoMode::LayerNoAudio,
                },
                RuntimeLiveVideoCommandKind::Stop { .. } => FfiRuntimeVideoMode::LayerNoAudio,
            },
            stage_width: match &command.command {
                RuntimeLiveVideoCommandKind::Play { stage_width, .. } => *stage_width,
                RuntimeLiveVideoCommandKind::Stop { .. } => 0,
            },
            stage_height: match &command.command {
                RuntimeLiveVideoCommandKind::Play { stage_height, .. } => *stage_height,
                RuntimeLiveVideoCommandKind::Stop { .. } => 0,
            },
            command: match command.command {
                RuntimeLiveVideoCommandKind::Play { .. } => FfiRuntimeVideoCommandKind::Play,
                RuntimeLiveVideoCommandKind::Stop { .. } => FfiRuntimeVideoCommandKind::Stop,
            },
        })
    }
    for wait in value.waits {
        {
            let (kind, number, name, keys, payload_len) = match wait.kind {
                RuntimeLiveWaitKind::Frame { frames } => (
                    FfiRuntimeWaitKind::Frame,
                    frames,
                    String::new(),
                    Vec::new(),
                    0,
                ),
                RuntimeLiveWaitKind::Time { milliseconds } => (
                    FfiRuntimeWaitKind::Time,
                    milliseconds,
                    String::new(),
                    Vec::new(),
                    0,
                ),
                RuntimeLiveWaitKind::Input { keys } => {
                    (FfiRuntimeWaitKind::Input, 0, String::new(), keys, 0)
                }
                RuntimeLiveWaitKind::MediaFence { media_id } => {
                    (FfiRuntimeWaitKind::MediaFence, 0, media_id, Vec::new(), 0)
                }
                RuntimeLiveWaitKind::PresentationFence { fence_id } => (
                    FfiRuntimeWaitKind::PresentationFence,
                    0,
                    fence_id,
                    Vec::new(),
                    0,
                ),
                RuntimeLiveWaitKind::ProviderCompletion { request_id } => (
                    FfiRuntimeWaitKind::ProviderCompletion,
                    0,
                    request_id,
                    Vec::new(),
                    0,
                ),
                RuntimeLiveWaitKind::FamilyOpaque {
                    wait_kind,
                    payload_len,
                } => (
                    FfiRuntimeWaitKind::FamilyOpaque,
                    0,
                    wait_kind,
                    Vec::new(),
                    payload_len,
                ),
            };
            waits.push(FfiRuntimeWait {
                sequence: wait.sequence,
                token_id: wait.token_id.into(),
                kind,
                number,
                name: name.into(),
                keys: RVec::from(keys.into_iter().map(RString::from).collect::<Vec<_>>()),
                payload_len,
            });
        }
    }
    for event in value.events {
        events.push(FfiRuntimeEvent {
            sequence: event.sequence,
            event: event.event.into(),
            payload: RVec::from(event.payload),
            due_tick: event.due_tick.into(),
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
            value: RVec::from(value),
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
            scenes: RVec::from(scenes),
            resource_scenes: RVec::from(resource_scenes),
            audio: RVec::from(audio),
            audio_commands: RVec::from(audio_commands),
            audio_cues: RVec::from(audio_cues),
            text: RVec::from(text),
            text_presentations: RVec::from(text_presentations),
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
                        pixels: RVec::from(pixels),
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
                        pixels: RVec::from(pixels),
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
    let sequence = match &command {
        RuntimeLiveAudioCommand::LoadResource { sequence, .. }
        | RuntimeLiveAudioCommand::CreateStream { sequence, .. }
        | RuntimeLiveAudioCommand::SubmitI16 { sequence, .. }
        | RuntimeLiveAudioCommand::SubmitF32 { sequence, .. }
        | RuntimeLiveAudioCommand::Play { sequence, .. }
        | RuntimeLiveAudioCommand::Stop { sequence, .. }
        | RuntimeLiveAudioCommand::Pause { sequence, .. }
        | RuntimeLiveAudioCommand::Resume { sequence, .. }
        | RuntimeLiveAudioCommand::SetParams { sequence, .. }
        | RuntimeLiveAudioCommand::DestroyStream { sequence, .. }
        | RuntimeLiveAudioCommand::MasterVolume { sequence, .. } => *sequence,
    };
    let (
        kind,
        stream_id,
        sample_rate,
        channels,
        encoding,
        sample_format,
        resource_uri,
        samples,
        volume,
        pan,
        repeat,
        fade_ms,
    ) = match command {
        RuntimeLiveAudioCommand::LoadResource {
            sequence: _,
            stream_id,
            encoding,
            resource_uri,
        } => (
            FfiRuntimeAudioCommandKind::LoadResource,
            stream_id,
            0,
            0,
            match encoding {
                RuntimeLiveAudioEncoding::Unknown => FfiRuntimeAudioEncoding::Unknown,
                RuntimeLiveAudioEncoding::Wav => FfiRuntimeAudioEncoding::Wav,
                RuntimeLiveAudioEncoding::Ogg => FfiRuntimeAudioEncoding::Ogg,
                RuntimeLiveAudioEncoding::Mp3 => FfiRuntimeAudioEncoding::Mp3,
                RuntimeLiveAudioEncoding::Flac => FfiRuntimeAudioEncoding::Flac,
            },
            FfiRuntimeAudioSampleFormat::I16,
            resource_uri,
            FfiRuntimePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::CreateStream {
            sequence: _,
            stream_id,
            sample_rate,
            channels,
            sample_format,
        } => (
            FfiRuntimeAudioCommandKind::CreateStream,
            stream_id,
            sample_rate,
            channels,
            FfiRuntimeAudioEncoding::Unknown,
            match sample_format {
                RuntimeLiveAudioSampleFormat::I16 => FfiRuntimeAudioSampleFormat::I16,
                RuntimeLiveAudioSampleFormat::F32 => FfiRuntimeAudioSampleFormat::F32,
            },
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::SubmitI16 {
            sequence: _,
            stream_id,
            samples,
        } => (
            FfiRuntimeAudioCommandKind::SubmitI16,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::from(samples)),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::SubmitF32 {
            sequence: _,
            stream_id,
            samples,
        } => (
            FfiRuntimeAudioCommandKind::SubmitF32,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::F32,
            String::new(),
            FfiRuntimePcmBuffer::F32(RVec::from(samples)),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::Play {
            sequence: _,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => (
            FfiRuntimeAudioCommandKind::Play,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            volume,
            pan,
            repeat,
            fade_in_ms,
        ),
        RuntimeLiveAudioCommand::Stop {
            sequence: _,
            stream_id,
            fade_ms,
        } => (
            FfiRuntimeAudioCommandKind::Stop,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            fade_ms,
        ),
        RuntimeLiveAudioCommand::Pause {
            sequence: _,
            stream_id,
        } => (
            FfiRuntimeAudioCommandKind::Pause,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::Resume {
            sequence: _,
            stream_id,
        } => (
            FfiRuntimeAudioCommandKind::Resume,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::SetParams {
            sequence: _,
            stream_id,
            volume,
            pan,
            repeat,
        } => (
            FfiRuntimeAudioCommandKind::SetParams,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            volume,
            pan,
            repeat,
            0,
        ),
        RuntimeLiveAudioCommand::DestroyStream {
            sequence: _,
            stream_id,
        } => (
            FfiRuntimeAudioCommandKind::DestroyStream,
            stream_id,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            0.0,
            0.0,
            false,
            0,
        ),
        RuntimeLiveAudioCommand::MasterVolume {
            sequence: _,
            volume,
        } => (
            FfiRuntimeAudioCommandKind::MasterVolume,
            0,
            0,
            0,
            FfiRuntimeAudioEncoding::Unknown,
            FfiRuntimeAudioSampleFormat::I16,
            String::new(),
            FfiRuntimePcmBuffer::I16(RVec::new()),
            volume,
            0.0,
            false,
            0,
        ),
    };
    FfiRuntimeAudioCommand {
        sequence,
        kind,
        stream_id,
        sample_rate,
        channels,
        encoding,
        sample_format,
        resource_uri: resource_uri.into(),
        samples,
        volume,
        pan,
        repeat,
        fade_ms,
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
        persisted: RVec::new(),
        diagnostics: RVec::from(vec![RString::from(message)]),
    }
}

#[cfg(test)]
mod runtime_view_tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn cursor(command_id: &str) -> VnCommandCursor {
        VnCommandCursor {
            story_id: "story".into(),
            state_id: "state".into(),
            scene_id: "scene".into(),
            command_id: command_id.into(),
            ordinal: 0,
        }
    }

    fn backlog_entry(command_id: &str) -> BacklogEntry {
        BacklogEntry {
            command_id: command_id.into(),
            key: format!("key.{command_id}"),
            speaker: None,
            voice: None,
            story_id: "story".into(),
            state_id: "state".into(),
            route_position: 0,
            read: true,
            layout: BacklogLayoutMetadata { window: None },
        }
    }

    fn state() -> VnRuntimeState {
        VnRuntimeState {
            schema: VN_RUNTIME_STATE_SCHEMA.into(),
            instance_id: "instance".into(),
            profile: "classic".into(),
            locale: "ja-JP".into(),
            cursor: Some(cursor("current")),
            call_stack: Vec::new(),
            system_stack: Vec::new(),
            system: VnSystemState::default(),
            pending_choice: None,
            variables: BTreeMap::new(),
            backlog: vec![backlog_entry("first"), backlog_entry("last")],
            read_state: BTreeSet::from(["read.command".into()]),
            voice_replay: BTreeMap::from([(
                "voice".into(),
                VoiceReplayEntry {
                    voice: "voice".into(),
                    line_key: "line".into(),
                    speaker: None,
                },
            )]),
            route_coverage: BTreeSet::from(["route".into()]),
            route_flags: BTreeMap::from([(
                "route".into(),
                VnRouteFlag::new(VnRouteFlagKind::Launch, "source", "target"),
            )]),
            wait_sequence: 0,
            pending_wait: None,
        }
    }

    fn open_page(state: &mut VnRuntimeState, page: SystemPageKind) {
        state.system_stack.push(VnSystemFrame {
            return_to: cursor("return"),
            return_wait: None,
            return_choice: None,
            page,
        });
    }

    #[test]
    fn ordinary_runtime_view_is_bounded_but_preserves_authoritative_count() {
        let state = state();
        let hash = Hash128::from_bytes([7; 16]);
        let view = runtime_view_state(&state, hash);

        assert_eq!(view.authoritative_state_hash, hash);
        assert_eq!(view.backlog_count, 2);
        assert_eq!(view.state.backlog, vec![backlog_entry("last")]);
        assert!(view.state.read_state.is_empty());
        assert!(view.state.voice_replay.is_empty());
        assert!(view.state.route_coverage.is_empty());
        assert!(view.state.route_flags.is_empty());
    }

    #[test]
    fn system_pages_expose_only_the_history_the_page_owns() {
        let mut backlog = state();
        open_page(&mut backlog, SystemPageKind::Backlog);
        let backlog_view = runtime_view_state(&backlog, Hash128::from_bytes([1; 16]));
        assert_eq!(backlog_view.state.backlog, backlog.backlog);
        assert!(backlog_view.state.voice_replay.is_empty());

        let mut voice = state();
        open_page(&mut voice, SystemPageKind::VoiceReplay);
        let voice_view = runtime_view_state(&voice, Hash128::from_bytes([2; 16]));
        assert_eq!(voice_view.state.voice_replay, voice.voice_replay);
        assert_eq!(voice_view.state.backlog.len(), 1);

        let mut route = state();
        open_page(&mut route, SystemPageKind::RouteChart);
        let route_view = runtime_view_state(&route, Hash128::from_bytes([3; 16]));
        assert_eq!(route_view.state.route_coverage, route.route_coverage);
        assert_eq!(route_view.state.route_flags, route.route_flags);
        assert!(route_view.state.voice_replay.is_empty());
    }

    #[test]
    fn terminal_runtime_view_exposes_route_completion_evidence() {
        let mut state = state();
        state.cursor = None;
        let view = runtime_view_state(&state, Hash128::from_bytes([4; 16]));
        assert_eq!(view.state.route_coverage, state.route_coverage);
        assert_eq!(view.state.route_flags, state.route_flags);
    }
}
