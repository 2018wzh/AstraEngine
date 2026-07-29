//! Provider-factory host with one ordered lock per runtime session.
//!
//! This is intentionally separate from the v1 host while providers migrate.
//! A factory owns only instance control-plane state; every opened session owns
//! its RuntimeWorld and is never observed through another session's lock.

use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(feature = "dynamic-abi")]
use abi_stable::std_types::RVec;
#[cfg(feature = "dynamic-abi")]
use astra_plugin_abi::{
    FfiRuntimeProviderInvoke, FfiRuntimeProviderRegistration, RuntimeProviderCall,
    RuntimeProviderCreateRequest, RuntimeProviderDestroyRequest, RuntimeProviderSessionCall,
    RuntimeProviderSessionHandle, RuntimeProviderSessionOpenReport,
    PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport,
    RuntimeProbeRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeShutdownReport,
    RuntimeStepInput, RuntimeStepMode, RuntimeStepOutput, ValidatedRuntimeProviderSelection,
};
#[cfg(feature = "dynamic-abi")]
use serde::{de::DeserializeOwned, Serialize};

use crate::{RuntimeHostError, RuntimeHostSchemaRegistry, WorkerBudgetBroker};

pub trait ProductRuntimeSession: Send {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String>;
    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String>;
    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String>;
    fn shutdown(
        self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String>;
}

pub trait ProductRuntimeProviderFactory: Send + Sync {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String>;
    fn create_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String>;
    fn destroy_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String>;
    fn prepare(&self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String>;
    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String>;
    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String>;
}

struct ConcurrentSession {
    seed: u64,
    last_fixed_step: Option<u64>,
    next_step_mode: RuntimeStepMode,
    poisoned: bool,
    session: Box<dyn ProductRuntimeSession>,
}

struct ConcurrentControl {
    destroyed: bool,
    poisoned: bool,
}

type SessionCall =
    Box<dyn FnOnce() -> Result<Box<dyn Any + Send>, RuntimeHostError> + Send + 'static>;

struct SessionCommand {
    operation: &'static str,
    call: SessionCall,
    response: tokio::sync::oneshot::Sender<Result<Box<dyn Any + Send>, RuntimeHostError>>,
}

struct SessionMailbox {
    sender: tokio::sync::mpsc::Sender<SessionCommand>,
    state: Arc<Mutex<ConcurrentSession>>,
}

const SESSION_MAILBOX_CAPACITY: usize = 32;

/// Async host that permits different sessions to execute concurrently while
/// preserving strict ordering and poisoning within each individual session.
#[derive(Clone)]
pub struct ConcurrentProductRuntimeHost {
    instance_id: ProviderInstanceId,
    factory: Arc<dyn ProductRuntimeProviderFactory>,
    schemas: RuntimeHostSchemaRegistry,
    sessions: Arc<Mutex<BTreeMap<String, Arc<SessionMailbox>>>>,
    control: Arc<Mutex<ConcurrentControl>>,
    runtime_binding: Option<ValidatedRuntimeProviderSelection>,
    timeout: Duration,
    worker_budget: WorkerBudgetBroker,
}

impl ConcurrentProductRuntimeHost {
    pub fn new<F: ProductRuntimeProviderFactory + 'static>(
        instance_id: impl Into<String>,
        factory: F,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::create(instance_id, Arc::new(factory), schemas, timeout, None)
    }

