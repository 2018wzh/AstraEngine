use astra_core::{Hash256, SchemaVersion, StableId};
use astra_package::{
    AstraContainerBuilder, ContainerKind, MigrationPolicy, SectionCodec, SectionPayload,
};
use astra_runtime::{
    AwaitKind, AwaitReplayPolicy, AwaitToken, AwaitTokenId, EventId, EventPayload, EventSource,
    MigrationManifest, MigrationManifestEntry, OrderedTickIngress, PackageHandle, PlayerInput,
    PresentationCommand, ProviderReplayOutput, ReplayHashCheckpoint, ReplayTick, RuntimeConfig,
    RuntimeEvent, RuntimeReplayTranscript, RuntimeWorld, SaveBlob, SaveRequest,
    SerializedEffectEnvelope, TickIngress, TickInput, TickRequest,
};

#[astra_headless_test::test]
fn save_load_rejects_previous_runtime_world_layout_without_hidden_compatibility() {
    let world = RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let version = SchemaVersion::new(1, 0, 0);
    let manifest = MigrationManifest {
        sections: vec![MigrationManifestEntry {
            schema: "runtime.world".to_string(),
            minimum_supported_version: version,
            current_version: version,
        }],
    };
    let blob = AstraContainerBuilder::new(ContainerKind::Save)
        .add_section(SectionPayload::new(
            "runtime.world",
            "runtime.world",
            version,
            SectionCodec::Postcard,
            postcard::to_allocvec(&world.snapshot()).unwrap(),
            MigrationPolicy::current(),
        ))
        .add_section(SectionPayload::new(
            "migration.manifest",
            "migration.manifest",
            version,
            SectionCodec::Postcard,
            postcard::to_allocvec(&manifest).unwrap(),
            MigrationPolicy::current(),
        ))
        .write()
        .unwrap();
    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let error = loaded.load(SaveBlob(blob.into_bytes())).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED"));
}

#[astra_headless_test::test]
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
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 17,
            },
            Vec::new(),
        ))
        .unwrap();
    let before = world.state_hash();
    let save = world.save(SaveRequest::default()).unwrap();
    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    loaded.load(save).unwrap();
    assert_eq!(loaded.state_hash(), before);

    let unsupported = world
        .save(SaveRequest {
            minimum_supported_version: SchemaVersion::new(0, 9, 0),
        })
        .unwrap_err();
    assert!(unsupported
        .to_string()
        .contains("ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED"));
}

#[astra_headless_test::test]
fn restored_world_requires_exactly_one_restore_continuation_tick() {
    let config = RuntimeConfig {
        seed: 17,
        required_slots: vec![],
    };
    let world = RuntimeWorld::create(config.clone(), PackageHandle::default()).unwrap();
    let save = world.save(SaveRequest::default()).unwrap();
    let mut restored = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    restored.load(save).unwrap();
    let checkpoint = postcard::to_allocvec(&restored.snapshot()).unwrap();

    let live_error = restored
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .unwrap_err();
    assert!(live_error
        .to_string()
        .contains("ASTRA_RUNTIME_TICK_MODE_INVALID"));
    assert_eq!(
        postcard::to_allocvec(&restored.snapshot()).unwrap(),
        checkpoint
    );

    restored
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .unwrap();
    let second_restore = restored
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .unwrap_err();
    assert!(second_restore
        .to_string()
        .contains("ASTRA_RUNTIME_TICK_MODE_INVALID"));
}

