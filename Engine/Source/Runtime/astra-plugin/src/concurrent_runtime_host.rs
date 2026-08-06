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

use astra_core::SchemaVersion;

#[cfg(feature = "dynamic-abi")]
use abi_stable::std_types::RVec;
#[cfg(feature = "dynamic-abi")]
use astra_plugin_abi::{
    FfiRuntimeAudioBus, FfiRuntimeAudioCommandKind, FfiRuntimeAudioEncoding,
    FfiRuntimeAudioSampleFormat, FfiRuntimeAudioSyncKind, FfiRuntimeAwaitResult,
    FfiRuntimeBlendMode, FfiRuntimeInputEdge, FfiRuntimeInstanceRequest, FfiRuntimeIntegrityMode,
    FfiRuntimeLiveOutput, FfiRuntimeOpenRequest, FfiRuntimePcmBuffer, FfiRuntimePersistedOutput,
    FfiRuntimePrepareRequest, FfiRuntimeProbeRequest, FfiRuntimeProviderRegistration,
    FfiRuntimeProviderResultItem, FfiRuntimeReportResult, FfiRuntimeRestoreRequest,
    FfiRuntimeSaveRequest, FfiRuntimeSaveResult, FfiRuntimeSceneResourceOperation,
    FfiRuntimeSection, FfiRuntimeSectionCodec, FfiRuntimeShutdownRequest, FfiRuntimeStepMode,
    FfiRuntimeStepRequest, FfiRuntimeStepResult, FfiRuntimeTextureFormat, FfiRuntimeVideoMode,
    FfiRuntimeWaitKind, PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeLiveAudioBus,
    RuntimeLiveAudioCommand, RuntimeLiveAudioCue, RuntimeLiveAudioEncoding, RuntimeLiveAudioPacket,
    RuntimeLiveAudioSampleFormat, RuntimeLiveAudioSync, RuntimeLiveBlendMode, RuntimeLiveControl,
    RuntimeLiveCoverage, RuntimeLiveEffect, RuntimeLiveEvent, RuntimeLiveOutput,
    RuntimeLivePcmBuffer, RuntimeLiveResourceScene, RuntimeLiveResourceTexture,
    RuntimeLiveSceneResourceOperation, RuntimeLiveSceneTransaction, RuntimeLiveScissor,
    RuntimeLiveTextLease, RuntimeLiveTextPresentation, RuntimeLiveTextRegion,
    RuntimeLiveTextureFormat, RuntimeLiveVideoCommand, RuntimeLiveVideoCommandKind,
    RuntimeLiveVideoMode, RuntimeLiveWait, RuntimeLiveWaitKind, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimeOutputDomain, RuntimePersistedOutput, RuntimePrepareReport,
    RuntimePrepareRequest, RuntimeProbeReport, RuntimeProbeRequest, RuntimeProviderInstanceReport,
    RuntimeRestoreReport, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections,
    RuntimeSectionCodec, RuntimeSectionPayload, RuntimeShutdownReport, RuntimeStepInput,
    RuntimeStepMode, RuntimeStepOutput, ValidatedRuntimeProviderSelection,
};

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
                        for persisted in &output.persisted {
                            schemas.validate(persisted.domain, persisted)?;
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
pub(crate) struct FfiRuntimeProviderFactory {
    registration: FfiRuntimeProviderRegistration,
    instance_id: Mutex<Option<ProviderInstanceId>>,
}

#[cfg(feature = "dynamic-abi")]
impl FfiRuntimeProviderFactory {
    pub(crate) fn new(
        registration: FfiRuntimeProviderRegistration,
    ) -> Result<Self, RuntimeHostError> {
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
        let report = (self.registration.create_instance)(FfiRuntimeInstanceRequest {
            instance_id: instance_id.0.clone().into(),
        });
        let mut report = runtime_report(report)?;
        report.instance_id = instance_id.clone();
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
        let mut report = runtime_report((self.registration.destroy_instance)(
            FfiRuntimeInstanceRequest {
                instance_id: instance_id.0.clone().into(),
            },
        ))?;
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_LOCK".to_string())?;
        if current.as_ref() != Some(&instance_id) {
            return Err("ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_MISMATCH".to_string());
        }
        *current = None;
        report.instance_id = instance_id;
        Ok(report)
    }

    fn prepare(&self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        let result = (self.registration.prepare)(FfiRuntimePrepareRequest {
            target_id: request.target_id.into(),
            profile: request.profile.into(),
            package_id: request.package_hash.into(),
            section_ids: RVec::from(
                request
                    .section_ids
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>(),
            ),
        });
        let (runtime_id, provider_id, status, diagnostics) = runtime_provider_report(result)?;
        Ok(RuntimePrepareReport {
            runtime_id,
            provider_id,
            status,
            diagnostics,
        })
    }

    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        let result = (self.registration.probe)(FfiRuntimeProbeRequest {
            target_id: request.target_id.into(),
            profile: request.profile.into(),
            platform: request.platform.map(Into::into).into(),
            section_ids: RVec::from(
                request
                    .section_ids
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>(),
            ),
        });
        let (runtime_id, provider_id, status, diagnostics) = runtime_provider_report(result)?;
        Ok(RuntimeProbeReport {
            runtime_id,
            provider_id,
            status,
            diagnostics,
        })
    }

    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String> {
        let instance_id = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_LOCK".to_string())?
            .clone()
            .ok_or_else(|| "ASTRA_RUNTIME_PROVIDER_FFI_INSTANCE_MISSING".to_string())?;
        let result =
            (self.registration.open_session)(ffi_open_request(instance_id.clone(), request)?);
        let opened = runtime_open_result(result)?;
        Ok((
            opened.0.clone(),
            Box::new(FfiRuntimeSession {
                registration: self.registration.clone(),
                instance_id,
                session_id: opened.0.session_id.clone(),
                session_handle: opened.1,
            }),
        ))
    }
}

