use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
};

use crate::{AwaitResult, AwaitTokenId, EventPayload};

/// Host-owned cancellation scope. It deliberately has no serialized identity.
#[derive(Clone)]
pub struct TaskScope(Arc<ScopeState>);

struct ScopeState {
    cancelled: AtomicBool,
    parent: Option<TaskScope>,
}

impl TaskScope {
    fn root() -> Self {
        Self(Arc::new(ScopeState {
            cancelled: AtomicBool::new(false),
            parent: None,
        }))
    }

    pub fn child(&self) -> Self {
        Self(Arc::new(ScopeState {
            cancelled: AtomicBool::new(false),
            parent: Some(self.clone()),
        }))
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        let mut current = Some(self);
        while let Some(scope) = current {
            if scope.0.cancelled.load(Ordering::Acquire) {
                return true;
            }
            current = scope.0.parent.as_ref();
        }
        false
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

pub(crate) struct TaskRuntime {
    root: TaskScope,
    pending: BTreeMap<AwaitTokenId, AwaitCompletionHandle>,
}

impl Default for TaskRuntime {
    fn default() -> Self {
        Self {
            root: TaskScope::root(),
            pending: BTreeMap::new(),
        }
    }
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
