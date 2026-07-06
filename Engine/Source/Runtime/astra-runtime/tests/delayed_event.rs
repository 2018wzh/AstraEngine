use astra_runtime::{
    EventPayload, EventSource, PackageHandle, RuntimeConfig, RuntimeWorld, SaveRequest, TickInput,
};

#[test]
fn delayed_events_drain_in_due_tick_sequence_order() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    world.schedule_event(3, EventSource::Scenario, EventPayload::new("timer.first"));
    let canceled = world.schedule_event(3, EventSource::Scenario, EventPayload::new("timer.skip"));
    world.schedule_event(3, EventSource::Scenario, EventPayload::new("timer.second"));
    assert!(world.cancel_delayed_event(canceled));

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
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    world.schedule_event(4, EventSource::Scenario, EventPayload::new("timer.saved"));
    tick(&mut world, 1);
    let save = world.save(SaveRequest::default()).unwrap();

    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    loaded.load(save).unwrap();
    tick(&mut loaded, 2);
    tick(&mut loaded, 3);
    assert!(loaded.debug_session().event_trace().is_empty());
    tick(&mut loaded, 4);
    assert_eq!(
        loaded.debug_session().event_trace()[0].payload.kind,
        "timer.saved"
    );
}

fn tick(world: &mut RuntimeWorld, fixed_step: u64) {
    world
        .tick(TickInput {
            fixed_step,
            delta_ns: 16_666_667,
            seed: 0,
        })
        .unwrap();
}
