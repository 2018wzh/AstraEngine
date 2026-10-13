use super::*;

pub(super) struct Fixture {
    pub(super) _host: astra_headless_test::HeadlessTestContext,
    pub(super) _root: tempfile::TempDir,
    pub(super) profile: PathBuf,
    pub(super) package: PathBuf,
    pub(super) input: PathBuf,
    pub(super) checkpoint: PathBuf,
    pub(super) build_identity: PathBuf,
    pub(super) messages: Vec<InputMessage>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let host = astra_headless_test::HeadlessTestContext::start().unwrap();
        let root = tempfile::tempdir().unwrap();
        let package_bytes = astra_player_vn::headless_test_fixture::product_package(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
        );
        let package_hash = astra_core::Hash256::from_sha256(&package_bytes).to_string();
        let build_identity = astra_headless_test::headless_build_identity_path().unwrap();
        let identity: serde_json::Value =
            serde_json::from_slice(&fs::read(&build_identity).unwrap()).unwrap();
        let build_fingerprint = identity["identity_hash"].as_str().unwrap().to_owned();
        let mut profile = HeadlessHostProfile::reference(
            "cli-contract",
            "com.astra.fixture.cli",
            build_fingerprint,
            package_hash,
        );
        profile.artifacts.namespace = "cli-contract".into();
        profile.artifacts.required_checkpoints = vec!["final".into()];
        profile.viewport_width = 320;
        profile.viewport_height = 180;

        let session = "cli-equivalence";
        let messages = vec![
            input(session, 2, 0, PhysicalInput::Resume),
            input(session, 3, 0, PhysicalInput::Focus { focused: true }),
            input(
                session,
                4,
                1,
                PhysicalInput::Keyboard {
                    physical_key: "Enter".into(),
                    logical_key: Some("Enter".into()),
                    state: ButtonState::Pressed,
                    repeat: false,
                },
            ),
            input(
                session,
                5,
                2,
                PhysicalInput::Checkpoint { id: "final".into() },
            ),
            input(session, 6, 2, PhysicalInput::Shutdown),
        ];
        let input_hash = hash_messages(&messages);
        let checkpoint = CheckpointConfig {
            schema: HEADLESS_CHECKPOINT_CONFIG_SCHEMA.into(),
            id: "cli-equivalence".into(),
            input_sequence_hash: input_hash,
            renderer_identity_hash:
                astra_headless_protocol::RendererExecutionIdentity::cpu_reference()
                    .hash()
                    .unwrap(),
            checkpoints: vec![astra_headless_protocol::CheckpointExpectation {
                id: "final".into(),
                required: true,
                observation_hash: None,
                image_baseline_path: None,
                image_baseline_hash: None,
                audio_baseline_path: None,
                audio_baseline_hash: None,
                image_tolerance: ImageTolerance {
                    changed_pixel_ratio: 0.002,
                    ..ImageTolerance::default()
                },
                audio_tolerance: Default::default(),
            }],
        };
        let profile_path = root.path().join("profile.json");
        let package_path = root.path().join("fixture.astrapkg");
        let input_path = root.path().join("input.jsonl");
        let checkpoint_path = root.path().join("checkpoint.json");
        fs::write(&profile_path, serde_json::to_vec_pretty(&profile).unwrap()).unwrap();
        fs::write(&package_path, package_bytes).unwrap();
        write_jsonl(&input_path, &messages);
        fs::write(
            &checkpoint_path,
            serde_json::to_vec_pretty(&checkpoint).unwrap(),
        )
        .unwrap();
        Self {
            _host: host,
            _root: root,
            profile: profile_path,
            package: package_path,
            input: input_path,
            checkpoint: checkpoint_path,
            build_identity,
            messages,
        }
    }

    pub(super) fn root(&self) -> &Path {
        self._root.path()
    }
}

#[test]
pub(super) fn file_and_stdio_execute_the_same_product_sequence() {
    assert_file_stdio_equivalence(&Fixture::new());
}

