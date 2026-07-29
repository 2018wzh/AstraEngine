use std::sync::{Arc, Barrier};

use astra_worker_budget::WorkerBudgetBroker;

#[astra_headless_test::test]
fn worker_budget_never_exceeds_the_configured_limit() {
    let broker = Arc::new(WorkerBudgetBroker::new(2).unwrap());
    let entered = Arc::new(Barrier::new(3));
    let release = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let broker = Arc::clone(&broker);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                let _lease = broker.blocking_acquire().unwrap();
                entered.wait();
                release.wait();
            })
        })
        .collect::<Vec<_>>();
    entered.wait();
    assert_eq!(broker.acquired(), 2);
    assert_eq!(broker.available(), 0);
    assert!(broker.try_acquire().unwrap().is_none());
    release.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(broker.acquired(), 0);
    assert_eq!(broker.available(), 2);
    assert_eq!(broker.peak_acquired(), 2);
}

#[astra_headless_test::test]
fn worker_budget_rejects_zero_and_over_global_limit() {
    assert!(WorkerBudgetBroker::new(0).is_err());
    assert!(WorkerBudgetBroker::new(9).is_err());
}

#[test]
fn nested_work_reuses_the_callers_scoped_token() {
    let broker = WorkerBudgetBroker::new(1).unwrap();
    broker
        .run_scoped(|| {
            assert_eq!(broker.acquired(), 1);
            let nested = broker.blocking_acquire().unwrap();
            assert_eq!(broker.acquired(), 1);
            drop(nested);
            assert_eq!(broker.acquired(), 1);
        })
        .unwrap();
    assert_eq!(broker.acquired(), 0);
}
