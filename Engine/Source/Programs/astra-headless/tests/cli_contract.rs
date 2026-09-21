use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use astra_headless_protocol::{
    ButtonState, CheckpointConfig, Envelope, ImageTolerance, InputMessage, Message,
    ObservationPredicate, PhysicalInput, RunReport, RunStatus, ToleranceApproval,
    ToleranceApprovalBinding, ToleranceApproverKind, HEADLESS_CHECKPOINT_CONFIG_SCHEMA,
    HEADLESS_PROTOCOL_SCHEMA, HEADLESS_TOLERANCE_APPROVAL_SCHEMA, USER_INPUT_SEQUENCE_SCHEMA,
};
use astra_platform::HeadlessHostProfile;
use sha2::{Digest, Sha256};

struct Fixture {
    _host: astra_headless_test::HeadlessTestContext,
    _root: tempfile::TempDir,
    profile: PathBuf,
    package: PathBuf,
    input: PathBuf,
    checkpoint: PathBuf,
    approval: PathBuf,
    build_identity: PathBuf,
    messages: Vec<InputMessage>,
}

impl Fixture {
    fn new() -> Self {
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
        let mut checkpoint = CheckpointConfig {
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
            tolerance_approval: None,
        };
        let approval = ToleranceApproval {
            schema: HEADLESS_TOLERANCE_APPROVAL_SCHEMA.into(),
            approval_id: "cli.fixture.approval".into(),
            approver_kind: ToleranceApproverKind::Human,
            approver_identity: "fixture.owner".into(),
            approved_tolerance_hash: checkpoint.tolerance_hash().unwrap(),
            previous_config_hash: None,
            reason_codes: vec!["fixture.comparator.coverage".into()],
        };
        let approval_bytes = serde_json::to_vec_pretty(&approval).unwrap();
        checkpoint.tolerance_approval = Some(ToleranceApprovalBinding {
            relative_path: "approval.json".into(),
            sha256: astra_core::Hash256::from_sha256(&approval_bytes).to_string(),
        });

        let profile_path = root.path().join("profile.json");
        let package_path = root.path().join("fixture.astrapkg");
        let input_path = root.path().join("input.jsonl");
        let checkpoint_path = root.path().join("checkpoint.json");
        let approval_path = root.path().join("approval.json");
        fs::write(&profile_path, serde_json::to_vec_pretty(&profile).unwrap()).unwrap();
        fs::write(&package_path, package_bytes).unwrap();
        write_jsonl(&input_path, &messages);
        fs::write(
            &checkpoint_path,
            serde_json::to_vec_pretty(&checkpoint).unwrap(),
        )
        .unwrap();
        fs::write(&approval_path, approval_bytes).unwrap();
        Self {
            _host: host,
            _root: root,
            profile: profile_path,
            package: package_path,
            input: input_path,
            checkpoint: checkpoint_path,
            approval: approval_path,
            build_identity,
            messages,
        }
    }

    fn root(&self) -> &Path {
        self._root.path()
    }
}

#[test]
fn file_and_stdio_execute_the_same_product_sequence() {
    let fixture = Fixture::new();
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
}

