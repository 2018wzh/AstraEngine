use astra_core::SchemaVersion;
use astra_package::{
    AstraContainerBuilder, ContainerKind, MigrationPolicy, SectionCodec, SectionPayload,
};
use astra_runtime::{
    EventPayload, MigrationManifest, MigrationManifestEntry, OrderedTickIngress, PlayerInput,
    ReplayHashCheckpoint, ReplayTick, RuntimeConfig, RuntimeReplayTranscript, RuntimeWorld,
    SaveBlob, SaveRequest, TickIngress, TickInput, TickRequest,
};

#[test]
fn save_load_rejects_previous_runtime_world_layout_without_compatibility() {
    let world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
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
            postcard::to_allocvec(&world.snapshot().unwrap()).unwrap(),
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
    let mut loaded = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let error = loaded.load(SaveBlob(blob.into_bytes())).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED"));
}

#[test]
fn save_load_preserves_typed_world_and_stable_id_sequence() {
    let config = RuntimeConfig {
        seed: 23,
        required_slots: vec![],
    };
    let mut uninterrupted = RuntimeWorld::create(config.clone()).unwrap();
    uninterrupted.create_actor("before-save", vec![]).unwrap();
    uninterrupted
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: config.seed,
            },
            Vec::new(),
        ))
        .unwrap();
    let before = uninterrupted.state_hash();
    let save = uninterrupted.save(SaveRequest::default()).unwrap();
    let expected = uninterrupted.create_actor("after-save", vec![]).unwrap();

    let mut restored = RuntimeWorld::create(config).unwrap();
    restored.load(save).unwrap();
    assert_eq!(restored.state_hash(), before);
    assert_eq!(
        restored.create_actor("after-save", vec![]).unwrap(),
        expected
    );
}

#[test]
fn restored_world_requires_exactly_one_restore_continuation_tick() {
    let config = RuntimeConfig {
        seed: 17,
        required_slots: vec![],
    };
    let world = RuntimeWorld::create(config.clone()).unwrap();
    let save = world.save(SaveRequest::default()).unwrap();
    let mut restored = RuntimeWorld::create(config).unwrap();
    restored.load(save).unwrap();
    assert!(restored
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .unwrap_err()
        .to_string()
        .contains("ASTRA_RUNTIME_TICK_MODE_INVALID"));
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
    assert!(restored
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .is_err());
}

#[test]
fn save_load_rejects_footer_hash_mismatch() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world.create_actor("corrupt", vec![]).unwrap();
    let mut save = world.save(SaveRequest::default()).unwrap();
    let payload_byte = save.0.len() / 2;
    save.0[payload_byte] ^= 1;
    let mut loaded = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    assert!(loaded.load(save).is_err());
}

#[test]
fn replay_consumes_typed_player_input_with_explicit_evidence_checkpoint() {
    let config = RuntimeConfig {
        seed: 31,
        required_slots: vec![],
    };
    let mut recorded = RuntimeWorld::create(config).unwrap();
    let checkpoint = recorded.snapshot().unwrap();
    let player_input = PlayerInput {
        kind: "player.advance".to_string(),
        payload: EventPayload::new("player.advance"),
    };
    let timing = TickInput {
        fixed_step: 1,
        delta_ns: 16_666_667,
        seed: 31,
    };
    let report = recorded
        .tick(TickRequest::live(
            timing,
            vec![OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::PlayerInput(player_input.clone()),
            }],
        ))
        .unwrap();
    let expected = ReplayHashCheckpoint {
        step: report.step,
        state_hash: recorded.state_hash(),
        event_hash: recorded.event_hash(),
        presentation_hash: recorded.presentation_hash(),
    };
    let transcript = RuntimeReplayTranscript {
        schema: "astra.runtime_replay_transcript.v3".to_string(),
        checkpoint,
        ticks: vec![ReplayTick {
            request: TickRequest::replay(
                timing,
                vec![OrderedTickIngress {
                    sequence: 1,
                    payload: TickIngress::PlayerInput(player_input),
                }],
            ),
            expected,
        }],
    };
    let mut replayed = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let replay_report = replayed.replay(transcript).unwrap();
    assert_eq!(replay_report.state_hash, expected.state_hash);
    assert_eq!(replay_report.event_hash, expected.event_hash);
    assert_eq!(replay_report.presentation_hash, expected.presentation_hash);
}