    pub fn bound_in_process<F: ProductRuntimeProviderFactory + 'static>(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        factory: F,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        let descriptor = factory.descriptor().map_err(|message| {
            RuntimeHostError::new("ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_UNAVAILABLE", message)
        })?;
        selection
            .validate_linked_descriptor(&descriptor)
            .map_err(|diagnostic| RuntimeHostError::new(diagnostic.code, diagnostic.message))?;
        Self::create(
            instance_id,
            Arc::new(factory),
            schemas,
            timeout,
            Some(selection.clone()),
        )
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn bound_ffi(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        registration: FfiRuntimeProviderRegistration,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::bound_in_process(
            instance_id,
            selection,
            FfiRuntimeProviderFactory::new(registration)?,
            schemas,
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
        Self::new(
            instance_id,
            FfiRuntimeProviderFactory::new(registration)?,
            schemas,
            timeout,
        )
    }

    fn create(
        instance_id: impl Into<String>,
        factory: Arc<dyn ProductRuntimeProviderFactory>,
        schemas: RuntimeHostSchemaRegistry,
        timeout: Duration,
        runtime_binding: Option<ValidatedRuntimeProviderSelection>,
    ) -> Result<Self, RuntimeHostError> {
        if timeout.is_zero() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_TIMEOUT_CONFIG",
                "runtime host timeout must be greater than zero",
            ));
        }
        let instance_id = ProviderInstanceId(instance_id.into());
        if instance_id.0.trim().is_empty() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_INSTANCE_ID",
                "runtime provider instance id must not be empty",
            ));
        }
        let report = factory
            .create_instance(instance_id.clone())
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_CREATE", message))?;
        if report.instance_id != instance_id || report.status != "created" {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_CREATE",
                "factory returned an invalid created instance report",
            ));
        }
        Ok(Self {
            instance_id,
            factory,
            schemas,
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            control: Arc::new(Mutex::new(ConcurrentControl {
                destroyed: false,
                poisoned: false,
            })),
            runtime_binding,
            timeout,
            worker_budget: WorkerBudgetBroker::global().clone(),
        })
    }

    pub async fn prepare(
        &self,
        request: RuntimePrepareRequest,
    ) -> Result<RuntimePrepareReport, RuntimeHostError> {
        self.require_control("prepare")?;
        self.validate_bound_request(&request.target_id, &request.profile)?;
        let factory = Arc::clone(&self.factory);
        let report = self
            .invoke("prepare", move || {
                factory
                    .prepare(request)
                    .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_PREPARE", message))
            })
            .await?;
        self.validate_bound_output_identity(&report.runtime_id, &report.provider_id, "prepare")?;
        Ok(report)
    }

    pub async fn probe(
        &self,
        request: RuntimeProbeRequest,
    ) -> Result<RuntimeProbeReport, RuntimeHostError> {
        self.require_control("probe")?;
        self.validate_bound_request(&request.target_id, &request.profile)?;
        let factory = Arc::clone(&self.factory);
        let report = self
            .invoke("probe", move || {
                factory
                    .probe(request)
                    .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_PROBE", message))
            })
            .await?;
        self.validate_bound_output_identity(&report.runtime_id, &report.provider_id, "probe")?;
        Ok(report)
    }

    pub async fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, RuntimeHostError> {
        self.require_control("open")?;
        self.validate_bound_request(&request.target_id, &request.profile)?;
        request
            .executor
            .validate()
            .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_EXECUTOR_CONFIG", message))?;
        let seed = request.seed;
        let factory = Arc::clone(&self.factory);
        let report_and_session = self
            .invoke("open", move || {
                factory
                    .open(request)
                    .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_OPEN", message))
            })
            .await?;
        let (report, session) = report_and_session;
        self.validate_bound_output_identity(&report.runtime_id, &report.provider_id, "open")?;
        if report.session_id.0.trim().is_empty() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_ID",
                "factory returned an empty session id",
            ));
        }
        let mut sessions = self.sessions.lock().map_err(|_| {
            RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_WORKER",
                "session registry mutex is poisoned",
            )
        })?;
        if sessions.contains_key(&report.session_id.0) {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_DUPLICATE",
                "factory returned an already-open session id",
            ));
        }
        let state = Arc::new(Mutex::new(ConcurrentSession {
            seed,
            last_fixed_step: None,
            next_step_mode: RuntimeStepMode::Live,
            poisoned: false,
            session,
        }));
        sessions.insert(
            report.session_id.0.clone(),
            Arc::new(self.start_session_mailbox(state)),
        );
        Ok(report)
    }

    pub async fn step(
        &self,
        input: RuntimeStepInput,
    ) -> Result<RuntimeStepOutput, RuntimeHostError> {
        let session_id = input.session_id.clone();
        let fixed_step = input.fixed_step;
        let entry = self.session(&session_id, "step")?;
        let state = Arc::clone(&entry.state);
        let poison_entry = Arc::clone(&state);
        let schemas = self.schemas.clone();
        let result = self
            .invoke_session(&entry, "step", move || {
                let mut session = state.lock().map_err(|_| {
                    RuntimeHostError::new("ASTRA_RUNTIME_HOST_WORKER", "session mutex is poisoned")
                })?;
                validate_step(&mut session, &input)?;
                let output = session
                    .session
                    .step(input)
                    .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_STEP", message));
                match output {
                    Ok(output) if output.session_id == session_id => {
                        schemas.validate_output_bounds(&output)?;
                        for envelope in &output.outputs {
                            schemas.validate(envelope.domain, envelope)?;
                        }
                        session.last_fixed_step = Some(fixed_step);
                        session.next_step_mode = RuntimeStepMode::Live;
                        Ok(output)
                    }
                    Ok(_) => {
                        session.poisoned = true;
                        Err(RuntimeHostError::new(
                            "ASTRA_RUNTIME_HOST_OUTPUT_SESSION",
                            "runtime output session does not match step input",
                        ))
                    }
                    Err(error) => {
                        session.poisoned = true;
                        Err(error)
                    }
                }
            })
            .await;
        if result.is_err() {
            poison_session(&poison_entry);
        }
        result
    }

    pub async fn save(
        &self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        let session_id = request.session_id.clone();
        let entry = self.session(&session_id, "save")?;
        let state = Arc::clone(&entry.state);
        let poison_entry = Arc::clone(&state);
        let schemas = self.schemas.clone();
        let result = self
            .invoke_session(&entry, "save", move || {
                let mut session = state.lock().map_err(|_| {
                    RuntimeHostError::new("ASTRA_RUNTIME_HOST_WORKER", "session mutex is poisoned")
                })?;
                if session.poisoned {
                    return Err(RuntimeHostError::new(
                        "ASTRA_RUNTIME_HOST_SESSION_POISONED",
                        "save is blocked because the runtime session is poisoned",
                    ));
                }
                let report = session
                    .session
                    .save(request)
                    .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_SAVE", message));
                match report {
                    Ok(report) if report.session_id == session_id => {
                        schemas.validate_sections(&report.sections)?;
                        Ok(report)
                    }
                    Ok(_) => {
                        session.poisoned = true;
                        Err(RuntimeHostError::new(
                            "ASTRA_RUNTIME_HOST_SAVE_SESSION",
                            "save report session does not match the requested session",
                        ))
                    }
                    Err(error) => {
                        session.poisoned = true;
                        Err(error)
                    }
                }
            })
            .await;
        if result.is_err() {
            poison_session(&poison_entry);
        }
        result
    }

    pub async fn restore(
        &self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        let session_id = request.session_id.clone();
        self.schemas.validate_sections(&request.sections)?;
        let entry = self.session(&session_id, "restore")?;
        let state = Arc::clone(&entry.state);
        let poison_entry = Arc::clone(&state);
        let result = self
            .invoke_session(&entry, "restore", move || {
                let mut session = state.lock().map_err(|_| {
                    RuntimeHostError::new("ASTRA_RUNTIME_HOST_WORKER", "session mutex is poisoned")
                })?;
                if session.poisoned {
                    return Err(RuntimeHostError::new(
                        "ASTRA_RUNTIME_HOST_SESSION_POISONED",
                        "restore is blocked because the runtime session is poisoned",
                    ));
                }
                let report = session.session.restore(request).map_err(|message| {
                    RuntimeHostError::new("ASTRA_RUNTIME_HOST_RESTORE", message)
                });
                match report {
                    Ok(report)
                        if report.session_id == session_id
                            && report.session_seed == session.seed =>
                    {
                        session.last_fixed_step = Some(report.restored_fixed_step);
                        session.next_step_mode = RuntimeStepMode::RestoreContinuation;
                        Ok(report)
                    }
                    Ok(_) => {
                        session.poisoned = true;
                        Err(RuntimeHostError::new(
                            "ASTRA_RUNTIME_HOST_RESTORE_IDENTITY",
                            "restore report does not match the requested session and seed",
                        ))
                    }
                    Err(error) => {
                        session.poisoned = true;
                        Err(error)
                    }
                }
            })
            .await;
        if result.is_err() {
            poison_session(&poison_entry);
        }
        result
    }

    pub async fn shutdown(
        &self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        let entry = self.session(&session_id, "shutdown")?;
        let state = Arc::clone(&entry.state);
        let poison_entry = Arc::clone(&state);
        let expected_session_id = session_id.clone();
        let report = self
            .invoke_session(&entry, "shutdown", move || {
                let mut guard = state.lock().map_err(|_| {
                    RuntimeHostError::new("ASTRA_RUNTIME_HOST_WORKER", "session mutex is poisoned")
                })?;
                let placeholder = Box::new(ClosedSession) as Box<dyn ProductRuntimeSession>;
                let session = std::mem::replace(&mut guard.session, placeholder);
                session.shutdown(session_id.clone()).map_err(|message| {
                    RuntimeHostError::new("ASTRA_RUNTIME_HOST_SHUTDOWN", message)
                })
            })
            .await;
        let report = match report {
            Ok(report)
                if report.session_id == expected_session_id && report.status == "shutdown" =>
            {
                report
            }
            Ok(_) => {
                poison_session(&poison_entry);
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_SHUTDOWN_REPORT",
                    "shutdown report has invalid session identity or status",
                ));
            }
            Err(error) => {
                poison_session(&poison_entry);
                return Err(error);
            }
        };
        self.sessions
            .lock()
            .map_err(|_| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    "session registry mutex is poisoned",
                )
            })?
            .remove(&report.session_id.0);
        Ok(report)
    }

    pub async fn destroy(&self) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        {
            let sessions = self.sessions.lock().map_err(|_| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    "session registry mutex is poisoned",
                )
            })?;
            if !sessions.is_empty() {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_LIFECYCLE",
                    "destroy requires all runtime sessions to be shut down",
                ));
            }
        }
        let factory = Arc::clone(&self.factory);
        let instance_id = self.instance_id.clone();
        let report = self
            .invoke("destroy", move || {
                factory
                    .destroy_instance(instance_id)
                    .map_err(|message| RuntimeHostError::new("ASTRA_RUNTIME_HOST_DESTROY", message))
            })
            .await?;
        let mut control = self.control.lock().map_err(|_| {
            RuntimeHostError::new("ASTRA_RUNTIME_HOST_WORKER", "control mutex is poisoned")
        })?;
        control.destroyed = true;
        Ok(report)
    }

    fn session(
        &self,
        session_id: &GameRuntimeSessionId,
        operation: &str,
    ) -> Result<Arc<SessionMailbox>, RuntimeHostError> {
        self.require_control(operation)?;
        self.sessions
            .lock()
            .map_err(|_| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_WORKER",
                    "session registry mutex is poisoned",
                )
            })?
            .get(&session_id.0)
            .cloned()
            .ok_or_else(|| {
                RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_SESSION",
                    format!("{operation} session is not open"),
                )
            })
    }

    fn start_session_mailbox(&self, state: Arc<Mutex<ConcurrentSession>>) -> SessionMailbox {
        let (sender, mut receiver) =
            tokio::sync::mpsc::channel::<SessionCommand>(SESSION_MAILBOX_CAPACITY);
        let budget = self.worker_budget.clone();
        let worker_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                let operation = command.operation;
                let call = command.call;
                let scoped_budget = budget.clone();
                let result = tokio::task::spawn_blocking(move || {
                    scoped_budget
                        .run_scoped(call)
                        .map_err(|error| RuntimeHostError::new(error.code(), error.to_string()))?
                })
                .await
                .map_err(|error| {
                    RuntimeHostError::new(
                        "ASTRA_RUNTIME_HOST_WORKER",
                        format!("{operation} mailbox worker failed: {error}"),
                    )
                })
                .and_then(|result| result);
                if result.is_err() {
                    poison_session(&worker_state);
                }
                let _ = command.response.send(result);
            }
        });
        SessionMailbox { sender, state }
    }

    async fn invoke_session<T: Send + 'static>(
        &self,
        mailbox: &SessionMailbox,
        operation: &'static str,
        call: impl FnOnce() -> Result<T, RuntimeHostError> + Send + 'static,
    ) -> Result<T, RuntimeHostError> {
        let (response, received) = tokio::sync::oneshot::channel();
        let command = SessionCommand {
            operation,
            call: Box::new(move || call().map(|value| Box::new(value) as Box<dyn Any + Send>)),
            response,
        };
        if mailbox.sender.try_send(command).is_err() {
            poison_session(&mailbox.state);
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_QUEUE_FULL",
                format!("runtime provider {operation} mailbox is closed or full"),
            ));
        }
        let result = match tokio::time::timeout(self.timeout, received).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_WORKER",
                format!("runtime provider {operation} mailbox closed without a response"),
            )),
            Err(_) => Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_TIMEOUT",
                format!("runtime provider {operation} timed out"),
            )),
        };
        let boxed = match result {
            Ok(value) => value,
            Err(error) => {
                poison_session(&mailbox.state);
                return Err(error);
            }
        };
        boxed.downcast::<T>().map(|value| *value).map_err(|_| {
            poison_session(&mailbox.state);
            RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_WORKER_TYPE",
                format!("runtime provider {operation} returned an invalid mailbox result type"),
            )
        })
    }

    fn require_control(&self, operation: &str) -> Result<(), RuntimeHostError> {
        let control = self.control.lock().map_err(|_| {
            RuntimeHostError::new("ASTRA_RUNTIME_HOST_WORKER", "control mutex is poisoned")
        })?;
        if control.destroyed || control.poisoned {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_INSTANCE_POISONED",
                format!("{operation} is blocked because the provider instance is unavailable"),
            ));
        }
        Ok(())
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

    async fn invoke<T: Send + 'static>(
        &self,
        operation: &'static str,
        call: impl FnOnce() -> Result<T, RuntimeHostError> + Send + 'static,
    ) -> Result<T, RuntimeHostError> {
        let budget = self.worker_budget.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            budget
                .run_scoped(call)
                .map_err(|error| RuntimeHostError::new(error.code(), error.to_string()))?
        });
        match tokio::time::timeout(self.timeout, &mut worker).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_WORKER",
                format!("{operation} worker failed: {error}"),
            )),
            Err(_) => Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_TIMEOUT",
                format!("runtime provider {operation} timed out"),
            )),
        }
    }
}

