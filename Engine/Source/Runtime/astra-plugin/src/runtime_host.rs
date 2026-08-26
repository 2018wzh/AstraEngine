//! v1 同步宿主已删除，当前为 v2 工厂式宿主的同步兼容桥。
//! 新代码请直接使用 `concurrent_runtime_host::ConcurrentProductRuntimeHost`（`ProductRuntimeHostV2`）。

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(feature = "dynamic-abi")]
use astra_plugin_abi::FfiRuntimeProviderRegistration;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport,
    RuntimeProbeRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionPayload,
    RuntimeShutdownReport, RuntimeStepInput, RuntimeStepMode, RuntimeStepOutput,
    ValidatedRuntimeProviderSelection,
};

use crate::concurrent_runtime_host::{
    ConcurrentProductRuntimeHost, ProductRuntimeProviderFactory, ProductRuntimeSession,
};

#[derive(Debug, Clone)]
pub struct RuntimeHostLimits {
    max_outputs: usize,
    max_output_bytes: usize,
}

impl Default for RuntimeHostLimits {
    fn default() -> Self {
        Self {
            max_outputs: 256,
            max_output_bytes: 8 * 1024 * 1024,
        }
    }
}

impl RuntimeHostLimits {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_descriptor(_descriptor: &ProductRuntimeDescriptor) -> Self {
        Self::new()
    }
    pub fn with_bounds(mut self, max_outputs: usize, max_output_bytes: usize) -> Self {
        self.max_outputs = max_outputs;
        self.max_output_bytes = max_output_bytes;
        self
    }

    pub(crate) fn validate_output_bounds(
        &self,
        output: &astra_plugin_abi::RuntimeStepOutput,
    ) -> Result<(), RuntimeHostError> {
        let live_count = output.live.scenes.len()
            + output.live.resource_scenes.len()
            + output.live.audio.len()
            + output.live.audio_commands.len()
            + output.live.audio_cues.len()
            + output.live.text.len()
            + output.live.text_presentations.len()
            + output.live.presentations.len()
            + output.live.timeline.len()
            + output.live.video.len()
            + output.live.waits.len()
            + output.live.events.len()
            + output.live.blackboard.len()
            + output.live.dirty_sections.len();
        if live_count > self.max_outputs {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_OUTPUT_COUNT",
                "runtime provider live output count exceeds the configured bound",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_sections(
        &self,
        sections: &[astra_plugin_abi::RuntimeSectionPayload],
    ) -> Result<(), RuntimeHostError> {
        use std::collections::BTreeSet;
        if sections.len() > self.max_outputs {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SECTION_COUNT",
                "runtime save section count exceeds the configured bound",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut bytes = 0usize;
        for section in sections {
            if section.section_id.is_empty()
                || section.section_id.len() > 128
                || !section
                    .section_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                || section.schema.is_empty()
                || section.schema.len() > 128
                || !section
                    .schema
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHostError {
    code: &'static str,
    message: String,
}

impl RuntimeHostError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
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

// 保留旧 trait 以兼容存量 provider 实现（NativeVn/Emu 仍实现此 trait，内部通过适配器转为 Factory）
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

struct ProviderAsFactory<P: ProductRuntimeProvider> {
    inner: Arc<Mutex<P>>,
}

impl<P: ProductRuntimeProvider + 'static> ProductRuntimeProviderFactory for ProviderAsFactory<P> {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        self.inner.lock().unwrap().descriptor()
    }
    fn create_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        self.inner.lock().unwrap().create_instance(instance_id)
    }
    fn destroy_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        self.inner.lock().unwrap().destroy_instance(instance_id)
    }
    fn prepare(&self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        self.inner.lock().unwrap().prepare(request)
    }
    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        self.inner.lock().unwrap().probe(request)
    }
    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String> {
        let report = self.inner.lock().unwrap().open(request)?;
        let session_id = report.session_id.clone();
        let inner = Arc::clone(&self.inner);
        let session: Box<dyn ProductRuntimeSession> = Box::new(ProviderAsSession { inner, session_id });
        Ok((report, session))
    }
}

struct ProviderAsSession<P: ProductRuntimeProvider> {
    inner: Arc<Mutex<P>>,
    session_id: GameRuntimeSessionId,
}

impl<P: ProductRuntimeProvider + 'static> ProductRuntimeSession for ProviderAsSession<P> {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        self.inner.lock().unwrap().step(input)
    }
    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        self.inner.lock().unwrap().save(request)
    }
    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        self.inner.lock().unwrap().restore(request)
    }
    fn shutdown(
        self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        if session_id != self.session_id {
            return Err("ASTRA_RUNTIME_PROVIDER_SESSION_MISMATCH".to_string());
        }
        self.inner.lock().unwrap().shutdown(session_id)
    }
}