pub(super) fn assert_file_stdio_equivalence(fixture: &Fixture) -> RunReport {
    let file_root = fixture.root().join("file-run");
    let output = command()
        .args([
            "run",
            "--profile",
            fixture.profile.to_str().unwrap(),
            "--package",
            fixture.package.to_str().unwrap(),
            "--input",
            fixture.input.to_str().unwrap(),
            "--artifact-root",
            file_root.to_str().unwrap(),
            "--checkpoint-config",
            fixture.checkpoint.to_str().unwrap(),
            "--build-identity",
            fixture.build_identity.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout_report: RunReport = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout_report.status, RunStatus::Passed);
    assert!(!output.stderr.is_empty());

    let stdio_root = fixture.root().join("stdio-run");
    let open = Envelope {
        schema: HEADLESS_PROTOCOL_SCHEMA.into(),
        session: "cli-equivalence".into(),
        sequence: 1,
        tick: 0,
        message: Message::Open {
            profile_path: fixture.profile.to_string_lossy().into_owned(),
            package_path: Some(fixture.package.to_string_lossy().into_owned()),
            checkpoint_config_path: Some(fixture.checkpoint.to_string_lossy().into_owned()),
            artifact_root: stdio_root.to_string_lossy().into_owned(),
        },
    };
    let mut envelopes = vec![open];
    for message in &fixture.messages[..fixture.messages.len() - 1] {
        envelopes.push(Envelope {
            schema: HEADLESS_PROTOCOL_SCHEMA.into(),
            session: message.session.clone(),
            sequence: message.sequence,
            tick: message.tick,
            message: Message::Input {
                input: message.clone(),
            },
        });
    }
    let shutdown = fixture.messages.last().unwrap();
    envelopes.push(Envelope {
        schema: HEADLESS_PROTOCOL_SCHEMA.into(),
        session: shutdown.session.clone(),
        sequence: shutdown.sequence,
        tick: shutdown.tick,
        message: Message::Shutdown,
    });
    let output = run_stdio(&fixture.build_identity, &envelopes);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        serde_json::from_str::<Envelope>(line).unwrap();
    }
    let stdio_report: RunReport = serde_json::from_slice(
        &fs::read(
            stdio_root
                .join("cli-equivalence")
                .join("cli-equivalence.run-report.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(stdout_report, stdio_report);
    assert_eq!(
        fs::read(file_root.join("artifact-manifest.json")).unwrap(),
        fs::read(
            stdio_root
                .join("cli-equivalence")
                .join("artifact-manifest.json"),
        )
        .unwrap()
    );
    stdout_report
}

#[test]
pub(super) fn duration_counts_consumed_ticks_in_file_and_stdio() {
    for (event, expected_tick) in [
        (PhysicalInput::AdvanceTicks { count: 7 }, 16),
        (
            PhysicalInput::Await {
                observation: ObservationPredicate::Exists {
                    key: "vn.text_reveal_complete".into(),
                },
                timeout_ticks: 30,
                continue_at_match: false,
            },
            10,
        ),
    ] {
        let mut fixture = Fixture::new();
        // The checkpoint consumed tick 2. Await at tick 10 must consume tick 10
        // even when its predicate matches without additional polling.
        fixture.messages.pop();
        fixture
            .messages
            .push(input("cli-equivalence", 6, 10, event));
        fixture
            .messages
            .push(input("cli-equivalence", 7, 1000, PhysicalInput::Shutdown));
        write_jsonl(&fixture.input, &fixture.messages);
        let mut checkpoint: CheckpointConfig =
            serde_json::from_slice(&fs::read(&fixture.checkpoint).unwrap()).unwrap();
        checkpoint.input_sequence_hash = hash_messages(&fixture.messages);
        fs::write(
            &fixture.checkpoint,
            serde_json::to_vec_pretty(&checkpoint).unwrap(),
        )
        .unwrap();
        let report = assert_file_stdio_equivalence(&fixture);
        assert_eq!(report.duration_ns, expected_tick * 16_666_667);
    }
}

#[test]
pub(super) fn duration_includes_await_polling_in_file_and_stdio() {
    for continue_at_match in [false, true] {
        let mut fixture = Fixture::new();
        // Leave the initial line revealing instead of pressing Enter to finish it.
        fixture.messages[2].event = PhysicalInput::AdvanceTicks { count: 1 };
        fixture.messages.pop();
        fixture.messages.push(input(
            "cli-equivalence",
            6,
            3,
            PhysicalInput::Await {
                observation: ObservationPredicate::Equals {
                    key: "vn.text_reveal_complete".into(),
                    value_hash: astra_core::Hash256::from_sha256(b"true").to_string(),
                },
                timeout_ticks: 120,
                continue_at_match,
            },
        ));
        fixture
            .messages
            .push(input("cli-equivalence", 7, 1000, PhysicalInput::Shutdown));
        write_jsonl(&fixture.input, &fixture.messages);
        let mut checkpoint: CheckpointConfig =
            serde_json::from_slice(&fs::read(&fixture.checkpoint).unwrap()).unwrap();
        checkpoint.input_sequence_hash = hash_messages(&fixture.messages);
        fs::write(
            &fixture.checkpoint,
            serde_json::to_vec_pretty(&checkpoint).unwrap(),
        )
        .unwrap();
        let report = assert_file_stdio_equivalence(&fixture);
        assert!(report.duration_ns > 3 * 16_666_667);
        assert!(report.duration_ns <= 123 * 16_666_667);
    }
}

#[test]
#[ignore = "requires a native hardware GPU runner"]
pub(super) fn gpu_run_executes_product_sequence_and_records_native_backend_identity() {
    let fixture = Fixture::new();
    let mut profile: HeadlessHostProfile =
        serde_json::from_slice(&fs::read(&fixture.profile).unwrap()).unwrap();
    profile.providers.renderer = "wgpu_offscreen".into();
    fs::write(
        &fixture.profile,
        serde_json::to_vec_pretty(&profile).unwrap(),
    )
    .unwrap();
    let artifact_root = fixture.root().join("gpu-run");
    let output = command()
        .args([
            "run",
            "--gpu",
            "--profile",
            fixture.profile.to_str().unwrap(),
            "--package",
            fixture.package.to_str().unwrap(),
            "--input",
            fixture.input.to_str().unwrap(),
            "--artifact-root",
            artifact_root.to_str().unwrap(),
            "--build-identity",
            fixture.build_identity.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: RunReport = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report.status, RunStatus::Passed);
    assert_eq!(report.render_policy, "checkpoints");
    let manifest: astra_headless_protocol::ArtifactManifest =
        serde_json::from_slice(&fs::read(artifact_root.join("artifact-manifest.json")).unwrap())
            .unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.renderer_identity.provider, "wgpu_offscreen");
    assert_eq!(
        manifest.renderer_identity.backend,
        if cfg!(target_os = "windows") {
            "dx12"
        } else if cfg!(target_os = "linux") {
            "vulkan"
        } else {
            "metal"
        }
    );
    assert!(manifest.submitted_frame_count >= manifest.rasterized_frame_count);
    assert!(manifest.rasterized_frame_count >= 1);
}

#[test]
pub(super) fn malformed_file_and_broken_stdio_fail_with_committed_blocked_reports() {
    let fixture = Fixture::new();
    let malformed = fixture.root().join("malformed.jsonl");
    fs::write(
        &malformed,
        b"{\"schema\":\"astra.user_input_sequence.v1\",\"session\":\"bad\",\"sequence\":1,\"tick\":0,\"event\":{\"type\":\"advance\"}}\n",
    )
    .unwrap();
    let blocked_root = fixture.root().join("blocked-file");
    let output = command()
        .args([
            "run",
            "--profile",
            fixture.profile.to_str().unwrap(),
            "--package",
            fixture.package.to_str().unwrap(),
            "--input",
            malformed.to_str().unwrap(),
            "--artifact-root",
            blocked_root.to_str().unwrap(),
            "--build-identity",
            fixture.build_identity.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let report: RunReport =
        serde_json::from_slice(&fs::read(blocked_root.join("run-report.json")).unwrap()).unwrap();
    assert_eq!(report.status, RunStatus::Blocked);
    assert!(!report.diagnostics.is_empty());

    let stream_root = fixture.root().join("broken-stdio");
    let open = Envelope {
        schema: HEADLESS_PROTOCOL_SCHEMA.into(),
        session: "broken-stream".into(),
        sequence: 1,
        tick: 0,
        message: Message::Open {
            profile_path: fixture.profile.to_string_lossy().into_owned(),
            package_path: Some(fixture.package.to_string_lossy().into_owned()),
            checkpoint_config_path: None,
            artifact_root: stream_root.to_string_lossy().into_owned(),
        },
    };
    let output = run_stdio(&fixture.build_identity, &[open]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("ASTRA_HEADLESS_STDIO_CLOSED_WITH_LIVE_SESSIONS"));
    let report: RunReport = serde_json::from_slice(
        &fs::read(
            stream_root
                .join("broken-stream")
                .join("broken-stream.run-report.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report.status, RunStatus::Blocked);
}
