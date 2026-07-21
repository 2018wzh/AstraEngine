use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[cfg(feature = "dynamic-abi")]
use abi_stable::std_types::RVec;
use astra_core::SchemaVersion;
#[cfg(feature = "dynamic-abi")]
use astra_plugin_abi::{
    FfiRuntimeProviderInvoke, FfiRuntimeProviderRegistration, RuntimeProviderCall,
    RuntimeProviderCreateRequest, RuntimeProviderDestroyRequest,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimeOutputDomain, RuntimeOutputEnvelope, RuntimePrepareReport,
    RuntimePrepareRequest, RuntimeProbeReport, RuntimeProbeRequest, RuntimeProviderInstanceReport,
    RuntimeRestoreReport, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections,
    RuntimeSectionPayload, RuntimeShutdownReport, RuntimeStepInput, RuntimeStepMode,
    RuntimeStepOutput, ValidatedRuntimeProviderSelection,
};
#[cfg(feature = "dynamic-abi")]
use serde::{de::DeserializeOwned, Serialize};

pub trait ProductRuntimeProvider: Send {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Err("ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_UNAVAILABLE: provider does not expose a linked descriptor".to_string())
    }

    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String>;

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String>;

    fn prepare(&mut self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String>;
    fn probe(&mut self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String>;
    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String>;
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String>;
    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String>;
    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String>;
    fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String>;
}

#[derive(Debug, Clone, Copy)]
struct SessionState {
    seed: u64,
    last_fixed_step: Option<u64>,
    next_step_mode: RuntimeStepMode,
    poisoned: bool,
}

impl SessionState {
    fn opened(seed: u64) -> Self {
        Self {
            seed,
            last_fixed_step: None,
            next_step_mode: RuntimeStepMode::Live,
            poisoned: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeHostSchemaRegistry {
    schemas: BTreeMap<RuntimeOutputDomain, BTreeSet<(String, SchemaVersion)>>,
    max_outputs: usize,
    max_output_bytes: usize,
}

impl Default for RuntimeHostSchemaRegistry {
    fn default() -> Self {
        Self {
            schemas: BTreeMap::new(),
            max_outputs: 256,
            max_output_bytes: 8 * 1024 * 1024,
        }
    }
}

impl RuntimeHostSchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_descriptor(descriptor: &ProductRuntimeDescriptor) -> Self {
        let mut registry = Self::new();
        for output in &descriptor.output_schemas {
            registry = registry.allow_version(output.domain, &output.schema, output.version);
        }
        registry
    }

    pub fn with_bounds(mut self, max_outputs: usize, max_output_bytes: usize) -> Self {
        self.max_outputs = max_outputs;
        self.max_output_bytes = max_output_bytes;
        self
    }

    pub fn allow(mut self, domain: RuntimeOutputDomain, schema: impl Into<String>) -> Self {
        self.schemas
            .entry(domain)
            .or_default()
            .insert((schema.into(), SchemaVersion::new(1, 0, 0)));
        self
    }

    pub fn allow_version(
        mut self,
        domain: RuntimeOutputDomain,
        schema: impl Into<String>,
        version: SchemaVersion,
    ) -> Self {
        self.schemas
            .entry(domain)
            .or_default()
            .insert((schema.into(), version));
        self
    }

    fn validate(
        &self,
        domain: RuntimeOutputDomain,
        envelope: &RuntimeOutputEnvelope,
    ) -> Result<(), RuntimeHostError> {
        let allowed = self.schemas.get(&domain);
        let Some((schema, version)) =
            allowed.and_then(|schemas| schemas.get(&(envelope.schema.clone(), envelope.version)))
        else {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_ENVELOPE_SCHEMA",
                format!("unknown {:?} output schema {}", domain, envelope.schema),
            ));
        };
        envelope
            .validate_binding(domain, schema, *version)
            .map_err(|err| RuntimeHostError::new(err.code(), err.to_string()))?;
        Ok(())
    }

    fn validate_output_bounds(&self, output: &RuntimeStepOutput) -> Result<(), RuntimeHostError> {
        if output.outputs.len() > self.max_outputs {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_OUTPUT_COUNT",
                "runtime provider output count exceeds the configured bound",
            ));
        }
        let bytes = output.outputs.iter().try_fold(0usize, |total, envelope| {
            total.checked_add(envelope.bytes().len()).ok_or_else(|| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_OUTPUT_BYTES",
                    "runtime provider output byte count overflowed",
                )
            })
        })?;
        if bytes > self.max_output_bytes {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_OUTPUT_BYTES",
                "runtime provider output bytes exceed the configured bound",
            ));
        }
        Ok(())
    }

    fn validate_sections(
        &self,
        sections: &[RuntimeSectionPayload],
    ) -> Result<(), RuntimeHostError> {
        if sections.len() > self.max_outputs {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SECTION_COUNT",
                "runtime save section count exceeds the configured bound",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut bytes = 0usize;
        for section in sections {
            if !is_safe_runtime_symbol(&section.section_id)
                || !is_safe_runtime_symbol(&section.schema)
            {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_SECTION_DESCRIPTOR",
                    "runtime save section id and schema must be non-empty safe symbols",
                ));
            }
            if !ids.insert(section.section_id.as_str()) {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_SECTION_DUPLICATE",
                    "runtime save section ids must be unique",
                ));
            }
            if !section.validate_hash() {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_SECTION_HASH",
                    "runtime save section hash does not match its bytes",
                ));
            }
            bytes = bytes.checked_add(section.bytes.len()).ok_or_else(|| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_SECTION_BYTES",
                    "runtime save section byte count overflowed",
                )
            })?;
        }
        if bytes > self.max_output_bytes {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SECTION_BYTES",
                "runtime save section bytes exceed the configured bound",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostState {
    Created,
    Open,
    Shutdown,
    Poisoned,
    Destroyed,
}

pub struct ProductRuntimeHost {
    instance_id: ProviderInstanceId,
    provider: Box<dyn ProductRuntimeProvider>,
    schemas: RuntimeHostSchemaRegistry,
    sessions: BTreeMap<String, SessionState>,
    state: HostState,
    runtime_binding: Option<ValidatedRuntimeProviderSelection>,
}

impl ProductRuntimeHost {
    pub fn bound_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
    ) -> Result<Self, RuntimeHostError> {
        let descriptor = provider.descriptor().map_err(|message| {
            RuntimeHostError::new("ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_UNAVAILABLE", message)
        })?;
        selection
            .validate_linked_descriptor(&descriptor)
            .map_err(|diagnostic| RuntimeHostError::new(diagnostic.code, diagnostic.message))?;
        let mut host = Self::create(instance_id, Box::new(provider), schemas)?;
        host.runtime_binding = Some(selection.clone());
        Ok(host)
    }

    pub fn reference_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
    ) -> Result<Self, RuntimeHostError> {
        Self::create(instance_id, Box::new(provider), schemas)
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn bound_ffi(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        registration: FfiRuntimeProviderRegistration,
        schemas: RuntimeHostSchemaRegistry,
    ) -> Result<Self, RuntimeHostError> {
        Self::bound_in_process(
            instance_id,
            selection,
            FfiProductRuntimeProvider::new(registration),
            schemas,
        )
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn reference_ffi(
        instance_id: impl Into<String>,
        registration: FfiRuntimeProviderRegistration,
        schemas: RuntimeHostSchemaRegistry,
    ) -> Result<Self, RuntimeHostError> {
        Self::create(
            instance_id,
            Box::new(FfiProductRuntimeProvider::new(registration)),
            schemas,
        )
    }

    fn create(
        instance_id: impl Into<String>,
        mut provider: Box<dyn ProductRuntimeProvider>,
        schemas: RuntimeHostSchemaRegistry,
    ) -> Result<Self, RuntimeHostError> {
        let instance_id = ProviderInstanceId(instance_id.into());
        if instance_id.0.trim().is_empty() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_INSTANCE_ID",
                "runtime provider instance id must not be empty",
            ));
        }
        let report = match call_provider("ASTRA_RUNTIME_HOST_CREATE", "create", || {
            provider.create_instance(instance_id.clone())
        }) {
            Ok(report) => report,
            Err(create_error) => {
                let rollback =
                    call_provider("ASTRA_RUNTIME_HOST_CREATE_ROLLBACK", "destroy", || {
                        provider.destroy_instance(instance_id.clone())
                    });
                return Err(match rollback {
                    Ok(_) => create_error,
                    Err(rollback_error) => RuntimeHostError::new(
                        "ASTRA_RUNTIME_HOST_CREATE_ROLLBACK",
                        format!("{create_error}; rollback failed: {rollback_error}"),
                    ),
                });
            }
        };
        if let Err(report_error) = validate_instance_report(&report, &instance_id, "created") {
            let rollback = call_provider("ASTRA_RUNTIME_HOST_CREATE_ROLLBACK", "destroy", || {
                provider.destroy_instance(instance_id.clone())
            });
            return Err(match rollback {
                Ok(_) => report_error,
                Err(rollback_error) => RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_CREATE_ROLLBACK",
                    format!("{report_error}; rollback failed: {rollback_error}"),
                ),
            });
        }
        Ok(Self {
            instance_id,
            provider,
            schemas,
            sessions: BTreeMap::new(),
            state: HostState::Created,
            runtime_binding: None,
        })
    }

    pub fn prepare(
        &mut self,
        request: RuntimePrepareRequest,
    ) -> Result<RuntimePrepareReport, RuntimeHostError> {
        self.require_state(HostState::Created, "prepare")?;
        self.validate_bound_request(&request.target_id, &request.profile)?;
        let report = call_provider("ASTRA_RUNTIME_HOST_PREPARE", "prepare", || {
            self.provider.prepare(request)
        })
        .inspect_err(|_| {
            self.state = HostState::Poisoned;
        })?;
        if let Err(error) =
            self.validate_bound_output_identity(&report.runtime_id, &report.provider_id, "prepare")
        {
            self.state = HostState::Poisoned;
            return Err(error);
        }
        Ok(report)
    }

    pub fn probe(
        &mut self,
        request: RuntimeProbeRequest,
    ) -> Result<RuntimeProbeReport, RuntimeHostError> {
        self.require_state(HostState::Created, "probe")?;
        self.validate_bound_request(&request.target_id, &request.profile)?;
        let report = call_provider("ASTRA_RUNTIME_HOST_PROBE", "probe", || {
            self.provider.probe(request)
        })
        .inspect_err(|_| {
            self.state = HostState::Poisoned;
        })?;
        if let Err(error) =
            self.validate_bound_output_identity(&report.runtime_id, &report.provider_id, "probe")
        {
            self.state = HostState::Poisoned;
            return Err(error);
        }
        Ok(report)
    }

    pub fn open(
        &mut self,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, RuntimeHostError> {
        if matches!(self.state, HostState::Destroyed | HostState::Poisoned) {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                "open is invalid after provider destruction or poisoning",
            ));
        }
        self.validate_bound_request(&request.target_id, &request.profile)?;
        let session_seed = request.seed;
        let report = call_provider("ASTRA_RUNTIME_HOST_OPEN", "open", || {
            self.provider.open(request)
        })
        .inspect_err(|_| {
            self.state = HostState::Poisoned;
        })?;
        if report.session_id.0.trim().is_empty() {
            self.state = HostState::Poisoned;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_ID",
                "provider returned an empty session id",
            ));
        }
        if let Err(identity_error) =
            self.validate_bound_output_identity(&report.runtime_id, &report.provider_id, "open")
        {
            let rollback = call_provider("ASTRA_RUNTIME_HOST_OPEN_ROLLBACK", "shutdown", || {
                self.provider.shutdown(report.session_id.clone())
            });
            self.state = HostState::Poisoned;
            return Err(match rollback {
                Ok(_) => identity_error,
                Err(rollback_error) => RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_OPEN_ROLLBACK",
                    format!("{identity_error}; rollback failed: {rollback_error}"),
                ),
            });
        }
        if self.sessions.contains_key(&report.session_id.0) {
            let rollback = call_provider("ASTRA_RUNTIME_HOST_OPEN_ROLLBACK", "shutdown", || {
                self.provider.shutdown(report.session_id.clone())
            });
            self.state = HostState::Poisoned;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_DUPLICATE",
                match rollback {
                    Ok(_) => "provider returned an already-open session id".to_string(),
                    Err(error) => format!(
                        "provider returned an already-open session id; rollback failed: {error}"
                    ),
                },
            ));
        }
        self.sessions.insert(
            report.session_id.0.clone(),
            SessionState::opened(session_seed),
        );
        self.state = HostState::Open;
        Ok(report)
    }

    fn validate_bound_request(&self, target: &str, profile: &str) -> Result<(), RuntimeHostError> {
        let Some(binding) = &self.runtime_binding else {
            return Ok(());
        };
        if target != binding.target() || profile != binding.profile() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_BINDING_CONTEXT",
                "runtime request target/profile does not match the package-selected binding",
            ));
        }
        Ok(())
    }

    fn validate_bound_output_identity(
        &self,
        runtime_id: &str,
        provider_id: &str,
        operation: &str,
    ) -> Result<(), RuntimeHostError> {
        let Some(binding) = &self.runtime_binding else {
            return Ok(());
        };
        if runtime_id != binding.descriptor().runtime_id || provider_id != binding.provider_id() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_PROVIDER_IDENTITY",
                format!(
                    "runtime provider {operation} report does not match the package-selected descriptor"
                ),
            ));
        }
        Ok(())
    }

    pub fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, RuntimeHostError> {
        self.require_session(&input.session_id, "step")?;
        let session = self.require_session_mut(&input.session_id, "step")?;
        let expected_step = session
            .last_fixed_step
            .map_or(1, |step| step.saturating_add(1));
        if input.fixed_step != expected_step {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_STEP_ORDER",
                format!("runtime fixed step must be {expected_step}"),
            ));
        }
        if input.delta_ns == 0 || input.delta_ns > 1_000_000_000 {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_DELTA",
                "runtime delta_ns must be within 1..=1000000000",
            ));
        }
        if input.session_seed != session.seed {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SEED",
                "runtime step seed does not match the opened session seed",
            ));
        }
        if input.mode == RuntimeStepMode::Replay {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_REPLAY_PROVIDER_BYPASS",
                "provider-free replay cannot invoke a live runtime provider",
            ));
        }
        if input.mode != session.next_step_mode {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_STEP_MODE",
                format!(
                    "runtime step mode must be {:?} after the preceding lifecycle operation",
                    session.next_step_mode
                ),
            ));
        }
        let expected_session = input.session_id.clone();
        let fixed_step = input.fixed_step;
        let output = call_provider("ASTRA_RUNTIME_HOST_STEP", "step", || {
            self.provider.step(input)
        });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                self.poison_session(&expected_session);
                return Err(error);
            }
        };
        if output.session_id != expected_session {
            self.poison_session(&expected_session);
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_OUTPUT_SESSION",
                "runtime output session does not match step input",
            ));
        }
        if let Err(error) = self.validate_output(&output) {
            self.poison_session(&expected_session);
            return Err(error);
        }
        self.sessions
            .get_mut(&expected_session.0)
            .expect("validated runtime session must remain registered")
            .last_fixed_step = Some(fixed_step);
        self.sessions
            .get_mut(&expected_session.0)
            .expect("validated runtime session must remain registered")
            .next_step_mode = RuntimeStepMode::Live;
        Ok(output)
    }

    pub fn save(
        &mut self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        self.require_session(&request.session_id, "save")?;
        let session_id = request.session_id.clone();
        let report = call_provider("ASTRA_RUNTIME_HOST_SAVE", "save", || {
            self.provider.save(request)
        })
        .inspect_err(|_| {
            self.poison_session(&session_id);
        })?;
        if report.session_id != session_id {
            self.poison_session(&session_id);
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SAVE_SESSION",
                "save report session does not match the requested session",
            ));
        }
        if let Err(error) = self.schemas.validate_sections(&report.sections) {
            self.poison_session(&session_id);
            return Err(error);
        }
        Ok(report)
    }

    pub fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        self.require_session(&request.session_id, "restore")?;
        let session_id = request.session_id.clone();
        self.schemas.validate_sections(&request.sections)?;
        let report = call_provider("ASTRA_RUNTIME_HOST_RESTORE", "restore", || {
            self.provider.restore(request)
        })
        .inspect_err(|_| {
            self.poison_session(&session_id);
        })?;
        if report.session_id != session_id {
            self.poison_session(&session_id);
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_RESTORE_SESSION",
                "restore report session does not match the requested session",
            ));
        }
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .expect("validated runtime session must remain registered");
        if report.session_seed != session.seed {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_RESTORE_SEED",
                "restored runtime seed does not match the opened session seed",
            ));
        }
        session.last_fixed_step = Some(report.restored_fixed_step);
        session.next_step_mode = RuntimeStepMode::RestoreContinuation;
        Ok(report)
    }

    pub fn shutdown_session(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        if !self.sessions.contains_key(&session_id.0) {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION",
                "shutdown session is not open in this provider instance",
            ));
        }
        let report = call_provider("ASTRA_RUNTIME_HOST_SHUTDOWN", "shutdown", || {
            self.provider.shutdown(session_id.clone())
        })
        .inspect_err(|_| {
            self.poison_session(&session_id);
        })?;
        if report.session_id != session_id {
            self.poison_session(&session_id);
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SHUTDOWN_SESSION",
                "shutdown report session does not match the requested session",
            ));
        }
        self.sessions.remove(&session_id.0);
        if self.sessions.is_empty() {
            self.state = HostState::Shutdown;
        }
        Ok(report)
    }

    pub fn shutdown(&mut self) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        if self.sessions.len() != 1 {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION",
                "shutdown without a session id requires exactly one open session",
            ));
        }
        let session_id = GameRuntimeSessionId(
            self.sessions
                .keys()
                .next()
                .expect("session count was checked")
                .clone(),
        );
        self.shutdown_session(session_id)
    }

    pub fn destroy(&mut self) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        if !self.sessions.is_empty()
            || matches!(self.state, HostState::Destroyed | HostState::Poisoned)
        {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                "destroy requires a created or shutdown provider instance",
            ));
        }
        let report = call_provider("ASTRA_RUNTIME_HOST_DESTROY", "destroy", || {
            self.provider.destroy_instance(self.instance_id.clone())
        })
        .inspect_err(|_| {
            self.state = HostState::Poisoned;
        })?;
        if let Err(error) = validate_instance_report(&report, &self.instance_id, "destroyed") {
            self.state = HostState::Poisoned;
            return Err(error);
        }
        self.state = HostState::Destroyed;
        Ok(report)
    }

    pub fn cleanup_after_failure(
        &mut self,
    ) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        let session_ids = self
            .sessions
            .keys()
            .cloned()
            .map(GameRuntimeSessionId)
            .collect::<Vec<_>>();
        for session_id in session_ids {
            let report = call_provider("ASTRA_RUNTIME_HOST_CLEANUP_SHUTDOWN", "shutdown", || {
                self.provider.shutdown(session_id.clone())
            })?;
            if report.session_id != session_id {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_CLEANUP_SESSION",
                    "cleanup shutdown report session does not match the requested session",
                ));
            }
            self.sessions.remove(&session_id.0);
        }
        let report = call_provider("ASTRA_RUNTIME_HOST_CLEANUP_DESTROY", "destroy", || {
            self.provider.destroy_instance(self.instance_id.clone())
        })?;
        validate_instance_report(&report, &self.instance_id, "destroyed")?;
        self.state = HostState::Destroyed;
        Ok(report)
    }

    fn validate_output(&self, output: &RuntimeStepOutput) -> Result<(), RuntimeHostError> {
        self.schemas.validate_output_bounds(output)?;
        for envelope in &output.outputs {
            self.schemas.validate(envelope.domain, envelope)?;
        }
        Ok(())
    }

    fn require_state(&self, expected: HostState, operation: &str) -> Result<(), RuntimeHostError> {
        if self.state == expected {
            Ok(())
        } else {
            Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                format!("{operation} is invalid in host state {:?}", self.state),
            ))
        }
    }

    fn require_session(
        &self,
        session_id: &GameRuntimeSessionId,
        operation: &str,
    ) -> Result<(), RuntimeHostError> {
        if matches!(self.state, HostState::Poisoned | HostState::Destroyed) {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                format!("{operation} is blocked in host state {:?}", self.state),
            ));
        }
        let Some(session) = self.sessions.get(&session_id.0) else {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION",
                format!("{operation} session does not match the open host session"),
            ));
        };
        if session.poisoned {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_POISONED",
                format!("{operation} is blocked because the runtime session is poisoned"),
            ));
        }
        Ok(())
    }

    fn require_session_mut(
        &mut self,
        session_id: &GameRuntimeSessionId,
        operation: &str,
    ) -> Result<&mut SessionState, RuntimeHostError> {
        let session = self.sessions.get_mut(&session_id.0).ok_or_else(|| {
            RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION",
                format!("{operation} session is not open in this provider instance"),
            )
        })?;
        if session.poisoned {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_POISONED",
                format!("{operation} is blocked because the runtime session is poisoned"),
            ));
        }
        Ok(session)
    }

    fn poison_session(&mut self, session_id: &GameRuntimeSessionId) {
        if let Some(session) = self.sessions.get_mut(&session_id.0) {
            session.poisoned = true;
        }
    }
}