struct ClosedSession;
impl ProductRuntimeSession for ClosedSession {
    fn step(&mut self, _: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        Err("ASTRA_RUNTIME_HOST_SESSION_CLOSED".into())
    }
    fn save(&mut self, _: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        Err("ASTRA_RUNTIME_HOST_SESSION_CLOSED".into())
    }
    fn restore(&mut self, _: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        Err("ASTRA_RUNTIME_HOST_SESSION_CLOSED".into())
    }
    fn shutdown(self: Box<Self>, _: GameRuntimeSessionId) -> Result<RuntimeShutdownReport, String> {
        Err("ASTRA_RUNTIME_HOST_SESSION_CLOSED".into())
    }
}

#[cfg(feature = "dynamic-abi")]
struct FfiRuntimeProviderFactory {
    registration: FfiRuntimeProviderRegistration,
    instance_id: Mutex<Option<ProviderInstanceId>>,
}

#[cfg(feature = "dynamic-abi")]
impl FfiRuntimeProviderFactory {
    fn new(registration: FfiRuntimeProviderRegistration) -> Result<Self, RuntimeHostError> {
        if registration.abi_version != PRODUCT_RUNTIME_PROVIDER_ABI_VERSION {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_PROVIDER_ABI_VERSION",
                format!(
                    "runtime provider ABI {} is unsupported; expected {}",
                    registration.abi_version, PRODUCT_RUNTIME_PROVIDER_ABI_VERSION
                ),
            ));
        }
        Ok(Self {
            registration,
            instance_id: Mutex::new(None),
        })
    }

    fn direct<I: Serialize, O: DeserializeOwned>(
        invoke: FfiRuntimeProviderInvoke,
        input: &I,
    ) -> Result<O, String> {
        let bytes = serde_json::to_vec(input).map_err(|error| error.to_string())?;
        decode_ffi_result(invoke(RVec::from(bytes)))
    }

    fn instance<I: Serialize, O: DeserializeOwned>(
        &self,
        invoke: FfiRuntimeProviderInvoke,
        input: &I,
    ) -> Result<O, String> {
        let instance_id = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_LOCK".to_string())?
            .clone()
            .ok_or_else(|| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_MISSING".to_string())?;
        let payload = serde_json::to_vec(input).map_err(|error| error.to_string())?;
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
impl ProductRuntimeProviderFactory for FfiRuntimeProviderFactory {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        if self.registration.descriptor_schema.as_str()
            != astra_plugin_abi::PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA
        {
            return Err("ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_SCHEMA".to_string());
        }
        serde_json::from_slice(self.registration.descriptor_json.as_slice())
            .map_err(|error| format!("ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_DECODE: {error}"))
    }

    fn create_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        let report = Self::direct(
            self.registration.create_instance,
            &RuntimeProviderCreateRequest {
                instance_id: instance_id.clone(),
            },
        )?;
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_LOCK".to_string())?;
        if current.replace(instance_id).is_some() {
            return Err("ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_DUPLICATE".to_string());
        }
        Ok(report)
    }

    fn destroy_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        let report = Self::direct(
            self.registration.destroy_instance,
            &RuntimeProviderDestroyRequest {
                instance_id: instance_id.clone(),
            },
        )?;
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_LOCK".to_string())?;
        if current.as_ref() != Some(&instance_id) {
            return Err("ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_MISMATCH".to_string());
        }
        *current = None;
        Ok(report)
    }

    fn prepare(&self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Self::direct(self.registration.prepare, &request)
    }

    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Self::direct(self.registration.probe, &request)
    }

    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String> {
        let opened: RuntimeProviderSessionOpenReport =
            self.instance(self.registration.open_session, &request)?;
        Ok((
            opened.report.clone(),
            Box::new(FfiRuntimeSession {
                registration: self.registration.clone(),
                instance_id: self
                    .instance_id
                    .lock()
                    .map_err(|_| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_LOCK".to_string())?
                    .clone()
                    .ok_or_else(|| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_MISSING".to_string())?,
                session_id: opened.report.session_id,
                session_handle: opened.session_handle,
            }),
        ))
    }
}

#[cfg(feature = "dynamic-abi")]
struct FfiRuntimeSession {
    registration: FfiRuntimeProviderRegistration,
    instance_id: ProviderInstanceId,
    session_id: GameRuntimeSessionId,
    session_handle: RuntimeProviderSessionHandle,
}

#[cfg(feature = "dynamic-abi")]
impl FfiRuntimeSession {
    fn invoke<I: Serialize, O: DeserializeOwned>(
        &self,
        callback: FfiRuntimeProviderInvoke,
        input: &I,
    ) -> Result<O, String> {
        let payload = serde_json::to_vec(input).map_err(|error| error.to_string())?;
        FfiRuntimeProviderFactory::direct(
            callback,
            &RuntimeProviderSessionCall {
                instance_id: self.instance_id.clone(),
                session_handle: self.session_handle,
                payload,
            },
        )
    }
}

#[cfg(feature = "dynamic-abi")]
impl ProductRuntimeSession for FfiRuntimeSession {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        self.invoke(self.registration.step, &input)
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        self.invoke(self.registration.save, &request)
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        self.invoke(self.registration.restore, &request)
    }

    fn shutdown(
        self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        if session_id != self.session_id {
            return Err("ASTRA_RUNTIME_PROVIDER_FFI_SESSION_MISMATCH".to_string());
        }
        self.invoke(self.registration.shutdown, &session_id)
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
    serde_json::from_slice(result.payload.as_slice()).map_err(|error| error.to_string())
}

fn validate_step(
    session: &mut ConcurrentSession,
    input: &RuntimeStepInput,
) -> Result<(), RuntimeHostError> {
    if session.poisoned {
        return Err(RuntimeHostError::new(
            "ASTRA_RUNTIME_HOST_SESSION_POISONED",
            "runtime session is poisoned",
        ));
    }
    let expected = session
        .last_fixed_step
        .map_or(1, |step| step.saturating_add(1));
    if input.fixed_step != expected
        || input.delta_ns == 0
        || input.delta_ns > 1_000_000_000
        || input.session_seed != session.seed
        || input.mode != session.next_step_mode
        || input.mode == RuntimeStepMode::Replay
    {
        session.poisoned = true;
        return Err(RuntimeHostError::new(
            "ASTRA_RUNTIME_HOST_STEP_ORDER",
            "runtime step violates the session lifecycle contract",
        ));
    }
    Ok(())
}

fn poison_session(entry: &Arc<Mutex<ConcurrentSession>>) {
    match entry.lock() {
        Ok(mut session) => session.poisoned = true,
        Err(poisoned) => poisoned.into_inner().poisoned = true,
    }
}