#[cfg(feature = "dynamic-abi")]
struct FfiRuntimeSession {
    registration: FfiRuntimeProviderRegistration,
    instance_id: ProviderInstanceId,
    session_id: GameRuntimeSessionId,
    session_handle: u64,
}

#[cfg(feature = "dynamic-abi")]
impl ProductRuntimeSession for FfiRuntimeSession {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        let request = ffi_step_request(&self.instance_id, self.session_handle, input)?;
        runtime_step_result((self.registration.step)(request))
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        let result = (self.registration.save)(FfiRuntimeSaveRequest {
            instance_id: self.instance_id.0.clone().into(),
            session_handle: self.session_handle,
            session_id: request.session_id.0.into(),
            slot: request.slot.into(),
        });
        runtime_save_result(result)
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        let result = (self.registration.restore)(FfiRuntimeRestoreRequest {
            instance_id: self.instance_id.0.clone().into(),
            session_handle: self.session_handle,
            session_id: request.session_id.0.into(),
            sections: RVec::from(
                request
                    .sections
                    .into_iter()
                    .map(ffi_section)
                    .collect::<Vec<_>>(),
            ),
        });
        runtime_restore_result(result)
    }

    fn shutdown(
        self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        if session_id != self.session_id {
            return Err("ASTRA_RUNTIME_PROVIDER_FFI_SESSION_MISMATCH".to_string());
        }
        runtime_shutdown_result((self.registration.shutdown)(FfiRuntimeShutdownRequest {
            instance_id: self.instance_id.0.into(),
            session_handle: self.session_handle,
            session_id: session_id.0.into(),
        }))
    }
}

#[cfg(feature = "dynamic-abi")]
fn diagnostics(ok: bool, values: &[abi_stable::std_types::RString]) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "))
    }
}

