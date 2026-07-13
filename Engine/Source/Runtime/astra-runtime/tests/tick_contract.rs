use astra_core::Hash256;
use astra_runtime::{
    EngineModuleSlot, EventPayload, ModuleBindingContext, OrderedTickIngress, PackageHandle,
    PlayerInput, ProviderReplayOutput, RuntimeConfig, RuntimeWorld, TickIngress, TickInput,
    TickMode, TickRequest, ValidatedModuleBinding,
};

fn input(step: u64, seed: u64) -> TickInput {
    TickInput {
        fixed_step: step,
        delta_ns: 16_666_667,
        seed,
    }
}

fn request(input: TickInput) -> TickRequest {
    TickRequest::live(input, Vec::new())
}

fn binding_context(package_id: &str) -> ModuleBindingContext {
    let package = PackageHandle::default();
    ModuleBindingContext {
        package_id: package_id.to_string(),
        target: package.target,
        profile: package.profile,
        engine_version: package.engine_version,
        rustc_fingerprint: package.rustc_fingerprint,
        feature_fingerprint: package.feature_fingerprint,
        abi_fingerprint: package.abi_fingerprint,
    }
}

#[test]
fn tick_rejects_duplicate_gap_regression_delta_and_seed_without_mutation() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 41,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.create_actor("system", vec![]);
    world.tick(request(input(1, 41))).unwrap();
    let checkpoint = postcard::to_allocvec(&world.snapshot()).unwrap();

    let cases = [
        input(1, 41),
        input(0, 41),
        input(3, 41),
        TickInput {
            delta_ns: 0,
            ..input(2, 41)
        },
        TickInput {
            delta_ns: 1_000_000_001,
            ..input(2, 41)
        },
        input(2, 42),
    ];
    for invalid in cases {
        assert!(world.tick(request(invalid)).is_err());
        assert_eq!(
            postcard::to_allocvec(&world.snapshot()).unwrap(),
            checkpoint
        );
    }

    assert_eq!(world.tick(request(input(2, 41))).unwrap().step, 2);
}

#[test]
fn missing_required_module_blocks_before_step_or_id_state_changes() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 7,
            required_slots: vec!["presentation".to_string()],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let checkpoint = postcard::to_allocvec(&world.snapshot()).unwrap();
    let error = world.tick(request(input(1, 7))).unwrap_err();
    assert!(error.to_string().contains("ASTRA_RUNTIME_MODULE_MISSING"));
    assert_eq!(
        postcard::to_allocvec(&world.snapshot()).unwrap(),
        checkpoint
    );
}

#[test]
fn module_mount_requires_matching_explicit_packaged_binding_and_unique_slot() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let slot = EngineModuleSlot("presentation".to_string());
    assert!(ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        binding_context("stage1.headless"),
        true,
        false,
    )
    .is_err());
    assert!(ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        binding_context("stage1.headless"),
        false,
        true,
    )
    .is_err());
    let wrong_package = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        binding_context("other.package"),
        true,
        true,
    )
    .unwrap();
    assert!(world.mount_module(slot.clone(), wrong_package).is_err());

    let binding = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        binding_context("stage1.headless"),
        true,
        true,
    )
    .unwrap();
    world.mount_module(slot.clone(), binding).unwrap();
    let duplicate = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.second",
        "presentation.headless",
        binding_context("stage1.headless"),
        true,
        true,
    )
    .unwrap();
    assert!(world.mount_module(slot, duplicate).is_err());
}

#[test]
fn tick_rejects_invalid_ingress_order_and_mode_without_mutation() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 41,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let checkpoint = postcard::to_allocvec(&world.snapshot()).unwrap();
    let player_input = || {
        TickIngress::PlayerInput(PlayerInput {
            kind: "advance".to_string(),
            payload: EventPayload::default(),
        })
    };

    for ingress in [
        vec![OrderedTickIngress {
            sequence: 0,
            payload: player_input(),
        }],
        vec![
            OrderedTickIngress {
                sequence: 1,
                payload: player_input(),
            },
            OrderedTickIngress {
                sequence: 1,
                payload: player_input(),
            },
        ],
        vec![
            OrderedTickIngress {
                sequence: 2,
                payload: player_input(),
            },
            OrderedTickIngress {
                sequence: 1,
                payload: player_input(),
            },
        ],
    ] {
        let error = world
            .tick(TickRequest::live(input(1, 41), ingress))
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("ASTRA_RUNTIME_TICK_INGRESS_ORDER_INVALID"));
        assert_eq!(
            postcard::to_allocvec(&world.snapshot()).unwrap(),
            checkpoint
        );
    }

    let recorded = ProviderReplayOutput {
        provider_id: "provider.fixture".to_string(),
        session_id: "session.fixture".to_string(),
        schema: "provider.output.v1".to_string(),
        payload_hash: Hash256::from_sha256(&[]),
        payload: vec![],
        events: vec![],
        presentation: vec![],
        awaits: vec![],
        effects: vec![],
    };
    let error = world
        .tick(TickRequest::live(
            input(1, 41),
            vec![OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::RecordedProviderOutput(recorded.clone()),
            }],
        ))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_LIVE_RECORDED_OUTPUT_FORBIDDEN"));
    assert_eq!(
        postcard::to_allocvec(&world.snapshot()).unwrap(),
        checkpoint
    );

    let error = world
        .tick(TickRequest {
            timing: input(1, 41),
            mode: TickMode::Replay,
            ingress: vec![OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::LiveProviderOutput(recorded),
            }],
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_TICK_MODE_INVALID"));
    assert_eq!(
        postcard::to_allocvec(&world.snapshot()).unwrap(),
        checkpoint
    );
}

#[test]
fn tick_rolls_back_all_prior_ingress_when_provider_output_is_invalid() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 41,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let checkpoint = postcard::to_allocvec(&world.snapshot()).unwrap();
    let error = world
        .tick(TickRequest::live(
            input(1, 41),
            vec![
                OrderedTickIngress {
                    sequence: 1,
                    payload: TickIngress::PlayerInput(PlayerInput {
                        kind: "advance".to_string(),
                        payload: EventPayload::default(),
                    }),
                },
                OrderedTickIngress {
                    sequence: 2,
                    payload: TickIngress::LiveProviderOutput(ProviderReplayOutput {
                        provider_id: "provider.fixture".to_string(),
                        session_id: "session.fixture".to_string(),
                        schema: "provider.output.v1".to_string(),
                        payload_hash: Hash256::from_sha256(b"different"),
                        payload: vec![],
                        events: vec![],
                        presentation: vec![],
                        awaits: vec![],
                        effects: vec![],
                    }),
                },
            ],
        ))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_PROVIDER_OUTPUT_HASH"));
    assert_eq!(
        postcard::to_allocvec(&world.snapshot()).unwrap(),
        checkpoint
    );
    assert!(world.debug_session().event_trace().is_empty());
}
