//! Deterministic reporting for bounded concurrent session batches.

use std::{
    collections::BTreeMap,
    future::Future,
    time::{Duration, Instant},
};

use tokio::task::JoinSet;

use crate::{RuntimeHostError, WorkerBudgetBroker};

#[derive(Debug)]
pub struct SessionBatchEntry<T> {
    pub session_id: String,
    pub queue_time: Duration,
    pub run_time: Duration,
    pub result: Result<T, RuntimeHostError>,
}

#[derive(Debug)]
pub struct SessionBatchReport<T> {
    pub wall_time: Duration,
    pub worker_limit: usize,
    pub peak_workers: usize,
    /// Entries are always ordered by the caller-provided stable session id.
    pub entries: Vec<SessionBatchEntry<T>>,
}

impl<T> SessionBatchReport<T> {
    pub fn succeeded(&self) -> bool {
        self.entries.iter().all(|entry| entry.result.is_ok())
    }
}

/// Runs independent sessions with one global worker budget.
///
/// Every submitted session is allowed to finish even after another session
/// fails.  The resulting report is sorted by session id, rather than task
/// completion order, so it is safe to use as a deterministic evidence input.
pub async fn run_session_batch<T, F, Fut>(
    budget: WorkerBudgetBroker,
    jobs: BTreeMap<String, F>,
) -> SessionBatchReport<T>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, RuntimeHostError>> + Send + 'static,
{
    let started = Instant::now();
    let mut running = JoinSet::new();
    for (session_id, job) in jobs {
        let session_budget = budget.clone();
        running.spawn(async move {
            let queued = Instant::now();
            let lease = match session_budget.acquire().await {
                Ok(lease) => lease,
                Err(error) => {
                    return (
                        session_id,
                        queued.elapsed(),
                        Duration::ZERO,
                        Err(RuntimeHostError::new(error.code(), error.to_string())),
                    )
                }
            };
            let run_started = Instant::now();
            let result = job().await;
            let run_time = run_started.elapsed();
            drop(lease);
            (
                session_id,
                run_started.duration_since(queued),
                run_time,
                result,
            )
        });
    }

    let mut entries = Vec::new();
    while let Some(joined) = running.join_next().await {
        match joined {
            Ok((session_id, queue_time, run_time, result)) => entries.push(SessionBatchEntry {
                session_id,
                queue_time,
                run_time,
                result,
            }),
            Err(error) => entries.push(SessionBatchEntry {
                session_id: format!("join-error-{}", entries.len()),
                queue_time: Duration::ZERO,
                run_time: Duration::ZERO,
                result: Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_SESSION_BATCH_WORKER",
                    format!("session batch worker failed: {error}"),
                )),
            }),
        }
    }
    entries.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    SessionBatchReport {
        wall_time: started.elapsed(),
        worker_limit: budget.limit(),
        peak_workers: budget.peak_acquired(),
        entries,
    }
}