#[test]
#[ignore = "requires a native hardware GPU runner"]
fn gpu_run_executes_product_sequence_and_records_native_backend_identity() {
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
fn malformed_file_and_broken_stdio_fail_with_committed_blocked_reports() {
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

    fs::write(&fixture.approval, b"{}").unwrap();
    let approval_blocked_root = fixture.root().join("blocked-approval");
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
            approval_blocked_root.to_str().unwrap(),
            "--checkpoint-config",
            fixture.checkpoint.to_str().unwrap(),
            "--build-identity",
            fixture.build_identity.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("ASTRA_HEADLESS_TOLERANCE_APPROVAL_HASH_MISMATCH"));

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

#[test]
fn artifact_limit_stops_the_run_and_preserves_only_committed_evidence() {
    let fixture = Fixture::new();
    let mut profile: HeadlessHostProfile =
        serde_json::from_slice(&fs::read(&fixture.profile).unwrap()).unwrap();
    profile.artifacts.max_artifacts = 1;
    fs::write(
        &fixture.profile,
        serde_json::to_vec_pretty(&profile).unwrap(),
    )
    .unwrap();
    let artifact_root = fixture.root().join("artifact-limit");
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
            artifact_root.to_str().unwrap(),
            "--checkpoint-config",
            fixture.checkpoint.to_str().unwrap(),
            "--build-identity",
            fixture.build_identity.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: RunReport =
        serde_json::from_slice(&fs::read(artifact_root.join("run-report.json")).unwrap()).unwrap();
    assert_eq!(report.status, RunStatus::Blocked);
    let manifest: astra_headless_protocol::ArtifactManifest =
        serde_json::from_slice(&fs::read(artifact_root.join("artifact-manifest.json")).unwrap())
            .unwrap();
    assert!(
        manifest.artifacts.len() <= 1,
        "artifact limit retained {} entries: {:?}",
        manifest.artifacts.len(),
        manifest.artifacts
    );
    assert!(!walk_files(&artifact_root).iter().any(|path| path
        .extension()
        .is_some_and(|extension| extension == "partial")));
}

#[test]
fn validate_input_cli_uses_protocol_validation_and_canonical_hashing() {
    let root = tempfile::tempdir().unwrap();
    let valid_messages = vec![
        input("validate-input", 1, 0, PhysicalInput::Resume),
        input("validate-input", 2, 0, PhysicalInput::Shutdown),
    ];
    let valid_path = root.path().join("valid.jsonl");
    write_jsonl(&valid_path, &valid_messages);
    let output = command()
        .args(["validate-input", "--input", valid_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "astra.headless_input_validation.v1");
    assert_eq!(report["status"], "pass");
    assert_eq!(report["message_count"], 2);
    assert_eq!(
        report["input_sequence_hash"],
        hash_messages(&valid_messages)
    );

    let invalid_sequences = [
        (
            "zero-advance",
            vec![
                input("validate-input", 1, 0, PhysicalInput::Resume),
                input(
                    "validate-input",
                    2,
                    0,
                    PhysicalInput::AdvanceTicks { count: 0 },
                ),
                input("validate-input", 3, 0, PhysicalInput::Shutdown),
            ],
        ),
        (
            "zero-await",
            vec![
                input("validate-input", 1, 0, PhysicalInput::Resume),
                input(
                    "validate-input",
                    2,
                    0,
                    PhysicalInput::Await {
                        observation: ObservationPredicate::Exists {
                            key: "vn.route_terminal".into(),
                        },
                        timeout_ticks: 0,
                        continue_at_match: false,
                    },
                ),
                input("validate-input", 3, 0, PhysicalInput::Shutdown),
            ],
        ),
    ];
    for (name, messages) in invalid_sequences {
        let path = root.path().join(format!("{name}.jsonl"));
        write_jsonl(&path, &messages);
        let output = command()
            .args(["validate-input", "--input", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{name}");
        assert!(output.stdout.is_empty(), "{name} unexpectedly passed");
    }

    let mut overflow = serde_json::to_value(&valid_messages).unwrap();
    overflow[0]["tick"] = serde_json::json!(u64::MAX);
    let overflow_path = root.path().join("tick-overflow.jsonl");
    fs::write(
        &overflow_path,
        overflow
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let output = command()
        .args(["validate-input", "--input", overflow_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let mut invalid_hash = serde_json::to_value(&valid_messages).unwrap();
    invalid_hash[0] = serde_json::json!({
        "schema": USER_INPUT_SEQUENCE_SCHEMA,
        "session": "validate-input",
        "sequence": 1,
        "tick": 0,
        "event": {
            "type": "await",
            "observation": {
                "kind": "equals",
                "key": "vn.route_terminal",
                "value_hash": "sha256:0"
            },
            "timeout_ticks": 1,
            "continue_at_match": false
        }
    });
    let invalid_hash_path = root.path().join("invalid-hash.jsonl");
    fs::write(
        &invalid_hash_path,
        invalid_hash
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let output = command()
        .args([
            "validate-input",
            "--input",
            invalid_hash_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

fn input(session: &str, sequence: u64, tick: u64, event: PhysicalInput) -> InputMessage {
    InputMessage {
        schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
        session: session.into(),
        sequence,
        tick,
        event,
    }
}

fn hash_messages(messages: &[InputMessage]) -> String {
    let mut digest = Sha256::new();
    for message in messages {
        digest.update(serde_json::to_vec(message).unwrap());
        digest.update(b"\n");
    }
    format!("sha256:{:x}", digest.finalize())
}

fn write_jsonl(path: &Path, messages: &[InputMessage]) {
    let mut bytes = Vec::new();
    for message in messages {
        serde_json::to_writer(&mut bytes, message).unwrap();
        bytes.push(b'\n');
    }
    fs::write(path, bytes).unwrap();
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_astra-headless"));
    command.env("ASTRA_LOG", "info");
    command
}

fn run_stdio(build_identity: &Path, envelopes: &[Envelope]) -> std::process::Output {
    let mut child = command()
        .args([
            "serve",
            "--stdio",
            "--build-identity",
            build_identity.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for envelope in envelopes {
            serde_json::to_writer(&mut stdin, envelope).unwrap();
            stdin.write_all(b"\n").unwrap();
        }
    }
    child.wait_with_output().unwrap()
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}