fn block_on<F: std::future::Future + Send + 'static>(future: F) -> F::Output
where
    F::Output: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    })
    .join()
    .unwrap()
}

impl ProductRuntimeHost {
    fn block_on_self<F: std::future::Future + Send + 'static>(&self, future: F) -> F::Output
    where
        F::Output: Send + 'static,
    {
        let rt = Arc::clone(&self.rt);
        std::thread::spawn(move || rt.block_on(future)).join().unwrap()
    }
}

pub struct ProductRuntimeHost {
    inner: ConcurrentProductRuntimeHost,
    open_sessions: Arc<Mutex<Vec<GameRuntimeSessionId>>>,
    rt: Arc<tokio::runtime::Runtime>,
}

impl ProductRuntimeHost {
    pub fn bound_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        provider: P,
        limits: RuntimeHostLimits,
    ) -> Result<Self, RuntimeHostError> {
        let factory = ProviderAsFactory {
            inner: Arc::new(Mutex::new(provider)),
        };
        let inner = ConcurrentProductRuntimeHost::bound_in_process(
            instance_id,
            selection,
            factory,
            limits,
            Duration::from_secs(10),
        )?;
        Ok(Self {
            inner,
            open_sessions: Arc::new(Mutex::new(Vec::new())),
            rt: Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()),
        })
    }

    pub fn reference_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        limits: RuntimeHostLimits,
    ) -> Result<Self, RuntimeHostError> {
        let factory = ProviderAsFactory {
            inner: Arc::new(Mutex::new(provider)),
        };
        let inner = ConcurrentProductRuntimeHost::new(
            instance_id,
            factory,
            limits,
            Duration::from_secs(10),
        )?;
        Ok(Self {
            inner,
            open_sessions: Arc::new(Mutex::new(Vec::new())),
            rt: Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()),
        })
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn bound_ffi(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        registration: FfiRuntimeProviderRegistration,
        limits: RuntimeHostLimits,
    ) -> Result<Self, RuntimeHostError> {
        let inner = ConcurrentProductRuntimeHost::bound_ffi(
            instance_id,
            selection,
            registration,
            limits,
            Duration::from_secs(10),
        )?;
        Ok(Self {
            inner,
            open_sessions: Arc::new(Mutex::new(Vec::new())),
            rt: Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()),
        })
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn reference_ffi(
        instance_id: impl Into<String>,
        registration: FfiRuntimeProviderRegistration,
        limits: RuntimeHostLimits,
    ) -> Result<Self, RuntimeHostError> {
        let inner = ConcurrentProductRuntimeHost::reference_ffi(
            instance_id,
            registration,
            limits,
            Duration::from_secs(10),
        )?;
        Ok(Self {
            inner,
            open_sessions: Arc::new(Mutex::new(Vec::new())),
            rt: Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()),
        })
    }

    pub fn prepare(
        &mut self,
        request: RuntimePrepareRequest,
    ) -> Result<RuntimePrepareReport, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.prepare(request).await })
    }

    pub fn probe(
        &mut self,
        request: RuntimeProbeRequest,
    ) -> Result<RuntimeProbeReport, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.probe(request).await })
    }

    pub fn open(
        &mut self,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, RuntimeHostError> {
        let inner = self.inner.clone();
        let session_store = Arc::clone(&self.open_sessions);
        let report = self.block_on_self(async move { inner.open(request).await })?;
        session_store.lock().unwrap().push(report.session_id.clone());
        Ok(report)
    }

    pub fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.step(input).await })
    }

    pub fn save(
        &mut self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.save(request).await })
    }

    pub fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.restore(request).await })
    }

    pub fn shutdown_session(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        let inner = self.inner.clone();
        let session_store = Arc::clone(&self.open_sessions);
        let sid = session_id.clone();
        let result = block_on(async move { inner.shutdown(session_id).await });
        if result.is_ok() {
            session_store.lock().unwrap().retain(|id| id != &sid);
        }
        result
    }

    pub fn shutdown(&mut self) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        let session_id = {
            let sessions = self.open_sessions.lock().unwrap();
            if sessions.len() != 1 {
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_LIFECYCLE",
                    "shutdown without a session id requires exactly one open session; use shutdown_session",
                ));
            }
            sessions[0].clone()
        };
        self.shutdown_session(session_id)
    }

    pub fn destroy(&mut self) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.destroy().await })
    }

    pub fn cleanup_after_failure(
        &mut self,
    ) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        let inner = self.inner.clone();
        self.block_on_self(async move { inner.destroy().await })
    }
}