#[cfg(feature = "dynamic-abi")]
fn runtime_report(
    result: astra_plugin_abi::FfiRuntimeReportResult,
) -> Result<RuntimeProviderInstanceReport, String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    Ok(RuntimeProviderInstanceReport {
        instance_id: ProviderInstanceId(String::new()),
        status: result.status.to_string(),
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_provider_report(
    result: FfiRuntimeReportResult,
) -> Result<(String, String, String, Vec<String>), String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    Ok((
        result.runtime_id.to_string(),
        result.provider_id.to_string(),
        result.status.to_string(),
        result
            .diagnostics
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    ))
}

#[cfg(feature = "dynamic-abi")]
fn ffi_section(section: RuntimeSectionPayload) -> FfiRuntimeSection {
    FfiRuntimeSection {
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

#[cfg(feature = "dynamic-abi")]
fn ffi_open_request(
    instance_id: ProviderInstanceId,
    request: RuntimeOpenRequest,
) -> Result<FfiRuntimeOpenRequest, String> {
    if request.executor.worker_count == 0 {
        return Err("ASTRA_RUNTIME_PROVIDER_EXECUTOR_WORKERS".into());
    }
    Ok(FfiRuntimeOpenRequest {
        instance_id: instance_id.0.into(),
        target_id: request.target_id.into(),
        profile: request.profile.into(),
        locale: request.locale.into(),
        seed: request.seed,
        integrity_mode: match request.integrity_mode {
            astra_plugin_abi::RuntimeTickIntegrityMode::Shipping => {
                FfiRuntimeIntegrityMode::Shipping
            }
            astra_plugin_abi::RuntimeTickIntegrityMode::Evidence => {
                FfiRuntimeIntegrityMode::Evidence
            }
        },
        worker_count: request.executor.worker_count,
        package_id: request.package_hash.into(),
        sections: RVec::from(
            request
                .sections
                .into_iter()
                .map(ffi_section)
                .collect::<Vec<_>>(),
        ),
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_open_result(
    result: astra_plugin_abi::FfiRuntimeOpenResult,
) -> Result<(RuntimeOpenReport, u64), String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    Ok((
        RuntimeOpenReport {
            session_id: GameRuntimeSessionId(result.session_id.to_string()),
            runtime_id: result.runtime_id.to_string(),
            provider_id: result.provider_id.to_string(),
            diagnostics: result.diagnostics.iter().map(ToString::to_string).collect(),
        },
        result.session_handle,
    ))
}

#[cfg(feature = "dynamic-abi")]
fn ffi_step_request(
    instance_id: &ProviderInstanceId,
    session_handle: u64,
    input: RuntimeStepInput,
) -> Result<FfiRuntimeStepRequest, String> {
    Ok(FfiRuntimeStepRequest {
        instance_id: instance_id.0.clone().into(),
        session_handle,
        session_id: input.session_id.0.into(),
        fixed_step: input.fixed_step,
        delta_ns: input.delta_ns,
        session_seed: input.session_seed,
        mode: match input.mode {
            RuntimeStepMode::Live => FfiRuntimeStepMode::Live,
            RuntimeStepMode::RestoreContinuation => FfiRuntimeStepMode::RestoreContinuation,
        },
        action: input.action.into(),
        argument: input.argument.map(Into::into).into(),
        auxiliary: input.auxiliary.map(Into::into).into(),
        flag: input.flag.into(),
        input_edges: RVec::from(
            input
                .input_edges
                .into_iter()
                .map(|edge| FfiRuntimeInputEdge {
                    control: edge.control.into(),
                    pressed: edge.pressed,
                    value: edge.value,
                    sequence: edge.sequence,
                })
                .collect::<Vec<_>>(),
        ),
        await_results: RVec::from(
            input
                .await_results
                .into_iter()
                .map(|result| FfiRuntimeAwaitResult {
                    token_id: result.token_id.into(),
                    status: result.status.into(),
                    payload_len: result.payload_len,
                    sequence: result.sequence,
                })
                .collect::<Vec<_>>(),
        ),
        provider_results: RVec::from(
            input
                .provider_results
                .into_iter()
                .map(|result| FfiRuntimeProviderResultItem {
                    request_id: result.request_id.into(),
                    provider_id: result.provider_id.into(),
                    status: result.status.into(),
                    payload_len: result.payload_len,
                    sequence: result.sequence,
                })
                .collect::<Vec<_>>(),
        ),
        max_instructions: input.budget.max_instructions,
        max_effects: input.budget.max_effects,
        max_trace_entries: input.budget.max_trace_entries,
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_live_scene(
    transaction: astra_plugin_abi::FfiRuntimeSceneTransaction,
) -> RuntimeLiveSceneTransaction {
    RuntimeLiveSceneTransaction {
        sequence: transaction.sequence,
        width: transaction.width,
        height: transaction.height,
        resources: transaction
            .resources
            .into_iter()
            .map(|operation| match operation {
                FfiRuntimeSceneResourceOperation::Create(value) => {
                    RuntimeLiveSceneResourceOperation::CreateTexture {
                        texture_id: value.texture_id,
                        generation: value.generation,
                        width: value.width,
                        height: value.height,
                        format: match value.format {
                            FfiRuntimeTextureFormat::Rgba8 => RuntimeLiveTextureFormat::Rgba8,
                            FfiRuntimeTextureFormat::LumaAlpha8 => {
                                RuntimeLiveTextureFormat::LumaAlpha8
                            }
                        },
                        pixels: value.pixels.into_vec(),
                    }
                }
                FfiRuntimeSceneResourceOperation::Update(value) => {
                    RuntimeLiveSceneResourceOperation::UpdateTexture {
                        texture_id: value.texture_id,
                        generation: value.generation,
                        x: value.x,
                        y: value.y,
                        width: value.width,
                        height: value.height,
                        format: match value.format {
                            FfiRuntimeTextureFormat::Rgba8 => RuntimeLiveTextureFormat::Rgba8,
                            FfiRuntimeTextureFormat::LumaAlpha8 => {
                                RuntimeLiveTextureFormat::LumaAlpha8
                            }
                        },
                        pixels: value.pixels.into_vec(),
                    }
                }
                FfiRuntimeSceneResourceOperation::Destroy {
                    texture_id,
                    generation,
                } => RuntimeLiveSceneResourceOperation::DestroyTexture {
                    texture_id,
                    generation,
                },
            })
            .collect(),
        draws: transaction
            .draws
            .into_iter()
            .map(|draw| astra_plugin_abi::RuntimeLiveDraw {
                texture_id: draw.texture_id,
                vertices: draw
                    .vertices
                    .map(|vertex| astra_plugin_abi::RuntimeLiveVertex {
                        x: vertex.x,
                        y: vertex.y,
                        u: vertex.u,
                        v: vertex.v,
                        color: [vertex.r, vertex.g, vertex.b, vertex.a],
                    }),
                blend: match draw.blend {
                    FfiRuntimeBlendMode::Alpha => RuntimeLiveBlendMode::Alpha,
                    FfiRuntimeBlendMode::Additive => RuntimeLiveBlendMode::Additive,
                    FfiRuntimeBlendMode::Opaque => RuntimeLiveBlendMode::Opaque,
                    FfiRuntimeBlendMode::Multiply => RuntimeLiveBlendMode::Multiply,
                    FfiRuntimeBlendMode::Screen => RuntimeLiveBlendMode::Screen,
                },
                scissor: draw.scissor.into_option().map(|value| RuntimeLiveScissor {
                    x: value.x,
                    y: value.y,
                    width: value.width,
                    height: value.height,
                }),
            })
            .collect(),
        reset_resources: transaction.reset_resources,
    }
}

#[cfg(feature = "dynamic-abi")]
fn runtime_live_draw(draw: astra_plugin_abi::FfiRuntimeDraw) -> astra_plugin_abi::RuntimeLiveDraw {
    astra_plugin_abi::RuntimeLiveDraw {
        texture_id: draw.texture_id,
        vertices: draw
            .vertices
            .map(|vertex| astra_plugin_abi::RuntimeLiveVertex {
                x: vertex.x,
                y: vertex.y,
                u: vertex.u,
                v: vertex.v,
                color: [vertex.r, vertex.g, vertex.b, vertex.a],
            }),
        blend: match draw.blend {
            FfiRuntimeBlendMode::Alpha => RuntimeLiveBlendMode::Alpha,
            FfiRuntimeBlendMode::Additive => RuntimeLiveBlendMode::Additive,
            FfiRuntimeBlendMode::Opaque => RuntimeLiveBlendMode::Opaque,
            FfiRuntimeBlendMode::Multiply => RuntimeLiveBlendMode::Multiply,
            FfiRuntimeBlendMode::Screen => RuntimeLiveBlendMode::Screen,
        },
        scissor: draw.scissor.into_option().map(|value| RuntimeLiveScissor {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }),
    }
}

#[cfg(feature = "dynamic-abi")]
fn runtime_live_audio_command(
    command: astra_plugin_abi::FfiRuntimeAudioCommand,
) -> Result<RuntimeLiveAudioCommand, String> {
    let astra_plugin_abi::FfiRuntimeAudioCommand {
        sequence,
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
    } = command;
    let encoding = match encoding {
        FfiRuntimeAudioEncoding::Unknown => RuntimeLiveAudioEncoding::Unknown,
        FfiRuntimeAudioEncoding::Wav => RuntimeLiveAudioEncoding::Wav,
        FfiRuntimeAudioEncoding::Ogg => RuntimeLiveAudioEncoding::Ogg,
        FfiRuntimeAudioEncoding::Mp3 => RuntimeLiveAudioEncoding::Mp3,
        FfiRuntimeAudioEncoding::Flac => RuntimeLiveAudioEncoding::Flac,
    };
    let sample_format = match sample_format {
        FfiRuntimeAudioSampleFormat::I16 => RuntimeLiveAudioSampleFormat::I16,
        FfiRuntimeAudioSampleFormat::F32 => RuntimeLiveAudioSampleFormat::F32,
    };
    let resource_uri = resource_uri.to_string();
    let samples = match (kind, samples) {
        (FfiRuntimeAudioCommandKind::SubmitI16, FfiRuntimePcmBuffer::I16(values)) => {
            Some(RuntimeLivePcmBuffer::I16(values.into_vec()))
        }
        (FfiRuntimeAudioCommandKind::SubmitF32, FfiRuntimePcmBuffer::F32(values)) => {
            Some(RuntimeLivePcmBuffer::F32(values.into_vec()))
        }
        (FfiRuntimeAudioCommandKind::SubmitI16, FfiRuntimePcmBuffer::F32(_))
        | (FfiRuntimeAudioCommandKind::SubmitF32, FfiRuntimePcmBuffer::I16(_)) => {
            return Err("ASTRA_RUNTIME_AUDIO_SAMPLE_FORMAT_MISMATCH".into())
        }
        (_, FfiRuntimePcmBuffer::I16(values)) if !values.is_empty() => {
            return Err("ASTRA_RUNTIME_AUDIO_UNEXPECTED_I16_PAYLOAD".into())
        }
        (_, FfiRuntimePcmBuffer::F32(values)) if !values.is_empty() => {
            return Err("ASTRA_RUNTIME_AUDIO_UNEXPECTED_F32_PAYLOAD".into())
        }
        (_, _) => None,
    };
    Ok(match kind {
        FfiRuntimeAudioCommandKind::LoadResource => RuntimeLiveAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding,
            resource_uri,
        },
        FfiRuntimeAudioCommandKind::CreateStream => RuntimeLiveAudioCommand::CreateStream {
            sequence,
            stream_id,
            sample_rate,
            channels,
            sample_format,
        },
        FfiRuntimeAudioCommandKind::SubmitI16 => RuntimeLiveAudioCommand::SubmitI16 {
            sequence,
            stream_id,
            samples: match samples.expect("sample command payload was checked") {
                RuntimeLivePcmBuffer::I16(values) => values,
                RuntimeLivePcmBuffer::F32(_) => unreachable!(),
            },
        },
        FfiRuntimeAudioCommandKind::SubmitF32 => RuntimeLiveAudioCommand::SubmitF32 {
            sequence,
            stream_id,
            samples: match samples.expect("sample command payload was checked") {
                RuntimeLivePcmBuffer::F32(values) => values,
                RuntimeLivePcmBuffer::I16(_) => unreachable!(),
            },
        },
        FfiRuntimeAudioCommandKind::Play => RuntimeLiveAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms: fade_ms,
        },
        FfiRuntimeAudioCommandKind::Stop => RuntimeLiveAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        },
        FfiRuntimeAudioCommandKind::Pause => RuntimeLiveAudioCommand::Pause {
            sequence,
            stream_id,
        },
        FfiRuntimeAudioCommandKind::Resume => RuntimeLiveAudioCommand::Resume {
            sequence,
            stream_id,
        },
        FfiRuntimeAudioCommandKind::SetParams => RuntimeLiveAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        },
        FfiRuntimeAudioCommandKind::DestroyStream => RuntimeLiveAudioCommand::DestroyStream {
            sequence,
            stream_id,
        },
        FfiRuntimeAudioCommandKind::MasterVolume => {
            RuntimeLiveAudioCommand::MasterVolume { sequence, volume }
        }
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_live_output(value: FfiRuntimeLiveOutput) -> Result<RuntimeLiveOutput, String> {
    let mut effects = Vec::with_capacity(
        value.scenes.len()
            + value.resource_scenes.len()
            + value.audio.len()
            + value.audio_commands.len()
            + value.text.len()
            + value.text_presentations.len()
            + value.video.len()
            + value.waits.len()
            + value.events.len()
            + value.blackboard.len()
            + value.dirty_sections.len(),
    );
    effects.extend(
        value
            .scenes
            .into_iter()
            .map(|scene| RuntimeLiveEffect::Scene(runtime_live_scene(scene))),
    );
    effects.extend(value.resource_scenes.into_iter().map(|scene| {
        RuntimeLiveEffect::ResourceScene(RuntimeLiveResourceScene {
            sequence: scene.sequence,
            width: scene.width,
            height: scene.height,
            textures: scene
                .textures
                .into_iter()
                .map(|texture| RuntimeLiveResourceTexture {
                    texture_id: texture.texture_id,
                    resource_uri: texture.resource_uri.to_string(),
                    codec: texture.codec.to_string(),
                    revision: texture.revision,
                    decoded_width: texture.decoded_width,
                    decoded_height: texture.decoded_height,
                    decoded_format: match texture.decoded_format {
                        FfiRuntimeTextureFormat::Rgba8 => RuntimeLiveTextureFormat::Rgba8,
                        FfiRuntimeTextureFormat::LumaAlpha8 => RuntimeLiveTextureFormat::LumaAlpha8,
                    },
                })
                .collect(),
            draws: scene.draws.into_iter().map(runtime_live_draw).collect(),
        })
    }));
    effects.extend(value.audio.into_iter().map(|packet| {
        RuntimeLiveEffect::Audio(RuntimeLiveAudioPacket {
            sequence: packet.sequence,
            stream_id: packet.stream_id,
            sample_rate: packet.sample_rate,
            channels: packet.channels,
            pcm: match packet.pcm {
                FfiRuntimePcmBuffer::I16(samples) => RuntimeLivePcmBuffer::I16(samples.into_vec()),
                FfiRuntimePcmBuffer::F32(samples) => RuntimeLivePcmBuffer::F32(samples.into_vec()),
            },
        })
    }));
    for command in value.audio_commands {
        effects.push(RuntimeLiveEffect::AudioCommand(runtime_live_audio_command(
            command,
        )?));
    }
    effects.extend(value.audio_cues.into_iter().map(|cue| {
        RuntimeLiveEffect::AudioCue(RuntimeLiveAudioCue {
            sequence: cue.sequence,
            command_id: cue.command_id.to_string(),
            bus: match cue.bus {
                FfiRuntimeAudioBus::Voice => RuntimeLiveAudioBus::Voice,
                FfiRuntimeAudioBus::Bgm => RuntimeLiveAudioBus::Bgm,
                FfiRuntimeAudioBus::Se => RuntimeLiveAudioBus::Se,
                FfiRuntimeAudioBus::Movie => RuntimeLiveAudioBus::Movie,
            },
            asset: cue.asset.to_string(),
            looped: cue.looped,
            fade_ms: cue.fade_ms,
            sync: match cue.sync_kind {
                FfiRuntimeAudioSyncKind::None => RuntimeLiveAudioSync::None,
                FfiRuntimeAudioSyncKind::Text => RuntimeLiveAudioSync::Text,
                FfiRuntimeAudioSyncKind::Fence => {
                    RuntimeLiveAudioSync::Fence(cue.sync_fence.to_string())
                }
            },
        })
    }));
    effects.extend(value.text.into_iter().map(|text| {
        RuntimeLiveEffect::Text(RuntimeLiveTextLease {
            sequence: text.sequence,
            lease_id: text.lease_id.to_string(),
            byte_len: text.byte_len,
            source_ref: text.source_ref.to_string(),
        })
    }));
    effects.extend(value.text_presentations.into_iter().map(|text| {
        RuntimeLiveEffect::TextPresentation(RuntimeLiveTextPresentation {
            sequence: text.sequence,
            lease_id: text.lease_id.to_string(),
            layout_id: text.layout_id.to_string(),
            language: text.language.to_string(),
            font_families: text
                .font_families
                .into_iter()
                .map(|family| family.to_string())
                .collect(),
            body: RuntimeLiveTextRegion {
                x: text.body.x,
                y: text.body.y,
                width: text.body.width,
                height: text.body.height,
                font_size: text.body.font_size,
                line_height: text.body.line_height,
                max_lines: text.body.max_lines,
            },
            speaker: text
                .speaker
                .into_option()
                .map(|speaker| RuntimeLiveTextRegion {
                    x: speaker.x,
                    y: speaker.y,
                    width: speaker.width,
                    height: speaker.height,
                    font_size: speaker.font_size,
                    line_height: speaker.line_height,
                    max_lines: speaker.max_lines,
                }),
            rgba: text.rgba,
        })
    }));
    effects.extend(value.video.into_iter().map(|video| {
        RuntimeLiveEffect::Video(RuntimeLiveVideoCommand {
            sequence: video.sequence,
            command: match video.command {
                astra_plugin_abi::FfiRuntimeVideoCommandKind::Play => {
                    RuntimeLiveVideoCommandKind::Play {
                        playback_id: video.playback_id.to_string(),
                        resource_uri: video.resource_uri.to_string(),
                        mode: match video.mode {
                            FfiRuntimeVideoMode::ModalWithAudio => {
                                RuntimeLiveVideoMode::ModalWithAudio
                            }
                            FfiRuntimeVideoMode::LayerNoAudio => RuntimeLiveVideoMode::LayerNoAudio,
                        },
                        stage_width: video.stage_width,
                        stage_height: video.stage_height,
                    }
                }
                astra_plugin_abi::FfiRuntimeVideoCommandKind::Stop => {
                    RuntimeLiveVideoCommandKind::Stop {
                        playback_id: video.playback_id.to_string(),
                    }
                }
            },
        })
    }));
    effects.extend(value.waits.into_iter().map(|wait| {
        RuntimeLiveEffect::Wait(RuntimeLiveWait {
            sequence: wait.sequence,
            token_id: wait.token_id.to_string(),
            kind: match wait.kind {
                FfiRuntimeWaitKind::Frame => RuntimeLiveWaitKind::Frame {
                    frames: wait.number,
                },
                FfiRuntimeWaitKind::Time => RuntimeLiveWaitKind::Time {
                    milliseconds: wait.number,
                },
                FfiRuntimeWaitKind::Input => RuntimeLiveWaitKind::Input {
                    keys: wait.keys.into_iter().map(|key| key.to_string()).collect(),
                },
                FfiRuntimeWaitKind::MediaFence => RuntimeLiveWaitKind::MediaFence {
                    media_id: wait.name.to_string(),
                },
                FfiRuntimeWaitKind::PresentationFence => RuntimeLiveWaitKind::PresentationFence {
                    fence_id: wait.name.to_string(),
                },
                FfiRuntimeWaitKind::ProviderCompletion => RuntimeLiveWaitKind::ProviderCompletion {
                    request_id: wait.name.to_string(),
                },
                FfiRuntimeWaitKind::FamilyOpaque => RuntimeLiveWaitKind::FamilyOpaque {
                    wait_kind: wait.name.to_string(),
                    payload_len: wait.payload_len,
                },
            },
        })
    }));
    effects.extend(value.events.into_iter().map(|event| {
        RuntimeLiveEffect::Event(RuntimeLiveEvent {
            sequence: event.sequence,
            event: event.event.to_string(),
            payload: event.payload.into_vec(),
            due_tick: event.due_tick.into_option(),
        })
    }));
    effects.extend(value.blackboard.into_iter().map(|mutation| {
        RuntimeLiveEffect::Control(RuntimeLiveControl::SetBlackboard {
            sequence: mutation.sequence,
            key: mutation.key.to_string(),
            value: mutation.value.into_vec(),
        })
    }));
    effects.extend(value.dirty_sections.into_iter().map(|dirty| {
        RuntimeLiveEffect::Control(RuntimeLiveControl::SnapshotDirty {
            sequence: dirty.sequence,
            section_id: dirty.section_id.to_string(),
        })
    }));
    effects.sort_by_key(RuntimeLiveEffect::sequence);
    Ok(RuntimeLiveOutput {
        effects,
        state_revision: value.state_revision,
        coverage: RuntimeLiveCoverage {
            instructions: value.instructions,
            syscalls: value.syscalls,
            presentation_commands: value.presentation_commands,
            audio_commands: value.audio_command_count,
            text_events: value.text_events,
            capture_bytes: value.capture_bytes,
            operation_bytes: value.operation_bytes,
            pcm_moved_bytes: value.pcm_moved_bytes,
            pcm_copied_bytes: value.pcm_copied_bytes,
        },
        diagnostics: Vec::new(),
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_step_result(result: FfiRuntimeStepResult) -> Result<RuntimeStepOutput, String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    let persisted = result
        .persisted
        .into_iter()
        .map(|value: FfiRuntimePersistedOutput| {
            if !matches!(value.codec, FfiRuntimeSectionCodec::Postcard) {
                return Err("ASTRA_RUNTIME_PROVIDER_PERSISTED_CODEC".into());
            }
            let domain = match value.domain {
                0 => RuntimeOutputDomain::Effect,
                1 => RuntimeOutputDomain::Presentation,
                2 => RuntimeOutputDomain::Audio,
                3 => RuntimeOutputDomain::Await,
                4 => RuntimeOutputDomain::Observation,
                5 => RuntimeOutputDomain::Trace,
                6 => RuntimeOutputDomain::DirtySaveSection,
                _ => return Err("ASTRA_RUNTIME_PROVIDER_OUTPUT_DOMAIN".into()),
            };
            Ok(RuntimePersistedOutput::postcard_bytes(
                domain,
                value.schema.to_string(),
                SchemaVersion::new(
                    value.version_major,
                    value.version_minor,
                    value.version_patch,
                ),
                value.bytes.into_vec().into(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RuntimeStepOutput {
        session_id: GameRuntimeSessionId(result.session_id.to_string()),
        status: result.status.to_string(),
        live: runtime_live_output(result.live)?,
        persisted,
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_save_result(result: FfiRuntimeSaveResult) -> Result<RuntimeSaveSections, String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    Ok(RuntimeSaveSections {
        session_id: GameRuntimeSessionId(result.session_id.to_string()),
        sections: result
            .sections
            .into_iter()
            .map(|section| RuntimeSectionPayload {
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
                bytes: section.bytes.into_vec(),
            })
            .collect(),
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_restore_result(
    result: astra_plugin_abi::FfiRuntimeRestoreResult,
) -> Result<RuntimeRestoreReport, String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    Ok(RuntimeRestoreReport {
        session_id: GameRuntimeSessionId(result.session_id.to_string()),
        restored_fixed_step: result.restored_fixed_step,
        session_seed: result.session_seed,
        status: result.status.to_string(),
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    })
}

#[cfg(feature = "dynamic-abi")]
fn runtime_shutdown_result(
    result: astra_plugin_abi::FfiRuntimeShutdownResult,
) -> Result<RuntimeShutdownReport, String> {
    diagnostics(result.ok, result.diagnostics.as_slice())?;
    Ok(RuntimeShutdownReport {
        session_id: GameRuntimeSessionId(result.session_id.to_string()),
        status: result.status.to_string(),
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    })
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