/// Tokio facade for native/FFI providers. Provider calls run on one ordered blocking worker;
/// callers never execute provider code on an async runtime thread.
#[derive(Clone)]
pub struct AsyncProductRuntimeHost {
    inner: Arc<Mutex<ProductRuntimeHost>>,
    timeout: Duration,
    instance_poisoned: Arc<AtomicBool>,
}

impl AsyncProductRuntimeHost {
    pub fn bound_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::from_host(
            ProductRuntimeHost::bound_in_process(instance_id, selection, provider, schemas)?,
            timeout,
        )
    }

    pub fn reference_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::from_host(
            ProductRuntimeHost::reference_in_process(instance_id, provider, schemas)?,
            timeout,
        )
    }

    pub fn reference_local_serialized<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::reference_in_process(instance_id, provider, schemas, timeout)
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn bound_ffi(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        registration: FfiRuntimeProviderRegistration,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::from_host(
            ProductRuntimeHost::bound_ffi(instance_id, selection, registration, schemas)?,
            timeout,
        )
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn reference_ffi(
        instance_id: impl Into<String>,
        registration: FfiRuntimeProviderRegistration,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::from_host(
            ProductRuntimeHost::reference_ffi(instance_id, registration, schemas)?,
            timeout,
        )
    }

    fn from_host(host: ProductRuntimeHost, timeout: Duration) -> Result<Self, RuntimeHostError> {
        if timeout.is_zero() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_TIMEOUT_CONFIG",
                "runtime host timeout must be greater than zero",
            ));
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(host)),
            timeout,
            instance_poisoned: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn invoke<T, F>(&self, operation: &'static str, call: F) -> Result<T, RuntimeHostError>
    where
        T: Send + 'static,
        F: FnOnce(&mut ProductRuntimeHost) -> Result<T, RuntimeHostError> + Send + 'static,
    {
        if self.instance_poisoned.load(Ordering::Acquire) {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_INSTANCE_POISONED",
                format!("{operation} is blocked because the provider instance is poisoned"),
            ));
        }
        let inner = Arc::clone(&self.inner);
        let mut worker = tokio::task::spawn_blocking(move || {
            let mut host = inner.lock().map_err(|_| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    "runtime provider worker mutex is poisoned",
                )
            })?;
            call(&mut host)
        });
        match tokio::time::timeout(self.timeout, &mut worker).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_error)) => {
                self.instance_poisoned.store(true, Ordering::Release);
                Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    format!("runtime provider worker failed: {join_error}"),
                ))
            }
            Err(_) => {
                self.instance_poisoned.store(true, Ordering::Release);
                let _ = worker.await;
                Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_TIMEOUT",
                    format!("runtime provider {operation} timed out"),
                ))
            }
        }
    }

    pub async fn prepare(
        &self,
        request: RuntimePrepareRequest,
    ) -> Result<RuntimePrepareReport, RuntimeHostError> {
        self.invoke("prepare", move |host| host.prepare(request))
            .await
    }

    pub async fn probe(
        &self,
        request: RuntimeProbeRequest,
    ) -> Result<RuntimeProbeReport, RuntimeHostError> {
        self.invoke("probe", move |host| host.probe(request)).await
    }

    pub async fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, RuntimeHostError> {
        self.invoke("open", move |host| host.open(request)).await
    }

    pub async fn step(
        &self,
        input: RuntimeStepInput,
    ) -> Result<RuntimeStepOutput, RuntimeHostError> {
        self.invoke("step", move |host| host.step(input)).await
    }

    pub async fn save(
        &self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        self.invoke("save", move |host| host.save(request)).await
    }

    pub async fn restore(
        &self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        self.invoke("restore", move |host| host.restore(request))
            .await
    }

    pub async fn shutdown(
        &self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        self.invoke("shutdown", move |host| host.shutdown_session(session_id))
            .await
    }

    pub async fn destroy(&self) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        self.invoke("destroy", ProductRuntimeHost::destroy).await
    }

    pub async fn cleanup_after_failure(
        &self,
    ) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        let inner = Arc::clone(&self.inner);
        let report = tokio::task::spawn_blocking(move || {
            let mut host = inner.lock().map_err(|_| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    "runtime provider worker mutex is poisoned",
                )
            })?;
            host.cleanup_after_failure()
        })
        .await
        .map_err(|error| {
            RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_WORKER",
                format!("runtime provider cleanup worker failed: {error}"),
            )
        })??;
        self.instance_poisoned.store(false, Ordering::Release);
        Ok(report)
    }
}

