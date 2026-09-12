use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use astra_runtime::{
    RuntimeConfig, RuntimeError, RuntimeWorld, SaveRequest, TaskOutcome, TaskScope,
};
use tokio::sync::oneshot;

struct Dropped(Arc<AtomicBool>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn sequence_preserves_order_and_stops_on_typed_failure() {
    let scope = TaskScope::new();
    let mut order = Vec::new();
    let result = scope
        .run(async {
            order.push(1);
            let value = async { Ok::<_, &'static str>(7) }.await?;
            order.push(value);
            async { Err::<(), _>("second step failed") }.await?;
            order.push(99);
            Ok::<_, &'static str>(())
        })
        .await;
    assert_eq!(result, TaskOutcome::Failed("second step failed"));
    assert_eq!(order, [1, 7]);
    assert!(!scope.is_cancelled());
    assert_eq!(
        scope.run(async { Ok::<_, ()>(9) }).await,
        TaskOutcome::Completed(9)
    );
}

#[tokio::test]
async fn parallel_completion_keeps_typed_values() {
    let scope = TaskScope::new();
    let (tx, rx) = oneshot::channel();
    let result = scope
        .run(async {
            futures_util::try_join!(async { rx.await.map_err(|_| "channel closed") }, async {
                tx.send(42).map_err(|_| "receiver closed")?;
                Ok::<_, &str>("ready")
            },)
        })
        .await;
    assert_eq!(result, TaskOutcome::Completed((42, "ready")));
}

#[tokio::test]
async fn parallel_failure_drops_pending_work_without_cancelling_siblings() {
    let scope = TaskScope::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(dropped.clone());
    let (started, waiting) = oneshot::channel();
    let result = scope
        .run(async {
            futures_util::try_join!(
                async {
                    let _guard = guard;
                    started.send(()).unwrap();
                    pending::<Result<(), &str>>().await
                },
                async {
                    waiting.await.unwrap();
                    Err::<(), _>("parallel step failed")
                },
            )
        })
        .await;
    assert_eq!(result, TaskOutcome::Failed("parallel step failed"));
    assert!(dropped.load(Ordering::Acquire));
    assert!(!scope.is_cancelled());
    assert!(!scope.child().is_cancelled());
}

#[tokio::test]
async fn parent_cancellation_wakes_all_descendants_and_preserves_sibling() {
    let root = TaskScope::new();
    let parent = root.child();
    let child = parent.child();
    let sibling = root.child();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(dropped.clone());
    let (started, waiting) = oneshot::channel();
    let task = tokio::spawn(async move {
        child
            .run(async {
                let _guard = guard;
                started.send(()).unwrap();
                pending::<Result<(), ()>>().await
            })
            .await
    });
    let notified = parent.child();
    let notification = tokio::spawn(async move { notified.cancelled().await });
    waiting.await.unwrap();
    parent.cancel();
    assert_eq!(task.await.unwrap(), TaskOutcome::Cancelled);
    notification.await.unwrap();
    assert!(dropped.load(Ordering::Acquire));
    assert!(!root.is_cancelled());
    assert!(!sibling.is_cancelled());
    assert!(parent.child().is_cancelled());
    root.cancel();
    sibling.cancelled().await;
}

#[tokio::test]
async fn cancelled_work_is_not_started_and_a_cancelled_result_is_not_published() {
    let scope = TaskScope::new();
    scope.cancel();
    assert_eq!(
        scope
            .run(async {
                panic!("cancelled work must not be polled");
                #[allow(unreachable_code)]
                Ok::<(), ()>(())
            })
            .await,
        TaskOutcome::Cancelled,
    );
    let scope = TaskScope::new();
    assert_eq!(
        scope
            .run(async {
                scope.cancel();
                Ok::<_, ()>(42)
            })
            .await,
        TaskOutcome::Cancelled,
    );
}

#[tokio::test]
async fn dropping_a_run_releases_its_future_without_cancelling_the_scope() {
    let scope = TaskScope::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(dropped.clone());
    {
        let work = scope.run(async {
            let _guard = guard;
            pending::<Result<(), ()>>().await
        });
        tokio::pin!(work);
        assert!(futures_util::poll!(work).is_pending());
    }
    assert!(dropped.load(Ordering::Acquire));
    assert!(!scope.is_cancelled());
    assert_eq!(
        scope.run(async { Ok::<_, ()>(1) }).await,
        TaskOutcome::Completed(1)
    );
}

#[tokio::test]
async fn successful_restore_and_world_drop_wake_old_work_but_rejected_restore_does_not() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let save = world.save(SaveRequest::default()).unwrap();
    let old_scope = world.task_scope().child();
    let scope = old_scope.clone();
    let (started, waiting) = oneshot::channel();
    let old_work = tokio::spawn(async move {
        scope
            .run(async {
                started.send(()).unwrap();
                pending::<Result<(), ()>>().await
            })
            .await
    });
    waiting.await.unwrap();
    assert!(world
        .load_with_validation(
            save.clone(),
            &astra_core::SchemaMigrationRegistry::default(),
            |_| Err::<(), _>(RuntimeError::message("reject candidate")),
        )
        .is_err());
    assert!(!old_scope.is_cancelled());
    assert!(!old_work.is_finished());
    world.load(save).unwrap();
    assert_eq!(old_work.await.unwrap(), TaskOutcome::Cancelled);
    let resumed_scope = world.task_scope();
    assert!(!resumed_scope.is_cancelled());
    let exit = tokio::spawn(async move { resumed_scope.run(pending::<Result<(), ()>>()).await });
    drop(world);
    assert_eq!(exit.await.unwrap(), TaskOutcome::Cancelled);
}

#[tokio::test]
async fn cancelling_a_parallel_group_drops_every_pending_branch() {
    let scope = TaskScope::new();
    let first = Arc::new(AtomicBool::new(false));
    let second = Arc::new(AtomicBool::new(false));
    let first_guard = Dropped(first.clone());
    let second_guard = Dropped(second.clone());
    let group = scope.run(async {
        futures_util::try_join!(
            async {
                let _guard = first_guard;
                pending::<Result<(), ()>>().await
            },
            async {
                let _guard = second_guard;
                pending::<Result<(), ()>>().await
            },
        )
    });
    tokio::pin!(group);
    assert!(futures_util::poll!(&mut group).is_pending());
    assert!(!first.load(Ordering::Acquire));
    assert!(!second.load(Ordering::Acquire));
    scope.cancel();
    assert_eq!(group.await, TaskOutcome::Cancelled);
    assert!(first.load(Ordering::Acquire));
    assert!(second.load(Ordering::Acquire));
}
