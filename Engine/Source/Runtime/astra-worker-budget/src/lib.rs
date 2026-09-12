//! Process-wide, bounded and FIFO worker budget.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Condvar, Mutex, OnceLock,
};
use std::{cell::Cell, fmt};

thread_local! {
    static SCOPED_BROKER: Cell<Option<(*const BrokerInner, usize)>> = const { Cell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerBudgetError {
    code: &'static str,
    message: &'static str,
}

impl WorkerBudgetError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for WorkerBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WorkerBudgetError {}

struct BudgetState {
    available: usize,
    next_ticket: u64,
    serving_ticket: u64,
}

struct BrokerInner {
    limit: usize,
    state: Mutex<BudgetState>,
    ready: Condvar,
    acquired: AtomicUsize,
    peak_acquired: AtomicUsize,
}

static GLOBAL_BROKER: OnceLock<WorkerBudgetBroker> = OnceLock::new();

#[derive(Clone)]
pub struct WorkerBudgetBroker {
    inner: Arc<BrokerInner>,
}

impl WorkerBudgetBroker {
    pub const DEFAULT_LIMIT: usize = 8;

    pub fn new(limit: usize) -> Result<Self, WorkerBudgetError> {
        if !(1..=Self::DEFAULT_LIMIT).contains(&limit) {
            return Err(WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET",
                "worker budget must be within 1..=8",
            ));
        }
        Ok(Self {
            inner: Arc::new(BrokerInner {
                limit,
                state: Mutex::new(BudgetState {
                    available: limit,
                    next_ticket: 0,
                    serving_ticket: 0,
                }),
                ready: Condvar::new(),
                acquired: AtomicUsize::new(0),
                peak_acquired: AtomicUsize::new(0),
            }),
        })
    }

    pub fn global() -> &'static Self {
        GLOBAL_BROKER.get_or_init(|| {
            WorkerBudgetBroker::new(Self::DEFAULT_LIMIT)
                .expect("default process worker budget is within the validated range")
        })
    }

    pub fn global_with_limit(limit: usize) -> Result<&'static Self, WorkerBudgetError> {
        if let Some(global) = GLOBAL_BROKER.get() {
            if global.limit() != limit {
                return Err(WorkerBudgetError::new(
                    "ASTRA_RUNTIME_WORKER_BUDGET_IDENTITY",
                    "process worker budget was already configured with another limit",
                ));
            }
            return Ok(global);
        }
        let broker = WorkerBudgetBroker::new(limit)?;
        let _ = GLOBAL_BROKER.set(broker);
        let global = GLOBAL_BROKER
            .get()
            .expect("worker budget OnceLock contains the installed broker");
        if global.limit() != limit {
            return Err(WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_IDENTITY",
                "process worker budget raced with another configured limit",
            ));
        }
        Ok(global)
    }

    pub fn limit(&self) -> usize {
        self.inner.limit
    }

    pub fn available(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("worker budget state lock must not be poisoned")
            .available
    }

    pub fn peak_acquired(&self) -> usize {
        self.inner.peak_acquired.load(Ordering::Acquire)
    }

    pub fn acquired(&self) -> usize {
        self.inner.acquired.load(Ordering::Acquire)
    }

    pub fn queued(&self) -> usize {
        let state = self
            .inner
            .state
            .lock()
            .expect("worker budget state lock must not be poisoned");
        usize::try_from(state.next_ticket.saturating_sub(state.serving_ticket))
            .unwrap_or(usize::MAX)
    }

    pub async fn acquire(&self) -> Result<WorkerBudgetLease, WorkerBudgetError> {
        let broker = self.clone();
        tokio::task::spawn_blocking(move || broker.blocking_acquire())
            .await
            .map_err(|_| {
                WorkerBudgetError::new(
                    "ASTRA_RUNTIME_WORKER_BUDGET_TASK",
                    "worker budget acquisition task panicked",
                )
            })?
    }

    pub fn blocking_acquire(&self) -> Result<WorkerBudgetLease, WorkerBudgetError> {
        let broker = Arc::as_ptr(&self.inner);
        if SCOPED_BROKER.with(|scope| {
            scope
                .get()
                .is_some_and(|(active_broker, depth)| active_broker == broker && depth > 0)
        }) {
            return Ok(WorkerBudgetLease { inner: None });
        }
        self.blocking_acquire_unscoped()
    }

    fn blocking_acquire_unscoped(&self) -> Result<WorkerBudgetLease, WorkerBudgetError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_POISONED",
                "worker budget state lock was poisoned",
            )
        })?;
        let ticket = state.next_ticket;
        state.next_ticket = state.next_ticket.checked_add(1).ok_or_else(|| {
            WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_SEQUENCE",
                "worker budget ticket sequence overflowed",
            )
        })?;
        while ticket != state.serving_ticket || state.available == 0 {
            state = self.inner.ready.wait(state).map_err(|_| {
                WorkerBudgetError::new(
                    "ASTRA_RUNTIME_WORKER_BUDGET_POISONED",
                    "worker budget wait lock was poisoned",
                )
            })?;
        }
        state.available -= 1;
        state.serving_ticket = state.serving_ticket.checked_add(1).ok_or_else(|| {
            WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_SEQUENCE",
                "worker budget serving sequence overflowed",
            )
        })?;
        drop(state);
        self.record_acquire();
        self.inner.ready.notify_all();
        Ok(WorkerBudgetLease {
            inner: Some(Arc::clone(&self.inner)),
        })
    }

    /// Executes work while lending the current worker token to nested work on
    /// the same thread. Nested subsystems therefore run inline without
    /// acquiring a second global token or deadlocking a one-worker profile.
    pub fn run_scoped<T>(&self, work: impl FnOnce() -> T) -> Result<T, WorkerBudgetError> {
        let broker = Arc::as_ptr(&self.inner);
        if SCOPED_BROKER.with(|scope| {
            scope
                .get()
                .is_some_and(|(active_broker, depth)| active_broker == broker && depth > 0)
        }) {
            return Ok(work());
        }
        let lease = self.blocking_acquire_unscoped()?;
        let prior = SCOPED_BROKER.with(|scope| {
            let prior = scope.get();
            match prior {
                Some((active, depth)) if active == broker => {
                    scope.set(Some((active, depth + 1)));
                }
                None => scope.set(Some((broker, 1))),
                Some(_) => {
                    return Err(WorkerBudgetError::new(
                        "ASTRA_RUNTIME_WORKER_BUDGET_NESTED_BROKER",
                        "a worker cannot enter two distinct broker scopes",
                    ));
                }
            }
            Ok(prior)
        })?;
        let scope = WorkerBudgetScope { prior };
        let result = work();
        drop(scope);
        drop(lease);
        Ok(result)
    }

    pub fn try_acquire(&self) -> Result<Option<WorkerBudgetLease>, WorkerBudgetError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_POISONED",
                "worker budget state lock was poisoned",
            )
        })?;
        if state.available == 0 || state.next_ticket != state.serving_ticket {
            return Ok(None);
        }
        state.available -= 1;
        state.next_ticket = state.next_ticket.checked_add(1).ok_or_else(|| {
            WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_SEQUENCE",
                "worker budget ticket sequence overflowed",
            )
        })?;
        state.serving_ticket = state.serving_ticket.checked_add(1).ok_or_else(|| {
            WorkerBudgetError::new(
                "ASTRA_RUNTIME_WORKER_BUDGET_SEQUENCE",
                "worker budget serving sequence overflowed",
            )
        })?;
        drop(state);
        self.record_acquire();
        Ok(Some(WorkerBudgetLease {
            inner: Some(Arc::clone(&self.inner)),
        }))
    }

    fn record_acquire(&self) {
        let current = self.inner.acquired.fetch_add(1, Ordering::AcqRel) + 1;
        self.inner
            .peak_acquired
            .fetch_max(current, Ordering::AcqRel);
    }
}

struct WorkerBudgetScope {
    prior: Option<(*const BrokerInner, usize)>,
}

impl Drop for WorkerBudgetScope {
    fn drop(&mut self) {
        SCOPED_BROKER.with(|scope| scope.set(self.prior));
    }
}

pub struct WorkerBudgetLease {
    inner: Option<Arc<BrokerInner>>,
}

impl Drop for WorkerBudgetLease {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.acquired.fetch_sub(1, Ordering::AcqRel);
            let mut state = inner
                .state
                .lock()
                .expect("worker budget state lock must not be poisoned during release");
            state.available += 1;
            drop(state);
            inner.ready.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_observes_an_explicit_process_limit() {
        let configured = WorkerBudgetBroker::global_with_limit(3).unwrap();
        assert_eq!(configured.limit(), 3);
        assert_eq!(WorkerBudgetBroker::global().limit(), 3);
    }
}
