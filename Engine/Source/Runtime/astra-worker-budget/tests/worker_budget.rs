use std::{
    sync::{mpsc, Arc, Barrier},
    time::{Duration, Instant},
};

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

#[astra_headless_test::test]
fn worker_budget_serves_queued_workers_in_fifo_order() {
    let broker = Arc::new(WorkerBudgetBroker::new(1).unwrap());
    let held = broker.blocking_acquire().unwrap();
    let (completed_tx, completed_rx) = mpsc::channel();
    let mut handles = Vec::new();

    for index in 0..4 {
        let worker_broker = Arc::clone(&broker);
        let completed_tx = completed_tx.clone();
        handles.push(std::thread::spawn(move || {
            let _lease = worker_broker.blocking_acquire().unwrap();
            completed_tx.send(index).unwrap();
        }));
        let deadline = Instant::now() + Duration::from_secs(2);
        while broker.queued() != index + 1 {
            assert!(
                Instant::now() < deadline,
                "worker did not enter the FIFO queue before the bounded deadline"
            );
            std::thread::yield_now();
        }
    }

    drop(held);
    let completed = (0..4)
        .map(|_| completed_rx.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(completed, vec![0, 1, 2, 3]);
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(broker.queued(), 0);
    assert_eq!(broker.acquired(), 0);
}
