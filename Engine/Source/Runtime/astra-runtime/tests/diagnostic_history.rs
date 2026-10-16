use astra_core::StableId;
use astra_runtime::{
    EventId, EventPayload, EventQueue, EventSource, PresentationCommand, RuntimeConfig,
    RuntimeWorld, SaveRequest,
};

#[test]
fn consumed_event_history_is_bounded_and_does_not_enter_recovery_state() {
    let mut queue = EventQueue::default();
    for index in 0..4097_u64 {
        queue.push(astra_runtime::RuntimeEvent {
            id: EventId(StableId::deterministic_v7(7, index, 11)),
            source: EventSource::Runtime,
            step: index,
            sequence: 0,
            payload: EventPayload::new(format!("event.{index}")),
        });
        let ready = queue.drain_ordered_for_step(index);
        assert_eq!(ready[0].sequence, index);
    }
    assert!(queue.trace().len() <= 1024);
    assert_eq!(
        queue.trace().iter().last().unwrap().payload.kind,
        "event.4096"
    );
    queue.push(astra_runtime::RuntimeEvent {
        id: EventId(StableId::deterministic_v7(7, 4097, 11)),
        source: EventSource::AwaitResult,
        step: 5000,
        sequence: 0,
        payload: EventPayload::new("pending.result"),
    });
    let encoded = postcard::to_allocvec(&queue).unwrap();
    let mut restored: EventQueue = postcard::from_bytes(&encoded).unwrap();
    assert!(restored.trace().is_empty());
    assert!(restored.drain_ordered_for_step(4999).is_empty());
    let ready = restored.drain_ordered_for_step(5000);
    assert_eq!(ready[0].payload.kind, "pending.result");
    assert_eq!(ready[0].sequence, 4097);
}

#[test]
fn presentation_history_is_bounded_without_reusing_sequence_after_restore() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let before_len = world.save(SaveRequest::default()).unwrap().0.len();
    for index in 0..4097_u64 {
        world
            .emit_presentation(PresentationCommand::Marker {
                name: format!("marker.{index}"),
            })
            .unwrap();
    }
    let trace = world.debug_session().presentation_trace();
    assert!(trace.len() <= 1024);
    assert_eq!(trace.last().unwrap().sequence, 4096);
    let save = world.save(SaveRequest::default()).unwrap();
    assert!(
        save.0.len() <= before_len + 64,
        "diagnostic history grew the save"
    );
    let mut restored = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    restored.load(save).unwrap();
    assert!(restored.debug_session().presentation_trace().is_empty());
    restored
        .emit_presentation(PresentationCommand::Marker {
            name: "continued".into(),
        })
        .unwrap();
    assert_eq!(
        restored.debug_session().presentation_trace()[0].sequence,
        4097
    );
}
