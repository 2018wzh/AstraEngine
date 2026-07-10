use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use abi_stable::std_types::RVec;
use astra_core::SchemaVersion;
use astra_plugin_abi::{
    FfiRuntimeProviderInvoke, FfiRuntimeProviderRegistration, GameRuntimeSessionId,
    ProductRuntimeDescriptor, ProviderInstanceId, RuntimeOpenReport, RuntimeOpenRequest,
    RuntimeOutputDomain, RuntimeOutputEnvelope, RuntimePrepareReport, RuntimePrepareRequest,
    RuntimeProbeReport, RuntimeProbeRequest, RuntimeProviderCall, RuntimeProviderCreateRequest,
    RuntimeProviderDestroyRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeShutdownReport,
    RuntimeStepInput, RuntimeStepOutput,
};
use serde::{de::DeserializeOwned, Serialize};

pub trait ProductRuntimeProvider: Send {
    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".to_string(),
            diagnostics: Vec::new(),
        })
    }

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".to_string(),
            diagnostics: Vec::new(),
        })
    }

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

#[derive(Debug, Clone, Copy, Default)]
struct SessionState {
    last_fixed_step: Option<u64>,
    poisoned: bool,
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
        envelopes: &[RuntimeOutputEnvelope],
    ) -> Result<(), RuntimeHostError> {
        let allowed = self.schemas.get(&domain);
        for envelope in envelopes {
            let Some((schema, version)) = allowed
                .and_then(|schemas| schemas.get(&(envelope.schema.clone(), envelope.version)))
            else {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_ENVELOPE_SCHEMA",
                    format!("unknown {:?} output schema {}", domain, envelope.schema),
                ));
            };
            envelope
                .validate_binding(domain, schema, *version)
                .map_err(|err| RuntimeHostError::new(err.code(), err.to_string()))?;
        }
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
            total.checked_add(envelope.bytes.len()).ok_or_else(|| {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostState {
    Created,
    Open,
    Shutdown,
    Destroyed,
}

pub struct ProductRuntimeHost {
    instance_id: ProviderInstanceId,
    provider: Box<dyn ProductRuntimeProvider>,
    schemas: RuntimeHostSchemaRegistry,
    sessions: BTreeMap<String, SessionState>,
    state: HostState,
}

impl ProductRuntimeHost {
    pub fn in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
    ) -> Result<Self, RuntimeHostError> {
        Self::create(instance_id, Box::new(provider), schemas)
    }

    pub fn ffi(
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
        provider
            .create_instance(instance_id.clone())
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_CREATE", message))?;
        Ok(Self {
            instance_id,
            provider,
            schemas,
            sessions: BTreeMap::new(),
            state: HostState::Created,
        })
    }

    pub fn prepare(
        &mut self,
        request: RuntimePrepareRequest,
    ) -> Result<RuntimePrepareReport, RuntimeHostError> {
        self.require_state(HostState::Created, "prepare")?;
        self.provider
            .prepare(request)
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_PREPARE", message))
    }

    pub fn probe(
        &mut self,
        request: RuntimeProbeRequest,
    ) -> Result<RuntimeProbeReport, RuntimeHostError> {
        self.require_state(HostState::Created, "probe")?;
        self.provider
            .probe(request)
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_PROBE", message))
    }

    pub fn open(
        &mut self,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, RuntimeHostError> {
        if self.state == HostState::Destroyed {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                "open is invalid after provider destruction",
            ));
        }
        let report = self
            .provider
            .open(request)
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_OPEN", message))?;
        if self
            .sessions
            .insert(report.session_id.0.clone(), SessionState::default())
            .is_some()
        {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_DUPLICATE",
                "provider returned an already-open session id",
            ));
        }
        self.state = HostState::Open;
        Ok(report)
    }

    pub fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, RuntimeHostError> {
        self.require_session(&input.session_id, "step")?;
        let session = self.require_session_mut(&input.session_id, "step")?;
        if session
            .last_fixed_step
            .is_some_and(|last_step| input.fixed_step <= last_step)
        {
            session.poisoned = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_STEP_ORDER",
                "runtime fixed steps must be strictly increasing",
            ));
        }
        let expected_session = input.session_id.clone();
        let fixed_step = input.fixed_step;
        let output = self.provider.step(input);
        let output = match output {
            Ok(output) => output,
            Err(message) => {
                self.poison_session(&expected_session);
                return Err(RuntimeHostError::new("ASTRA_RUNTIME_HOST_STEP", message));
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
        Ok(output)
    }

    pub fn save(
        &mut self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        self.require_session(&request.session_id, "save")?;
        self.provider
            .save(request)
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_SAVE", message))
    }

    pub fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        self.require_session(&request.session_id, "restore")?;
        self.provider
            .restore(request)
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_RESTORE", message))
    }

    pub fn shutdown_session(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        self.require_session(&session_id, "shutdown")?;
        let report = self
            .provider
            .shutdown(session_id.clone())
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_SHUTDOWN", message))?;
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
        if !self.sessions.is_empty() || self.state == HostState::Destroyed {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                "destroy requires a created or shutdown provider instance",
            ));
        }
        let report = self
            .provider
            .destroy_instance(self.instance_id.clone())
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_DESTROY", message))?;
        self.state = HostState::Destroyed;
        Ok(report)
    }

    fn validate_output(&self, output: &RuntimeStepOutput) -> Result<(), RuntimeHostError> {
        self.schemas.validate_output_bounds(output)?;
        for domain in [
            RuntimeOutputDomain::Effect,
            RuntimeOutputDomain::Presentation,
            RuntimeOutputDomain::Audio,
            RuntimeOutputDomain::Await,
            RuntimeOutputDomain::Trace,
            RuntimeOutputDomain::DirtySaveSection,
        ] {
            let envelopes = output
                .outputs
                .iter()
                .filter(|envelope| envelope.domain == domain)
                .cloned()
                .collect::<Vec<_>>();
            self.schemas.validate(domain, &envelopes)?;
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
    pub fn in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::from_host(
            ProductRuntimeHost::in_process(instance_id, provider, schemas)?,
            timeout,
        )
    }

    pub fn local_serialized<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::in_process(instance_id, provider, schemas, timeout)
    }

    pub fn ffi(
        instance_id: impl Into<String>,
        registration: FfiRuntimeProviderRegistration,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::from_host(
            ProductRuntimeHost::ffi(instance_id, registration, schemas)?,
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
        let worker = tokio::task::spawn_blocking(move || {
            let mut host = inner.lock().map_err(|_| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    "runtime provider worker mutex is poisoned",
                )
            })?;
            call(&mut host)
        });
        match tokio::time::timeout(self.timeout, worker).await {
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
}

struct FfiProductRuntimeProvider {
    registration: FfiRuntimeProviderRegistration,
    instance_id: Option<ProviderInstanceId>,
}

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

impl ProductRuntimeProvider for FfiProductRuntimeProvider {
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
