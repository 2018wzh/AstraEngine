use std::{
    collections::BTreeMap,
    fmt,
    future::Future,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};

use crate::{AwaitResult, AwaitTokenId, EventPayload};
use tokio_util::sync::CancellationToken;

/// Host-owned cancellation scope. It deliberately has no serialized identity.
#[derive(Clone)]
pub struct TaskScope(Arc<ScopeState>);

struct ScopeState {
    cancellation: CancellationToken,
    parent: Option<TaskScope>,
}

impl TaskScope {
    /// Create a standalone host scope. The owner must cancel it on shutdown.
    pub fn new() -> Self {
        Self(Arc::new(ScopeState {
            cancellation: CancellationToken::new(),
            parent: None,
        }))
    }

    pub fn child(&self) -> Self {
        Self(Arc::new(ScopeState {
            cancellation: self.0.cancellation.child_token(),
            parent: Some(self.clone()),
        }))
    }

    pub fn cancel(&self) {
        self.0.cancellation.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancellation.is_cancelled()
    }

    /// Wait for this scope or an ancestor to be cancelled, without polling a flag.
    pub async fn cancelled(&self) {
        self.0.cancellation.cancelled().await;
    }

    /// Run cancel-safe work without spawning it. Cancellation drops the future.
    /// Use ordinary async blocks and futures combinators for sequence and parallel work.
    /// Detached workers and external resources still require explicit owner cleanup.
    pub async fn run<T, E>(&self, work: impl Future<Output = Result<T, E>>) -> TaskOutcome<T, E> {
        let result = self.0.cancellation.run_until_cancelled(work).await;
        if self.is_cancelled() {
            return TaskOutcome::Cancelled;
        }
        match result {
            Some(Ok(value)) => TaskOutcome::Completed(value),
            Some(Err(error)) => TaskOutcome::Failed(error),
            None => TaskOutcome::Cancelled,
        }
    }

    fn belongs_to(&self, root: &Self) -> bool {
        let mut current = Some(self);
        while let Some(scope) = current {
            if scope == root {
                return true;
            }
            current = scope.0.parent.as_ref();
        }
        false
    }
}

impl Default for TaskScope {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for TaskScope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for TaskScope {}
impl fmt::Debug for TaskScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskScope")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// Process-local task result. Only explicit business state belongs in a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome<T, E> {
    Completed(T),
    Failed(E),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Completed,
    Cancelled,
}

/// Capture this handle before starting work, never after receiving its result.
#[derive(Clone)]
pub struct AwaitCompletionHandle {
    token_id: AwaitTokenId,
    scope: TaskScope,
    status: Arc<AtomicU8>,
}

impl AwaitCompletionHandle {
    pub fn token_id(&self) -> AwaitTokenId {
        self.token_id
    }

    pub fn status(&self) -> TaskStatus {
        match self.status.load(Ordering::Acquire) {
            1 => TaskStatus::Completed,
            2 => TaskStatus::Cancelled,
            _ if self.scope.is_cancelled() => TaskStatus::Cancelled,
            _ => TaskStatus::Pending,
        }
    }

    pub fn complete(&self, sequence: u64, step: u64, payload: EventPayload) -> AwaitCompletion {
        AwaitCompletion {
            handle: self.clone(),
            result: AwaitResult {
                token_id: self.token_id,
                sequence,
                completed_at_step: step,
                payload,
            },
        }
    }
}

impl PartialEq for AwaitCompletionHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.status, &other.status)
    }
}
impl fmt::Debug for AwaitCompletionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AwaitCompletionHandle")
            .field("token_id", &self.token_id)
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AwaitCompletion {
    pub(crate) handle: AwaitCompletionHandle,
    pub(crate) result: AwaitResult,
}

#[derive(Default)]
pub(crate) struct TaskRuntime {
    root: TaskScope,
    pending: BTreeMap<AwaitTokenId, AwaitCompletionHandle>,
}

impl Drop for TaskRuntime {
    fn drop(&mut self) {
        self.root.cancel();
    }
}

impl TaskRuntime {
    pub(crate) fn scope(&self) -> TaskScope {
        self.root.clone()
    }

    pub(crate) fn handle(
        &mut self,
        token: AwaitTokenId,
        scope: &TaskScope,
    ) -> Result<AwaitCompletionHandle, crate::RuntimeError> {
        if !scope.belongs_to(&self.root) || scope.is_cancelled() {
            return Err(crate::RuntimeError::message("ASTRA_TASK_SCOPE_INVALID: scope is cancelled or belongs to another World generation"));
        }
        if let Some(handle) = self.pending.get(&token) {
            if handle.scope != *scope {
                return Err(crate::RuntimeError::message(
                    "ASTRA_TASK_SCOPE_CONFLICT: await token already belongs to a different scope",
                ));
            }
            return Ok(handle.clone());
        }
        let handle = AwaitCompletionHandle {
            token_id: token,
            scope: scope.clone(),
            status: Arc::new(AtomicU8::new(0)),
        };
        self.pending.insert(token, handle.clone());
        Ok(handle)
    }

    pub(crate) fn accepts(&self, completion: &AwaitCompletion) -> bool {
        completion.handle.status() == TaskStatus::Pending
            && self.pending.get(&completion.result.token_id) == Some(&completion.handle)
    }

    pub(crate) fn finish(&mut self, token: AwaitTokenId, cancelled: bool) {
        if let Some(handle) = self.pending.remove(&token) {
            handle
                .status
                .store(if cancelled { 2 } else { 1 }, Ordering::Release);
        }
    }

    pub(crate) fn cancelled(&self) -> Vec<AwaitTokenId> {
        self.pending
            .iter()
            .filter_map(|(id, handle)| (handle.status() == TaskStatus::Cancelled).then_some(*id))
            .collect()
    }
}
