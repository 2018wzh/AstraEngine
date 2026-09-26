use astra_runtime::{AwaitCompletionHandle, TaskOutcome, TaskScope};
use astra_vn_core::VnError;
use astra_vn_policy::{PolicyIoFailure, PolicyIoHost, PolicyIoStatus, PolicyReadText};
use std::{
    sync::{mpsc, Arc},
    thread::JoinHandle,
};

pub(crate) struct Completion {
    pub id: u64,
    pub scope: TaskScope,
    pub status: PolicyIoStatus,
    pub handle: AwaitCompletionHandle,
}
pub(super) struct PolicyIoWorker {
    id: u64,
    handle: AwaitCompletionHandle,
    scope: TaskScope,
    result: mpsc::Receiver<PolicyIoStatus>,
    thread: Option<JoinHandle<()>>,
}
impl PolicyIoWorker {
    pub fn start(
        host: Arc<dyn PolicyIoHost>,
        request: PolicyReadText,
        scope: TaskScope,
        handle: AwaitCompletionHandle,
    ) -> Result<Self, VnError> {
        if cfg!(target_arch = "wasm32") {
            return Err(VnError::diagnostic(
                "ASTRA_POLICY_HOST_IO_UNSUPPORTED",
                "native policy IO workers are unavailable on Web",
            ));
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                VnError::diagnostic("ASTRA_POLICY_HOST_IO_WORKER", "cannot create IO executor")
            })?;
        let worker_scope = scope.clone();
        let id = request.id;
        let (send, result) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("astra-policy-io".into())
            .spawn(move || {
                let max_bytes = request.max_bytes;
                let outcome = runtime
                    .block_on(worker_scope.run(async move { host.read_text(request).await }));
                let status = match outcome {
                    TaskOutcome::Completed(text) if text.len() <= max_bytes => {
                        PolicyIoStatus::Ready(text)
                    }
                    TaskOutcome::Completed(_) => PolicyIoStatus::Failed(PolicyIoFailure::Limit),
                    TaskOutcome::Failed(error) => PolicyIoStatus::Failed(error),
                    TaskOutcome::Cancelled => PolicyIoStatus::Failed(PolicyIoFailure::Cancelled),
                };
                let _ = send.send(status);
            })
            .map_err(|_| {
                VnError::diagnostic("ASTRA_POLICY_HOST_IO_WORKER", "cannot start IO worker")
            })?;
        Ok(Self {
            id,
            handle,
            scope,
            result,
            thread: Some(thread),
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn try_complete(&mut self) -> Option<Completion> {
        let status = match self.result.try_recv() {
            Ok(value) => value,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => PolicyIoStatus::Failed(PolicyIoFailure::Host),
        };
        Some(Completion {
            id: self.id,
            scope: self.scope.clone(),
            handle: self.handle.clone(),
            status,
        })
    }
}
impl Drop for PolicyIoWorker {
    fn drop(&mut self) {
        self.scope.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
