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
use astra_core::{Hash128, Hash256, SchemaVersion};
use astra_plugin::{ProductRuntimeProvider, ProductRuntimeProviderFactory, ProductRuntimeSession};
#[cfg(feature = "ffi")]
use astra_plugin_abi::{
    FfiRuntimeProviderRegistration, FfiRuntimeProviderResult, RuntimeProviderCall,
    RuntimeProviderCreateRequest, RuntimeProviderDestroyRequest, RuntimeProviderSessionCall,
    RuntimeProviderSessionHandle, RuntimeProviderSessionOpenReport,
    PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA, PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ReleaseCheckDescriptor, RuntimeEditorMetadata,
    RuntimeExecutorKind, RuntimeOpenReport, RuntimeOpenRequest, RuntimeOutputCodec,
    RuntimeOutputDomain, RuntimeOutputEnvelope, RuntimeOutputSchemaDescriptor,
    RuntimePackageSectionPlan, RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport,
    RuntimeProbeRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionCodec,
    RuntimeSectionPayload, RuntimeSectionRef, RuntimeShutdownReport, RuntimeStepInput,
    RuntimeStepMode, RuntimeStepOutput, RuntimeTickIntegrityMode, GAME_RUNTIME_PROVIDER_SLOT,
    NATIVE_VN_PROVIDER_ID, NATIVE_VN_RUNTIME_ID, RUNTIME_EDITOR_METADATA_SCHEMA,
};
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, ActorId, BlackboardValue, ComponentId, ComponentRecord,
    DeterministicActionContext, EventPayload, GuardExpr, OrderedTickIngress, PackageHandle,
    PlayerInput, PresentationCommand as RuntimePresentationCommand, RuntimeAction,
    RuntimeComponentPayload, RuntimeConfig, RuntimeError, RuntimeSnapshot, RuntimeWorld, SaveBlob,
    SaveRequest, StateDefinition, StateMachineDefinition, TickIngress, TickInput,
    TickIntegrityMode, TickRequest, TransitionDefinition,
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
    payload_hash: Hash256,
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
    let (payload_hash, bytes) = session
        .world
        .read_component_postcard_payload(session.vn_component)
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    if let Some(state) = session
        .state_cache
        .lock()
        .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))?
        .as_ref()
        .filter(|cached| cached.payload_hash == payload_hash)
        .map(|cached| cached.state.clone())
    {
        return Ok(state);
    }
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
    let (payload_hash, _) = session
        .world
        .read_component_postcard_payload(session.vn_component)
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    let state_hash = Hash128::from_blake3(&postcard::to_allocvec(&hot)?);
    *session
        .state_cache
        .lock()
        .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))? =
        Some(VnStepStateCache {
            payload_hash,
            state_hash,
            state,
        });
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
        let (_, hot_bytes) = session
            .world
            .read_component_postcard_payload(session.vn_component)
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
            let (_, bytes) = session
                .world
                .read_component_postcard_payload(id)
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
                output_schema(RuntimeOutputDomain::Await, "astra.runtime.await_id.v1", 1),
                output_schema(
                    RuntimeOutputDomain::Observation,
                    "astra.product.observation.v1",
                    1,
                ),
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
        let (payload_hash, _) = world
            .read_component_postcard_payload(vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let initial_state_hash = Hash128::from_blake3(&postcard::to_allocvec(&initial_hot)?);
        let state_cache = Arc::new(Mutex::new(Some(VnStepStateCache {
            payload_hash,
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
        if input.mode == RuntimeStepMode::Replay {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_LIVE_PROVIDER_REPLAY",
                "provider-free replay cannot call NativeVnRuntimeProvider::step",
            ));
        }
        tracing::trace!(
            event = "vn.provider.session.step",
            fixed_step = input.fixed_step,
            "AstraVN runtime session step started"
        );
        let command = match input.action.as_str() {
            "command" => serde_json::from_value(input.payload.clone())
                .map_err(|err| CoreVnError::message(format!("decode VN player command: {err}")))?,
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
        let command_bytes = postcard::to_allocvec(&command)?;
        let (component_payload_hash, _) = session
            .world
            .read_component_postcard_payload(session.vn_component)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let cached_command_state = session
            .state_cache
            .lock()
            .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))?
            .as_ref()
            .filter(|cached| cached.payload_hash == component_payload_hash)
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
                    data: [("command".to_string(), BlackboardValue::Bytes(command_bytes))]
                        .into_iter()
                        .collect(),
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
            RuntimeStepMode::Replay => TickRequest::replay(timing, ingress),
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
        // Audio cues are also typed presentation commands. Preserve their position
        // relative to stage audio controls instead of grouping output by domain:
        // a BGM start followed by fade-stop must never be observed in reverse.
        let mut media = Vec::with_capacity(output.presentation.len() + output.audio.len());
        let mut audio = output.audio.iter();
        for command in &output.presentation {
            media.push(
                RuntimeOutputEnvelope::postcard(
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
                media.push(
                    RuntimeOutputEnvelope::postcard(
                        RuntimeOutputDomain::Audio,
                        "astra.vn.audio_command.v2",
                        SchemaVersion::new(2, 0, 0),
                        audio_command,
                    )
                    .map_err(|err| CoreVnError::message(err.to_string()))?,
                );
            }
        }
        if audio.next().is_some() {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_AUDIO_ORDER_EXTRA",
                "audio output has no matching typed presentation command",
            ));
        }
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
                VN_RUNTIME_VIEW_STATE_SCHEMA,
                SchemaVersion::new(VN_RUNTIME_VIEW_STATE_SCHEMA_MAJOR, 0, 0),
                &runtime_view_state,
            )
            .map_err(|err| CoreVnError::message(err.to_string()))?,
        ];
        let observations = vec![RuntimeOutputEnvelope::postcard(
            RuntimeOutputDomain::Observation,
            "astra.product.observation.v1",
            SchemaVersion::new(1, 0, 0),
            &NativeVnStepTrace {
                runtime_state_hash: tick.state_hash.to_string(),
                runtime_event_hash: tick.event_hash.to_string(),
                runtime_presentation_hash: tick.presentation_hash.to_string(),
            },
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?];
        let dirty_save_sections = vec![RuntimeOutputEnvelope::postcard(
            RuntimeOutputDomain::DirtySaveSection,
            "astra.runtime.dirty_save_section.v1",
            SchemaVersion::new(1, 0, 0),
            &"runtime.world".to_string(),
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?];
        Ok(RuntimeStepOutput {
            session_id,
            status: if output.presentation.is_empty() {
                "idle".to_string()
            } else {
                "blocked".to_string()
            },
            outputs: effects
                .into_iter()
                .chain(media)
                .chain(timeline)
                .chain(awaits)
                .chain(observations)
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
        let save_hash = Hash256::from_sha256(&save.0);
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: vec![RuntimeSectionPayload {
                section_id: "runtime.world".to_string(),
                schema: "astra.runtime.save_blob.v3".to_string(),
                version: SchemaVersion::new(3, 0, 0),
                codec: RuntimeSectionCodec::Raw,
                hash: save_hash,
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
            "astra.runtime.save_blob.v3",
            RuntimeSectionCodec::Raw,
        )?;
        let session = self.session_mut(&request.session_id)?;
        session
            .world
            .load(SaveBlob(runtime_section.bytes.clone()))
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let state = consume_materialized_restore_state(session)?;
        let (payload_hash, bytes) = session
            .world
            .read_component_postcard_payload(session.vn_component)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let state_hash = Hash128::from_blake3(&bytes);
        *session
            .state_cache
            .lock()
            .map_err(|_| CoreVnError::message("VN step state cache lock is poisoned"))? =
            Some(VnStepStateCache {
                payload_hash,
                state_hash,
                state,
            });
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
            "generic command input must be decoded before state-dependent command resolution",
        )),
        "launch_default" => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_LAUNCH_DISPATCH",
            "default launch must be resolved with authoritative session state",
        )),
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
        let profile = tracing::enabled!(tracing::Level::TRACE);
        let command_started = profile.then(Instant::now);
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
        let command_decode_ns = profile_elapsed_ns(command_started);
        let state_started = profile.then(Instant::now);
        let (payload_hash, previous_state_bytes) =
            ctx.read_component_postcard_payload(self.component)?;
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
            .take()
            .filter(|cached| cached.payload_hash == payload_hash);
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
                cached.state_hash,
                command,
            )
        } else {
            let state = decoded_state.expect("cache miss must materialize authoritative VN state");
            let state_hash = Hash128::from_blake3(
                &postcard::to_allocvec(&previous_hot)
                    .map_err(|error| RuntimeError::message(error.to_string()))?,
            );
            astra_vn_core::reduce_vn_step_indexed_prehashed_pending(
                Arc::clone(&self.compiled),
                Arc::clone(&self.runtime_index),
                state,
                state_hash,
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
                ctx.replace_component(tail_id, &tail)?;
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
                ctx.replace_component_encoded_postcard(tail_id, encoded.into())?;
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
        // The host-owned await identity is part of the authoritative state.
        // Reuse this exact byte hash at the Runtime component boundary instead
        // of hashing the growing VN state twice on every input frame.
        let encoded_state =
            astra_runtime::ValidatedRuntimeComponentEncoding::postcard_blake3(encoded_state);
        let authoritative_state_hash = encoded_state.state_hash();
        let output = output.finalize(authoritative_state_hash);
        let mutation_journal_entries = output.mutations.len();
        let (next_payload_hash, next_state_hash) =
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
        for command in &output.presentation {
            ctx.emit_presentation(runtime_presentation(command)?);
        }
        for command in &output.audio {
            ctx.emit_serialized_effect("audio", "astra.vn.audio_command.v2", command)?;
        }
        for task in &output.timeline_tasks {
            ctx.emit_serialized_effect("timeline", "astra.vn.timeline_task.v2", task)?;
        }
        let output_emit_ns = profile_elapsed_ns(output_started);
        let trace_started = profile.then(Instant::now);
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
        *self
            .state_cache
            .lock()
            .map_err(|_| RuntimeError::message("VN step state cache lock is poisoned"))? =
            Some(VnStepStateCache {
                payload_hash: next_payload_hash,
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
            command_decode_ns,
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
extern "C" fn ffi_prepare(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimePrepareRequest| {
        NativeVnRuntimeProvider::default().prepare(request)
    })
}

#[cfg(feature = "ffi")]
type FfiSession = Arc<Mutex<Option<Box<dyn ProductRuntimeSession>>>>;

#[cfg(feature = "ffi")]
struct FfiProviderInstance {
    factory: Arc<NativeVnRuntimeProviderFactory>,
    next_session_handle: u64,
    sessions: BTreeMap<RuntimeProviderSessionHandle, FfiSession>,
}

#[cfg(feature = "ffi")]
static FFI_INSTANCES: OnceLock<Mutex<BTreeMap<String, FfiProviderInstance>>> = OnceLock::new();

#[cfg(feature = "ffi")]
fn ffi_instances() -> &'static Mutex<BTreeMap<String, FfiProviderInstance>> {
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
        let factory = Arc::new(NativeVnRuntimeProviderFactory::default());
        let report = factory.create_instance(request.instance_id.clone())?;
        instances.insert(
            request.instance_id.0,
            FfiProviderInstance {
                factory,
                next_session_handle: 1,
                sessions: BTreeMap::new(),
            },
        );
        Ok(report)
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
        if !instance.sessions.is_empty() {
            return Err("provider instance still has active sessions".to_string());
        }
        let report = instance
            .factory
            .destroy_instance(request.instance_id.clone())?;
        instances.remove(&request.instance_id.0);
        Ok(report)
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
    ffi_json_result(
        payload,
        |call: RuntimeProviderCall| -> Result<RuntimeProviderSessionOpenReport, CoreVnError> {
            let request = serde_json::from_slice::<RuntimeOpenRequest>(&call.payload)
                .map_err(|err| CoreVnError::message(err.to_string()))?;
            let factory = {
                let instances = ffi_instances().lock().map_err(|_| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_FFI_INSTANCE_LOCK",
                        "provider instance registry lock is poisoned",
                    )
                })?;
                Arc::clone(
                    &instances
                        .get(&call.instance_id.0)
                        .ok_or_else(|| {
                            CoreVnError::diagnostic(
                                "ASTRA_NATIVE_VN_FFI_INSTANCE_MISSING",
                                "provider instance is not active",
                            )
                        })?
                        .factory,
                )
            };
            let (report, session) = factory.open(request).map_err(CoreVnError::message)?;
            let mut instances = ffi_instances().lock().map_err(|_| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_FFI_INSTANCE_LOCK",
                    "provider instance registry lock is poisoned",
                )
            })?;
            let instance = instances.get_mut(&call.instance_id.0).ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_FFI_INSTANCE_MISSING",
                    "provider instance was destroyed while opening a session",
                )
            })?;
            let handle = RuntimeProviderSessionHandle(instance.next_session_handle);
            instance.next_session_handle =
                instance.next_session_handle.checked_add(1).ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_FFI_SESSION_HANDLE_EXHAUSTED",
                        "provider session handle space is exhausted",
                    )
                })?;
            instance
                .sessions
                .insert(handle, Arc::new(Mutex::new(Some(session))));
            Ok(RuntimeProviderSessionOpenReport {
                session_handle: handle,
                report,
            })
        },
    )
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_step(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_session_json(payload, |session, request: RuntimeStepInput| {
        session.step(request)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_save(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_session_json(payload, |session, request: RuntimeSaveRequest| {
        session.save(request)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_restore(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_session_json(payload, |session, request: RuntimeRestoreRequest| {
        session.restore(request)
    })
}

#[cfg(feature = "ffi")]
extern "C" fn ffi_shutdown(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_result(payload, |call: RuntimeProviderSessionCall| {
        let session_id = serde_json::from_slice::<GameRuntimeSessionId>(&call.payload)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let session = remove_ffi_session(&call)?;
        let mut guard = session.lock().map_err(|_| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_SESSION_LOCK",
                "provider session lock is poisoned",
            )
        })?;
        let session = guard.take().ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_SESSION_CLOSED",
                "provider session is already closed",
            )
        })?;
        session.shutdown(session_id).map_err(CoreVnError::message)
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
fn ffi_session_json<T, U>(
    payload: RVec<u8>,
    f: impl FnOnce(&mut dyn ProductRuntimeSession, T) -> Result<U, String>,
) -> FfiRuntimeProviderResult
where
    T: serde::de::DeserializeOwned,
    U: serde::Serialize,
{
    ffi_json_result(payload, |call: RuntimeProviderSessionCall| {
        let request = serde_json::from_slice::<T>(&call.payload)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let session = find_ffi_session(&call)?;
        let mut guard = session.lock().map_err(|_| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_SESSION_LOCK",
                "provider session lock is poisoned",
            )
        })?;
        let session = guard.as_deref_mut().ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_SESSION_CLOSED",
                "provider session is already closed",
            )
        })?;
        f(session, request).map_err(CoreVnError::message)
    })
}

