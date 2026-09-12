use astra_core::StableId;
use astra_runtime::{
    AwaitCompletionPolicy, AwaitKind, AwaitToken, AwaitTokenId, OrderedTickIngress, RuntimeConfig,
    RuntimeWorld, TickIngress, TickInput, TickRequest,
};

#[test]
fn await_token_orders_out_of_order_results() {
    let left = run_with_order([2, 1]);
    let right = run_with_order([1, 2]);
    assert_eq!(left, right);
}

fn run_with_order(order: [u64; 2]) -> Vec<astra_runtime::RuntimeEvent> {
    let mut world = RuntimeWorld::create(RuntimeConfig {
        seed: 13,
        required_slots: vec![],
    })
    .unwrap();
    let token_a = AwaitTokenId(StableId::deterministic_v7(1, 1, 13));
    let token_b = AwaitTokenId(StableId::deterministic_v7(1, 2, 13));
    world
        .insert_await_token(AwaitToken {
            token_id: token_a,
            kind: AwaitKind::Custom("scenario".to_string()),
            requested_at_step: 0,
            timeout_step: None,
            completion_policy: AwaitCompletionPolicy::HostResult,
        })
        .unwrap();
    world
        .insert_await_token(AwaitToken {
            token_id: token_b,
            kind: AwaitKind::Custom("scenario".to_string()),
            requested_at_step: 0,
            timeout_step: None,
            completion_policy: AwaitCompletionPolicy::HostResult,
        })
        .unwrap();
    let ingress = order
        .into_iter()
        .enumerate()
        .map(|(index, sequence)| OrderedTickIngress {
            sequence: index as u64 + 1,
            payload: TickIngress::AwaitCompletion(completion(
                &mut world,
                token_for(sequence, token_a, token_b),
                sequence,
                1,
                "done",
            )),
        })
        .collect();
    world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 13,
            },
            ingress,
        ))
        .unwrap();
    world.debug_session().event_trace()
}

fn token_for(value: u64, token_a: AwaitTokenId, token_b: AwaitTokenId) -> AwaitTokenId {
    if value == 1 {
        token_a
    } else {
        token_b
    }
}

#[test]
fn await_token_is_serializable() {
    let token = AwaitToken {
        token_id: AwaitTokenId(StableId::deterministic_v7(1, 1, 1)),
        kind: AwaitKind::Timer,
        requested_at_step: 1,
        timeout_step: Some(4),
        completion_policy: AwaitCompletionPolicy::TickTimeout,
    };
    let encoded = postcard::to_allocvec(&token).unwrap();
    token.validate().unwrap();
    assert_eq!(postcard::from_bytes::<AwaitToken>(&encoded).unwrap(), token);
}

#[test]
fn await_timeout_materializes_deterministic_result() {
    let mut world = RuntimeWorld::create(RuntimeConfig {
        seed: 13,
        ..RuntimeConfig::default()
    })
    .unwrap();
    let token_id = AwaitTokenId(StableId::deterministic_v7(2, 1, 13));
    world
        .insert_await_token(AwaitToken {
            token_id,
            kind: AwaitKind::PresentationFence,
            requested_at_step: 1,
            timeout_step: Some(3),
            completion_policy: AwaitCompletionPolicy::TickTimeout,
        })
        .unwrap();

    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 13,
            },
            Vec::new(),
        ))
        .unwrap();
    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 13,
            },
            Vec::new(),
        ))
        .unwrap();
    let report = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 3,
                delta_ns: 16_666_667,
                seed: 13,
            },
            Vec::new(),
        ))
        .unwrap();

    assert!(report
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "ASTRA_AWAIT_TIMEOUT_INVALID"));
    assert!(world.debug_session().event_trace().iter().any(|event| {
        event.source == astra_runtime::EventSource::AwaitResult
            && event.payload.kind == "await.timeout"
    }));
    assert!(world.snapshot().unwrap().awaits.pending().is_empty());
}

#[test]
fn unknown_and_duplicate_await_results_are_diagnostic_only() {
    let mut world = RuntimeWorld::create(RuntimeConfig {
        seed: 13,
        ..RuntimeConfig::default()
    })
    .unwrap();
    let token_id = AwaitTokenId(StableId::deterministic_v7(3, 1, 13));
    world
        .insert_await_token(AwaitToken {
            token_id,
            kind: AwaitKind::Custom("scenario".to_string()),
            requested_at_step: 0,
            timeout_step: None,
            completion_policy: AwaitCompletionPolicy::HostResult,
        })
        .unwrap();
    let ingress = vec![
        OrderedTickIngress {
            sequence: 1,
            payload: TickIngress::AwaitCompletion(completion(&mut world, token_id, 1, 1, "done")),
        },
        OrderedTickIngress {
            sequence: 2,
            payload: TickIngress::AwaitCompletion(completion(
                &mut world,
                token_id,
                2,
                1,
                "done-again",
            )),
        },
    ];

    let report = world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 13,
            },
            ingress,
        ))
        .unwrap();

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_AWAIT_RESULT_DUPLICATE"));
    let scope = world.task_scope();
    assert!(world
        .await_handle(AwaitTokenId(StableId::deterministic_v7(3, 2, 13)), &scope)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_AWAIT_TOKEN_MISSING"));

    let await_events: Vec<_> = world
        .debug_session()
        .event_trace()
        .into_iter()
        .filter(|event| event.source == astra_runtime::EventSource::AwaitResult)
        .collect();
    assert_eq!(await_events.len(), 1);
    assert_eq!(await_events[0].payload.kind, "await.completed");
}

#[test]
fn await_completion_policy_rejects_invalid_tokens_and_live_timeout_results() {
    let mut world = RuntimeWorld::create(RuntimeConfig {
        seed: 13,
        ..RuntimeConfig::default()
    })
    .unwrap();
    let invalid_recorded = AwaitTokenId(StableId::deterministic_v7(4, 1, 13));
    let error = world
        .insert_await_token(AwaitToken {
            token_id: invalid_recorded,
            kind: AwaitKind::Timer,
            requested_at_step: 0,
            timeout_step: Some(2),
            completion_policy: AwaitCompletionPolicy::HostResult,
        })
        .unwrap_err();
    assert!(error.to_string().contains("ASTRA_AWAIT_COMPLETION_POLICY"));

    let timeout_token = AwaitTokenId(StableId::deterministic_v7(4, 2, 13));
    world
        .insert_await_token(AwaitToken {
            token_id: timeout_token,
            kind: AwaitKind::Timer,
            requested_at_step: 0,
            timeout_step: Some(2),
            completion_policy: AwaitCompletionPolicy::TickTimeout,
        })
        .unwrap();
    let scope = world.task_scope();
    let error = world.await_handle(timeout_token, &scope).unwrap_err();
    assert!(error.to_string().contains("ASTRA_AWAIT_RESULT_POLICY"));
    assert!(!world.is_failed());
    assert!(world.debug_session().event_trace().is_empty());
}

fn completion(
    world: &mut RuntimeWorld,
    token: AwaitTokenId,
    sequence: u64,
    step: u64,
    value: &str,
) -> astra_runtime::AwaitCompletion {
    let scope = world.task_scope();
    let handle = world.await_handle(token, &scope).unwrap();
    let mut payload = astra_runtime::EventPayload::new("await.completed");
    payload.data.insert(
        "value".into(),
        astra_runtime::BlackboardValue::String(value.into()),
    );
    handle.complete(sequence, step, payload)
}