#[cfg(feature = "dynamic-abi")]
struct FfiProductRuntimeProvider {
    registration: FfiRuntimeProviderRegistration,
    instance_id: Option<ProviderInstanceId>,
}

#[cfg(feature = "dynamic-abi")]
impl FfiProductRuntimeProvider {
    fn new(registration: FfiRuntimeProviderRegistration) -> Self {
        Self {
            registration,
            instance_id: None,
        }
    }

    fn direct<I: Serialize, O: DeserializeOwned>(
        invoke: FfiRuntimeProviderInvoke,
        input: &I,
    ) -> Result<O, String> {
        let payload = serde_json::to_vec(input).map_err(|err| err.to_string())?;
        decode_ffi_result(invoke(RVec::from(payload)))
    }

    fn instance<I: Serialize, O: DeserializeOwned>(
        &self,
        invoke: FfiRuntimeProviderInvoke,
        input: &I,
    ) -> Result<O, String> {
        let instance_id = self
            .instance_id
            .clone()
            .ok_or_else(|| "FFI provider instance is not created".to_string())?;
        let payload = serde_json::to_vec(input).map_err(|err| err.to_string())?;
        Self::direct(
            invoke,
            &RuntimeProviderCall {
                instance_id,
                payload,
            },
        )
    }
}