// 保留 AsyncProductRuntimeHost 作为 v2 的别名，以兼容存量 async 调用方
pub struct AsyncProductRuntimeHost {
    inner: ConcurrentProductRuntimeHost,
}

impl AsyncProductRuntimeHost {
    pub fn bound_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        provider: P,
        limits: RuntimeHostLimits,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        let factory = ProviderAsFactory {
            inner: Arc::new(Mutex::new(provider)),
        };
        Ok(Self {
            inner: ConcurrentProductRuntimeHost::bound_in_process(
                instance_id,
                selection,
                factory,
                limits,
                timeout,
            )?,
        })
    }

    pub fn reference_in_process<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        limits: RuntimeHostLimits,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        let factory = ProviderAsFactory {
            inner: Arc::new(Mutex::new(provider)),
        };
        Ok(Self {
            inner: ConcurrentProductRuntimeHost::new(instance_id, factory, limits, timeout)?,
        })
    }

    pub fn reference_local_serialized<P: ProductRuntimeProvider + 'static>(
        instance_id: impl Into<String>,
        provider: P,
        limits: RuntimeHostLimits,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Self::reference_in_process(instance_id, provider, limits, timeout)
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn bound_ffi(
        instance_id: impl Into<String>,
        selection: &ValidatedRuntimeProviderSelection,
        registration: FfiRuntimeProviderRegistration,
        limits: RuntimeHostLimits,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Ok(Self {
            inner: ConcurrentProductRuntimeHost::bound_ffi(
                instance_id,
                selection,
                registration,
                limits,
                timeout,
            )?,
        })
    }

    #[cfg(feature = "dynamic-abi")]
    pub fn reference_ffi(
        instance_id: impl Into<String>,
        registration: FfiRuntimeProviderRegistration,
        limits: RuntimeHostLimits,
        timeout: Duration,
    ) -> Result<Self, RuntimeHostError> {
        Ok(Self {
            inner: ConcurrentProductRuntimeHost::reference_ffi(
                instance_id, registration, limits, timeout,
            )?,
        })
    }

    pub async fn prepare(
        &self,
        request: RuntimePrepareRequest,
    ) -> Result<RuntimePrepareReport, RuntimeHostError> {
        self.inner.prepare(request).await
    }
    pub async fn probe(
        &self,
        request: RuntimeProbeRequest,
    ) -> Result<RuntimeProbeReport, RuntimeHostError> {
        self.inner.probe(request).await
    }
    pub async fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, RuntimeHostError> {
        self.inner.open(request).await
    }
    pub async fn step(
        &self,
        input: RuntimeStepInput,
    ) -> Result<RuntimeStepOutput, RuntimeHostError> {
        self.inner.step(input).await
    }
    pub async fn save(
        &self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        self.inner.save(request).await
    }
    pub async fn restore(
        &self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        self.inner.restore(request).await
    }
    pub async fn shutdown(
        &self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        self.inner.shutdown(session_id).await
    }
    pub async fn destroy(&self) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        self.inner.destroy().await
    }
    pub async fn cleanup_after_failure(
        &self,
    ) -> Result<RuntimeProviderInstanceReport, RuntimeHostError> {
        self.inner.destroy().await
    }
}
