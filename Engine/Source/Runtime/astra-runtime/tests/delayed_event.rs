use astra_runtime::{
    EventPayload, EventSource, RuntimeConfig, RuntimeWorld, SaveRequest, TickInput, TickRequest,
};

#[test]
fn delayed_events_drain_in_due_tick_sequence_order() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world
        .schedule_event(3, EventSource::Scenario, EventPayload::new("timer.first"))
        .unwrap();
    let canceled = world
        .schedule_event(3, EventSource::Scenario, EventPayload::new("timer.skip"))
        .unwrap();
    world
        .schedule_event(3, EventSource::Scenario, EventPayload::new("timer.second"))
        .unwrap();
    assert!(world.cancel_delayed_event(canceled).unwrap());

    tick(&mut world, 1);
    tick(&mut world, 2);
    assert!(world.debug_session().event_trace().is_empty());

    tick(&mut world, 3);
    let kinds: Vec<_> = world
        .debug_session()
        .event_trace()
        .into_iter()
        .map(|event| event.payload.kind)
        .collect();
    assert_eq!(kinds, vec!["timer.first", "timer.second"]);
}

#[test]
fn delayed_events_survive_save_load_before_due_tick() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world
        .schedule_event(4, EventSource::Scenario, EventPayload::new("timer.saved"))
        .unwrap();
    tick(&mut world, 1);
    let save = world.save(SaveRequest::default()).unwrap();

    let mut loaded = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    loaded.load(save).unwrap();
    loaded
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 0,
            },
            vec![],
        ))
        .unwrap();
    tick(&mut loaded, 3);
    assert!(loaded.debug_session().event_trace().is_empty());
    tick(&mut loaded, 4);
    assert_eq!(
        loaded.debug_session().event_trace()[0].payload.kind,
        "timer.saved"
    );
}

#[test]
fn restored_unsorted_timers_preserve_cancellation_and_equal_deadline_order() {
    use astra_core::StableId;
    use astra_runtime::{DelayedEventId, DelayedEventQueue, ScheduledEvent};
    let event = |id, due_tick, sequence, kind: &str| ScheduledEvent {
        id: DelayedEventId(StableId::deterministic_v7(9, id, 17)),
        due_tick,
        sequence,
        source: EventSource::Scenario,
        payload: EventPayload::new(kind),
    };
    let cancelled = event(1, 10, 1, "cancelled");
    let cancelled_id = cancelled.id;
    let encoded = postcard::to_allocvec(&(
        vec![event(0, 30, 0, "last"), cancelled, event(2, 10, 2, "first")],
        3_u64,
    ))
    .unwrap();
    let mut queue: DelayedEventQueue = postcard::from_bytes(&encoded).unwrap();
    assert!(queue.drain_due(9).is_empty());
    assert!(queue.cancel(cancelled_id));
    assert!(!queue.cancel(cancelled_id));
    queue.schedule(event(3, 10, 0, "second"));
    let due = queue.drain_due(10);
    assert_eq!(
        due.iter()
            .map(|event| (event.payload.kind.as_str(), event.sequence))
            .collect::<Vec<_>>(),
        vec![("first", 2), ("second", 3)],
    );
    assert!(queue.drain_due(29).is_empty());
    assert_eq!(queue.drain_due(30)[0].payload.kind, "last");
    assert!(queue.queued().is_empty());
}

fn tick(world: &mut RuntimeWorld, fixed_step: u64) {
    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step,
                delta_ns: 16_666_667,
                seed: 0,
            },
            Vec::new(),
        ))
        .unwrap();
}
