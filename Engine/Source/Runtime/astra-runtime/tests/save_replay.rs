use astra_core::{Hash256, SchemaMigrationRegistry, SchemaVersion, StableId};
use astra_runtime::{
    AwaitKind, AwaitReplayPolicy, AwaitToken, AwaitTokenId, EventId, EventPayload, EventSource,
    PackageHandle, PlayerInput, PresentationCommand, ProviderReplayOutput, ReplayHashCheckpoint,
    ReplayTick, RuntimeConfig, RuntimeEvent, RuntimeReplayTranscript, RuntimeWorld, SaveRequest,
    SerializedEffectEnvelope, TickInput,
};

#[test]
fn save_replay_loads_hash_and_blocks_missing_migrator() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 17,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.create_actor("save-test", vec![]);
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 17,
        })
        .unwrap();
    let before = world.state_hash();
    let save = world.save(SaveRequest::default()).unwrap();
    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    loaded.load(save).unwrap();
    assert_eq!(loaded.state_hash(), before);

    let save_needing_migration = world
        .save(SaveRequest {
            minimum_supported_version: SchemaVersion::new(0, 9, 0),
        })
        .unwrap();
    let mut blocked =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    assert!(blocked.load(save_needing_migration.clone()).is_err());

    let mut registry = SchemaMigrationRegistry::default();
    registry.register_identity(
        "runtime.world",
        SchemaVersion::new(0, 9, 0),
        SchemaVersion::new(1, 0, 0),
    );
    blocked
        .load_with_registry(save_needing_migration, &registry)
        .unwrap();
}

#[test]
fn save_load_rejects_footer_hash_mismatch() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 19,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.create_actor("corrupt", vec![]);
    let mut save = world.save(SaveRequest::default()).unwrap();
    let payload_byte = save.0.len() / 2;
    save.0[payload_byte] ^= 0x01;

    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    assert!(loaded.load(save).is_err());
}

#[test]
fn save_load_continues_the_stable_id_sequence() {
    let config = RuntimeConfig {
        seed: 23,
        required_slots: vec![],
    };
    let mut uninterrupted = RuntimeWorld::create(config.clone(), PackageHandle::default()).unwrap();
    uninterrupted.create_actor("before-save", vec![]);
    uninterrupted
        .tick(TickInput {
            fixed_step: 7,
            delta_ns: 16_666_667,
            seed: config.seed,
        })
        .unwrap();
    let save = uninterrupted.save(SaveRequest::default()).unwrap();
    let expected = uninterrupted.create_actor("after-save", vec![]);

    let mut restored = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    restored.load(save).unwrap();
    let actual = restored.create_actor("after-save", vec![]);

    assert_eq!(actual, expected);
    assert_eq!(restored.state_hash(), uninterrupted.state_hash());
}

#[test]
fn save_load_preserves_pending_events_trace_and_sequence() {
    let config = RuntimeConfig {
        seed: 29,
        required_slots: vec![],
    };
    let mut world = RuntimeWorld::create(config.clone(), PackageHandle::default()).unwrap();
    world.enqueue_event(RuntimeEvent {
        id: EventId(StableId::deterministic_v7(2, 77, config.seed)),
        source: EventSource::Runtime,
        step: 2,
        sequence: 77,
        payload: EventPayload::new("future.event"),
    });
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: config.seed,
        })
        .unwrap();
    let save = world.save(SaveRequest::default()).unwrap();

    let mut restored = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    restored.load(save).unwrap();
    assert_eq!(restored.event_hash(), world.event_hash());
    restored.enqueue_event(RuntimeEvent {
        id: EventId(StableId::deterministic_v7(2, 78, 29)),
        source: EventSource::Runtime,
        step: 2,
        sequence: 99,
        payload: EventPayload::new("second.event"),
    });

    restored
        .tick(TickInput {
            fixed_step: 2,
            delta_ns: 16_666_667,
            seed: 29,
        })
        .unwrap();
    let trace = restored.debug_session().event_trace();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].payload.kind, "future.event");
    assert_eq!(trace[0].sequence, 0);
    assert_eq!(trace[1].payload.kind, "second.event");
    assert_eq!(trace[1].sequence, 1);
}