#[cfg(feature = "ffi")]
fn find_ffi_session(call: &RuntimeProviderSessionCall) -> Result<FfiSession, CoreVnError> {
    let instances = ffi_instances().lock().map_err(|_| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_FFI_INSTANCE_LOCK",
            "provider instance registry lock is poisoned",
        )
    })?;
    let instance = instances.get(&call.instance_id.0).ok_or_else(|| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_FFI_INSTANCE_MISSING",
            "provider instance is not active",
        )
    })?;
    instance
        .sessions
        .get(&call.session_handle)
        .cloned()
        .ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_SESSION_MISSING",
                "provider session handle is not active",
            )
        })
}

#[cfg(feature = "ffi")]
fn remove_ffi_session(call: &RuntimeProviderSessionCall) -> Result<FfiSession, CoreVnError> {
    let mut instances = ffi_instances().lock().map_err(|_| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_FFI_INSTANCE_LOCK",
            "provider instance registry lock is poisoned",
        )
    })?;
    let instance = instances.get_mut(&call.instance_id.0).ok_or_else(|| {
        CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_FFI_INSTANCE_MISSING",
            "provider instance is not active",
        )
    })?;
    instance
        .sessions
        .remove(&call.session_handle)
        .ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_FFI_SESSION_MISSING",
                "provider session handle is not active",
            )
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