#[astra_headless_test::test]
fn replay_rejects_old_schema_and_live_output_without_partial_world_changes() {
    let config = RuntimeConfig {
        seed: 37,
        required_slots: vec![],
    };
    let mut world = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    world.create_actor("original", vec![]);
    let original = postcard::to_allocvec(&world.snapshot()).unwrap();
    let checkpoint = RuntimeWorld::create(
        RuntimeConfig {
            seed: 37,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap()
    .snapshot();
    let expected = ReplayHashCheckpoint {
        step: 1,
        state_hash: astra_core::Hash128::from_blake3(b"unused"),
        event_hash: astra_core::Hash128::from_blake3(b"unused"),
        presentation_hash: astra_core::Hash128::from_blake3(b"unused"),
    };

    let old_schema = world
        .replay(RuntimeReplayTranscript {
            schema: "astra.runtime_replay_transcript.v1".to_string(),
            checkpoint: checkpoint.clone(),
            ticks: vec![],
        })
        .unwrap_err();
    assert!(old_schema
        .to_string()
        .contains("ASTRA_RUNTIME_REPLAY_SCHEMA"));
    assert_eq!(postcard::to_allocvec(&world.snapshot()).unwrap(), original);

    let payload = vec![];
    let live_output = ProviderReplayOutput {
        provider_id: "provider.fixture".to_string(),
        session_id: "session.fixture".to_string(),
        schema: "provider.output.v1".to_string(),
        payload_hash: Hash256::from_sha256(&payload),
        payload,
        events: vec![],
        presentation: vec![],
        awaits: vec![],
        effects: vec![],
    };
    let error = world
        .replay(RuntimeReplayTranscript {
            schema: "astra.runtime_replay_transcript.v2".to_string(),
            checkpoint,
            ticks: vec![ReplayTick {
                request: TickRequest::replay(
                    TickInput {
                        fixed_step: 1,
                        delta_ns: 16_666_667,
                        seed: 37,
                    },
                    vec![OrderedTickIngress {
                        sequence: 1,
                        payload: TickIngress::LiveProviderOutput(live_output),
                    }],
                ),
                expected,
            }],
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_REPLAY_LIVE_OUTPUT_FORBIDDEN"));
    assert_eq!(postcard::to_allocvec(&world.snapshot()).unwrap(), original);
}

#[astra_headless_test::test]
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

#[astra_headless_test::test]
fn save_load_continues_the_stable_id_sequence() {
    let config = RuntimeConfig {
        seed: 23,
        required_slots: vec![],
    };
    let mut uninterrupted = RuntimeWorld::create(config.clone(), PackageHandle::default()).unwrap();
    uninterrupted.create_actor("before-save", vec![]);
    uninterrupted
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: config.seed,
            },
            Vec::new(),
        ))
        .unwrap();
    let save = uninterrupted.save(SaveRequest::default()).unwrap();
    let expected = uninterrupted.create_actor("after-save", vec![]);

    let mut restored = RuntimeWorld::create(config, PackageHandle::default()).unwrap();
    restored.load(save).unwrap();
    let actual = restored.create_actor("after-save", vec![]);

    assert_eq!(actual, expected);
    assert_eq!(restored.state_hash(), uninterrupted.state_hash());
}

#[astra_headless_test::test]
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
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: config.seed,
            },
            Vec::new(),
        ))
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
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 29,
            },
            vec![],
        ))
        .unwrap();
    let trace = restored.debug_session().event_trace();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].payload.kind, "future.event");
    assert_eq!(trace[0].sequence, 0);
    assert_eq!(trace[1].payload.kind, "second.event");
    assert_eq!(trace[1].sequence, 1);
}

#[astra_headless_test::test]
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
    let tick = TickInput {
        fixed_step: 1,
        delta_ns: 16_666_667,
        seed: 31,
    };
    let expected = recorded
        .tick(TickRequest::live(
            tick,
            vec![OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::PlayerInput(player_input.clone()),
            }],
        ))
        .unwrap();

    let transcript = RuntimeReplayTranscript {
        schema: "astra.runtime_replay_transcript.v2".to_string(),
        checkpoint,
        ticks: vec![ReplayTick {
            request: TickRequest::replay(
                tick,
                vec![OrderedTickIngress {
                    sequence: 1,
                    payload: TickIngress::PlayerInput(player_input),
                }],
            ),
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

#[astra_headless_test::test]
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
    let tick = TickInput {
        fixed_step: 1,
        delta_ns: 16_666_667,
        seed: 37,
    };
    let expected = recorded
        .tick(TickRequest::live(
            tick,
            vec![OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::LiveProviderOutput(output.clone()),
            }],
        ))
        .unwrap();

    let transcript = RuntimeReplayTranscript {
        schema: "astra.runtime_replay_transcript.v2".to_string(),
        checkpoint,
        ticks: vec![ReplayTick {
            request: TickRequest::replay(
                tick,
                vec![OrderedTickIngress {
                    sequence: 1,
                    payload: TickIngress::RecordedProviderOutput(output),
                }],
            ),
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

#[astra_headless_test::test]
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

    let checkpoint = world.snapshot();
    let expected = ReplayHashCheckpoint {
        step: 1,
        state_hash: world.state_hash(),
        event_hash: world.event_hash(),
        presentation_hash: world.presentation_hash(),
    };
    let error = world
        .replay(RuntimeReplayTranscript {
            schema: "astra.runtime_replay_transcript.v2".to_string(),
            checkpoint,
            ticks: vec![ReplayTick {
                request: TickRequest::replay(
                    TickInput {
                        fixed_step: 1,
                        delta_ns: 16_666_667,
                        seed: 0,
                    },
                    vec![OrderedTickIngress {
                        sequence: 1,
                        payload: TickIngress::RecordedProviderOutput(output),
                    }],
                ),
                expected,
            }],
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_PROVIDER_OUTPUT_HASH"));
}