#[cfg(feature = "dynamic-abi")]
impl ProductRuntimeProvider for FfiProductRuntimeProvider {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        if self.registration.descriptor_schema.as_str()
            != astra_plugin_abi::PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA
        {
            return Err(
                "ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_SCHEMA: FFI descriptor schema is unsupported"
                    .to_string(),
            );
        }
        serde_json::from_slice(self.registration.descriptor_json.as_slice())
            .map_err(|error| format!("ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_DECODE: {error}"))
    }

    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        let report = Self::direct(
            self.registration.create_instance,
            &RuntimeProviderCreateRequest {
                instance_id: instance_id.clone(),
            },
        )?;
        self.instance_id = Some(instance_id);
        Ok(report)
    }

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        let report = Self::direct(
            self.registration.destroy_instance,
            &RuntimeProviderDestroyRequest { instance_id },
        )?;
        self.instance_id = None;
        Ok(report)
    }

    fn prepare(&mut self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Self::direct(self.registration.prepare, &request)
    }

    fn probe(&mut self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Self::direct(self.registration.probe, &request)
    }

    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        self.instance(self.registration.open, &request)
    }

    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        self.instance(self.registration.step, &input)
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        self.instance(self.registration.save, &request)
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        self.instance(self.registration.restore, &request)
    }

    fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        self.instance(self.registration.shutdown, &session_id)
    }
}

