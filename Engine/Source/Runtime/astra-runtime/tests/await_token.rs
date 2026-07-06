use astra_core::StableId;
use astra_runtime::{
    AwaitKind, AwaitReplayPolicy, AwaitResult, AwaitToken, AwaitTokenId, PackageHandle,
    RuntimeConfig, RuntimeWorld, TickInput,
};

#[test]
fn await_token_orders_out_of_order_results() {
    let left = run_with_order([2, 1]);
    let right = run_with_order([1, 2]);
    assert_eq!(left.event_hash, right.event_hash);
    assert_eq!(left.state_hash, right.state_hash);
}

fn run_with_order(order: [u64; 2]) -> astra_runtime::TickReport {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 13,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let token_a = AwaitTokenId(StableId::deterministic_v7(1, 1, 13));
    let token_b = AwaitTokenId(StableId::deterministic_v7(1, 2, 13));
    world.submit_await_result(AwaitResult::custom(
        token_for(order[0], token_a, token_b),
        order[0],
        1,
        "done",
    ));
    world.submit_await_result(AwaitResult::custom(
        token_for(order[1], token_a, token_b),
        order[1],
        1,
        "done",
    ));
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 13,
        })
        .unwrap()
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
        deterministic_timeout_step: Some(4),
        replay_policy: AwaitReplayPolicy::RecordedResult,
    };
    let encoded = postcard::to_allocvec(&token).unwrap();
    assert!(!encoded.is_empty());
}
