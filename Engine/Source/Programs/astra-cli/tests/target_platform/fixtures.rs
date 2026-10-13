use super::*;

pub(super) fn run_nativevn_headless(
    package: &Path,
    root: &Path,
    case_dir: &Path,
    product_profile: &str,
    choice_key: &str,
) -> serde_json::Value {
    let _host = astra_headless_test::HeadlessTestContext::start().unwrap();
    let build_identity_path = astra_headless_test::headless_build_identity_path()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let build_identity: serde_json::Value =
        serde_json::from_slice(&fs::read(&build_identity_path).unwrap()).unwrap();
    let build_fingerprint = build_identity["identity_hash"].as_str().unwrap();
    let package_bytes = fs::read(package).unwrap();
    let (target, package_id) = if product_profile == "minimal" {
        (
            "nativevn-minimal-test-game",
            "com.astra.nativevn.engine-test",
        )
    } else {
        (
            "nativevn-flagship-game",
            "com.astra.nativevn.signal-glass-rain",
        )
    };
    let mut profile = HeadlessHostProfile::reference(
        target,
        package_id,
        build_fingerprint,
        Hash256::from_sha256(&package_bytes).to_string(),
    );
    profile.id = format!("headless-{product_profile}");
    profile.product_profile = product_profile.into();
    if cfg!(feature = "ffmpeg-vcpkg") {
        profile.providers.video_decode = "ffmpeg-vcpkg".into();
        // The checked-in advanced fixture is 90 RGBA frames at 1920x1080.
        // Bind the run to a finite budget that covers the complete decoded
        // stream plus its validated postcard envelope.
        profile.max_video_frames = 90;
        profile.max_decode_output_bytes = 768 * 1024 * 1024;
    }
    profile.artifacts.namespace = format!("headless-{product_profile}");
    profile.artifacts.required_checkpoints = vec!["final".into()];
    let profile_path = case_dir.join(format!("headless-{product_profile}.json"));
    fs::write(&profile_path, serde_json::to_vec_pretty(&profile).unwrap()).unwrap();

    let session = format!("nativevn-{product_profile}");
    let keyboard = |physical_key: &str| PhysicalInput::Keyboard {
        physical_key: physical_key.into(),
        logical_key: None,
        state: ButtonState::Pressed,
        repeat: false,
    };
    let pending_choices_hash = Hash256::from_sha256(
        &serde_json::to_vec(&vec!["choice.library", "choice.rooftop"]).unwrap(),
    )
    .to_string();
    let active_video_hash = Hash256::from_sha256(&serde_json::to_vec(&true).unwrap()).to_string();
    let active_voice_hash = active_video_hash.clone();
    let inactive_voice_hash =
        Hash256::from_sha256(&serde_json::to_vec(&false).unwrap()).to_string();
    let events = if cfg!(feature = "ffmpeg-vcpkg") && product_profile != "minimal" {
        vec![
            (0, PhysicalInput::Resume),
            (0, PhysicalInput::Focus { focused: true }),
            (1, PhysicalInput::AdvanceTicks { count: 30 }),
            (
                31,
                PhysicalInput::Await {
                    observation: ObservationPredicate::Equals {
                        key: "media.active_video".into(),
                        value_hash: active_video_hash,
                    },
                    timeout_ticks: 1,
                    continue_at_match: false,
                },
            ),
            (32, keyboard("Enter")),
            (33, keyboard("Enter")),
            (
                34,
                PhysicalInput::Await {
                    observation: ObservationPredicate::Equals {
                        key: "media.active_voice".into(),
                        value_hash: active_voice_hash,
                    },
                    timeout_ticks: 1,
                    continue_at_match: false,
                },
            ),
            (
                35,
                PhysicalInput::Await {
                    observation: ObservationPredicate::Equals {
                        key: "media.active_voice".into(),
                        value_hash: inactive_voice_hash,
                    },
                    timeout_ticks: 300,
                    continue_at_match: false,
                },
            ),
            (335, keyboard("Enter")),
            (
                336,
                PhysicalInput::Await {
                    observation: ObservationPredicate::Equals {
                        key: "vn.pending_choices".into(),
                        value_hash: pending_choices_hash,
                    },
                    timeout_ticks: 60,
                    continue_at_match: false,
                },
            ),
            (396, keyboard(choice_key)),
            (397, PhysicalInput::AdvanceTicks { count: 30 }),
            (427, keyboard("KeyB")),
            (428, keyboard("F5")),
            (429, keyboard("F9")),
            (430, PhysicalInput::Checkpoint { id: "final".into() }),
            (431, PhysicalInput::Shutdown),
        ]
    } else {
        vec![
            (0, PhysicalInput::Resume),
            (0, PhysicalInput::Focus { focused: true }),
            (1, PhysicalInput::AdvanceTicks { count: 300 }),
            (301, keyboard("Enter")),
            (
                302,
                PhysicalInput::Await {
                    observation: ObservationPredicate::Equals {
                        key: "vn.pending_choices".into(),
                        value_hash: pending_choices_hash,
                    },
                    timeout_ticks: 300,
                    continue_at_match: false,
                },
            ),
            (602, keyboard(choice_key)),
            (603, PhysicalInput::AdvanceTicks { count: 30 }),
            (633, keyboard("KeyB")),
            (634, keyboard("F5")),
            (635, keyboard("F9")),
            (636, keyboard("F5")),
            (637, PhysicalInput::Checkpoint { id: "final".into() }),
            (638, PhysicalInput::Shutdown),
        ]
    };
    let input_path = case_dir.join(format!("headless-{product_profile}.jsonl"));
    let mut writer = JsonlWriter::new(fs::File::create(&input_path).unwrap());
    for (index, (tick, event)) in events.into_iter().enumerate() {
        writer
            .write(&InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: session.clone(),
                sequence: index as u64 + 1,
                tick,
                event,
            })
            .unwrap();
    }
    let artifact_root = case_dir.join(format!("headless-{product_profile}-artifacts"));
    let output = Command::new(astra_headless_test::headless_binary_path().unwrap())
        .args([
            "run",
            "--profile",
            profile_path.to_str().unwrap(),
            "--package",
            package.to_str().unwrap(),
            "--input",
            input_path.to_str().unwrap(),
            "--artifact-root",
            artifact_root.to_str().unwrap(),
            "--build-identity",
            &build_identity_path,
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
pub(super) fn target_validate_and_platform_probe_emit_machine_readable_reports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();

    let target_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "target",
            "validate",
            "Docs/samples/astra-vn-script/project.yaml",
            "--target",
            "nativevn-game",
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        target_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&target_output.stderr)
    );
    let target_report: serde_json::Value = serde_json::from_slice(&target_output.stdout).unwrap();
    assert_eq!(target_report["schema"], "astra.target_validation_report.v1");
    assert_eq!(target_report["status"], "pass");

    let platform_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "platform",
            "probe",
            "--platform",
            "windows",
            "--target",
            "nativevn-game",
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        platform_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&platform_output.stderr)
    );
    let platform_report: serde_json::Value =
        serde_json::from_slice(&platform_output.stdout).unwrap();
    assert_eq!(
        platform_report["schema"],
        "astra.platform_capability_report.v3"
    );
    assert_eq!(platform_report["platform"], "windows");
    if cfg!(windows) {
        assert_eq!(platform_report["sdk_status"], "present");
        assert!(platform_report["renderer"]["selected"].is_null());
        assert!(platform_report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| { diagnostic["code"] == "ASTRA_PLATFORM_RUNTIME_PROBE_REQUIRED" }));
    } else {
        assert_eq!(platform_report["sdk_status"], "missing");
    }

    let web_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "platform",
            "probe",
            "--platform",
            "web",
            "--target",
            "nativevn-web",
            "--format",
            "json",
        ])
        .env("ASTRA_WEB_SDK", "1")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        web_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&web_output.stderr)
    );
    let web_report: serde_json::Value = serde_json::from_slice(&web_output.stdout).unwrap();
    assert_eq!(web_report["platform"], "web");
    if cfg!(target_arch = "wasm32") {
        assert_eq!(web_report["sdk_status"], "present");
    } else {
        assert_eq!(web_report["sdk_status"], "missing");
        assert!(web_report["renderer"]["available"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