#[test]
fn replay_consumes_checkpoint_and_ordered_player_input_transcript() {
    let config = RuntimeConfig {
        seed: 31,
        required_slots: vec![],
    };
    let mut recorded = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    let checkpoint = recorded.snapshot();
    let player_input = PlayerInput {
        kind: "player.advance".to_string(),
        payload: EventPayload::new("player.advance"),
    };
    recorded.apply_input(player_input.clone()).unwrap();
    let tick = TickInput {
        fixed_step: 1,
        delta_ns: 16_666_667,
        seed: 31,
    };
    let expected = recorded.tick(tick).unwrap();

    let transcript = RuntimeReplayTranscript {
        schema: "astra.runtime_replay_transcript.v1".to_string(),
        checkpoint,
        ticks: vec![ReplayTick {
            tick,
            player_inputs: vec![player_input],
            await_results: vec![],
            provider_outputs: vec![],
            expected: ReplayHashCheckpoint::from(&expected),
        }],
    };
    let mut replayed =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let report = replayed.replay(transcript).unwrap();

    assert_eq!(report.state_hash, expected.state_hash);
    assert_eq!(report.event_hash, expected.event_hash);
    assert_eq!(report.presentation_hash, expected.presentation_hash);
}

#[test]
fn replay_applies_hash_validated_provider_output_without_a_live_provider() {
    let config = RuntimeConfig {
        seed: 37,
        required_slots: vec![],
    };
    let mut recorded = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    let checkpoint = recorded.snapshot();
    let payload = postcard::to_allocvec(&"recorded-provider-output").unwrap();
    let output = ProviderReplayOutput {
        provider_id: "test.provider".to_string(),
        session_id: "session-1".to_string(),
        schema: "test.provider.output.v1".to_string(),
        payload_hash: Hash256::from_sha256(&payload),
        payload,
        events: vec![RuntimeEvent {
            id: EventId(StableId::deterministic_v7(1, 1, 37)),
            source: EventSource::Runtime,
            step: 1,
            sequence: 0,
            payload: EventPayload::new("provider.recorded"),
        }],
        presentation: vec![PresentationCommand::Marker {
            name: "provider-frame".to_string(),
        }],
        awaits: vec![AwaitToken {
            token_id: AwaitTokenId(StableId::deterministic_v7(1, 2, 37)),
            kind: AwaitKind::Custom("provider".to_string()),
            requested_at_step: 1,
            deterministic_timeout_step: None,
            replay_policy: AwaitReplayPolicy::RecordedResult,
        }],
        effects: vec![SerializedEffectEnvelope::postcard(
            "audio",
            "test.audio_command.v1",
            &"play",
        )
        .unwrap()],
    };
    recorded
        .apply_recorded_provider_output(1, output.clone())
        .unwrap();
    let tick = TickInput {
        fixed_step: 1,
        delta_ns: 16_666_667,
        seed: 37,
    };
    let expected = recorded.tick(tick).unwrap();

    let transcript = RuntimeReplayTranscript {
        schema: "astra.runtime_replay_transcript.v1".to_string(),
        checkpoint,
        ticks: vec![ReplayTick {
            tick,
            player_inputs: vec![],
            await_results: vec![],
            provider_outputs: vec![output],
            expected: ReplayHashCheckpoint::from(&expected),
        }],
    };
    let mut replayed =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let report = replayed.replay(transcript).unwrap();

    assert_eq!(report.state_hash, expected.state_hash);
    assert_eq!(replayed.snapshot().effects.len(), 1);
    assert_eq!(replayed.snapshot().awaits.pending().len(), 1);
}

#[test]
fn replay_blocks_provider_output_payload_hash_mismatch() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let output = ProviderReplayOutput {
        provider_id: "test.provider".to_string(),
        session_id: "session-1".to_string(),
        schema: "test.provider.output.v1".to_string(),
        payload_hash: Hash256::from_sha256(b"expected"),
        payload: b"tampered".to_vec(),
        events: vec![],
        presentation: vec![],
        awaits: vec![],
        effects: vec![],
    };

    let error = world.apply_recorded_provider_output(1, output).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_PROVIDER_OUTPUT_HASH"));
}
