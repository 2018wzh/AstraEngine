use super::*;

#[test]
pub(super) fn artifact_limit_stops_the_run_and_preserves_only_committed_evidence() {
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
pub(super) fn validate_input_cli_uses_protocol_validation_and_canonical_hashing() {
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

pub(super) fn input(session: &str, sequence: u64, tick: u64, event: PhysicalInput) -> InputMessage {
    InputMessage {
        schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
        session: session.into(),
        sequence,
        tick,
        event,
    }
}

pub(super) fn hash_messages(messages: &[InputMessage]) -> String {
    let mut digest = Sha256::new();
    for message in messages {
        digest.update(serde_json::to_vec(message).unwrap());
        digest.update(b"\n");
    }
    format!("sha256:{:x}", digest.finalize())
}

pub(super) fn write_jsonl(path: &Path, messages: &[InputMessage]) {
    let mut bytes = Vec::new();
    for message in messages {
        serde_json::to_writer(&mut bytes, message).unwrap();
        bytes.push(b'\n');
    }
    fs::write(path, bytes).unwrap();
}

pub(super) fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_astra-headless"));
    command.env("ASTRA_LOG", "info");
    command
}

pub(super) fn run_stdio(build_identity: &Path, envelopes: &[Envelope]) -> std::process::Output {
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

pub(super) fn walk_files(root: &Path) -> Vec<PathBuf> {
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
