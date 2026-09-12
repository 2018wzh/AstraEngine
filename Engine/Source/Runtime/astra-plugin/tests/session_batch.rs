use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

use astra_plugin::{run_session_batch, WorkerBudgetBroker};
use tokio::sync::Barrier;

#[tokio::test]
async fn session_batch_bounds_workers_and_sorts_reports() {
    let budget = WorkerBudgetBroker::new(2).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut jobs = BTreeMap::new();
    for session_id in ["session-c", "session-a", "session-d", "session-b"] {
        let barrier = Arc::clone(&barrier);
        jobs.insert(session_id.to_string(), move || async move {
            barrier.wait().await;
            Ok(session_id.to_string())
        });
    }

    let report = run_session_batch(budget, jobs).await;
    assert!(report.succeeded());
    assert_eq!(report.worker_limit, 2);
    assert_eq!(report.peak_workers, 2);
    assert_eq!(
        report
            .entries
            .iter()
            .map(|entry| entry.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["session-a", "session-b", "session-c", "session-d"]
    );
}

#[tokio::test]
async fn session_batch_collects_failures_without_cancelling_other_sessions() {
    let budget = WorkerBudgetBroker::new(2).unwrap();
    type Job = Box<
        dyn FnOnce() -> Pin<
                Box<dyn Future<Output = Result<(), astra_plugin::RuntimeHostError>> + Send>,
            > + Send,
    >;
    let mut jobs: BTreeMap<String, Job> = BTreeMap::new();
    jobs.insert(
        "failed".to_string(),
        Box::new(|| {
            Box::pin(async {
                Err(astra_plugin::RuntimeHostError::new(
                    "ASTRA_TEST_FAILURE",
                    "expected fixture failure",
                ))
            })
        }),
    );
    jobs.insert(
        "successful".to_string(),
        Box::new(|| Box::pin(async { Ok::<_, astra_plugin::RuntimeHostError>(()) })),
    );

    let report = run_session_batch(budget, jobs).await;
    assert!(!report.succeeded());
    assert!(report.entries[0].result.is_err());
    assert!(report.entries[1].result.is_ok());
}