#[cfg(feature = "dynamic-abi")]
fn decode_ffi_result<T: DeserializeOwned>(
    result: astra_plugin_abi::FfiRuntimeProviderResult,
) -> Result<T, String> {
    if !result.ok {
        return Err(result
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "));
    }
    serde_json::from_slice(result.payload.as_slice()).map_err(|err| err.to_string())
}

fn call_provider<T>(
    code: &'static str,
    operation: &'static str,
    call: impl FnOnce() -> Result<T, String>,
) -> Result<T, RuntimeHostError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(RuntimeHostError::new(code, message)),
        Err(_) => Err(RuntimeHostError::new(
            "ASTRA_RUNTIME_HOST_PROVIDER_PANIC",
            format!("runtime provider panicked during {operation}"),
        )),
    }
}

fn validate_instance_report(
    report: &RuntimeProviderInstanceReport,
    expected_id: &ProviderInstanceId,
    expected_status: &str,
) -> Result<(), RuntimeHostError> {
    if report.instance_id != *expected_id {
        return Err(RuntimeHostError::new(
            "ASTRA_RUNTIME_HOST_INSTANCE_REPORT_ID",
            "provider instance report id does not match the host instance",
        ));
    }
    if report.status != expected_status {
        return Err(RuntimeHostError::new(
            "ASTRA_RUNTIME_HOST_INSTANCE_REPORT_STATUS",
            format!("provider instance report status must be {expected_status}"),
        ));
    }
    Ok(())
}

fn is_safe_runtime_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHostError {
    code: &'static str,
    message: String,
}

impl RuntimeHostError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for RuntimeHostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeHostError {}
