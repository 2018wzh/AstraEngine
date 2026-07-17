use astra_core::Hash256;
use astra_headless_protocol::{
    ButtonState, InputMessage, JsonlWriter, ObservationPredicate, PhysicalInput,
    USER_INPUT_SEQUENCE_SCHEMA,
};
use astra_package::PackageReader;
use astra_platform::HeadlessHostProfile;
use astra_target::{TargetKind, TargetManifest};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn run_nativevn_headless(
    package: &Path,
    root: &Path,
    case_dir: &Path,
    product_profile: &str,
    choice_key: &str,
) -> serde_json::Value {
    let build_identity_path = env::var("ASTRA_BUILD_IDENTITY").unwrap();
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
                },
            ),
            (602, keyboard(choice_key)),
            (603, PhysicalInput::AdvanceTicks { count: 30 }),
            (633, keyboard("KeyB")),
            (634, keyboard("F5")),
            (635, keyboard("F9")),
            (636, PhysicalInput::Checkpoint { id: "final".into() }),
            (637, PhysicalInput::Shutdown),
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
    let output = Command::new(env::var("ASTRA_HEADLESS_BINARY").unwrap())
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

#[astra_headless_test::test]
fn target_validate_and_platform_probe_emit_machine_readable_reports() {
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
        "astra.platform_capability_report.v2"
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

#[astra_headless_test::test]
fn web_bundle_requires_wasm_bindgen_pair_and_embeds_canonical_host_scripts() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "web-explicit-artifacts");
    let project = write_nativevn_minimal_fixture(root, &case_dir);
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("game.astrapkg");
    run_astra(
        root,
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "minimal",
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            cooked.to_str().unwrap(),
        ],
    );
    run_astra(
        root,
        [
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            package.to_str().unwrap(),
        ],
    );
    let missing_dir = case_dir.join("missing");
    let missing = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "bundle",
            package.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--profile",
            "minimal",
            "--platform",
            "web",
            "--out",
            missing_dir.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--web-player-wasm"));

    let wasm = case_dir.join("astra_player_web_bg.wasm");
    let glue = case_dir.join("astra_player_web.js");
    fs::write(&wasm, b"\0asm\x01\0\0\0").unwrap();
    fs::write(
        &glue,
        b"export default async function init(path = new URL('astra_player_web_bg.wasm', import.meta.url)) { return WebAssembly.instantiateStreaming(fetch(path)); }",
    )
    .unwrap();
    let missing_glue = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "bundle",
            package.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--profile",
            "minimal",
            "--platform",
            "web",
            "--out",
            missing_dir.to_str().unwrap(),
            "--web-player-wasm",
            wasm.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!missing_glue.status.success());
    assert!(String::from_utf8_lossy(&missing_glue.stderr).contains("--web-player-glue"));

    let invalid_wasm = case_dir.join("invalid.wasm");
    let invalid_wasm_bundle = case_dir.join("invalid-wasm-bundle");
    fs::write(&invalid_wasm, b"not-wasm").unwrap();
    let invalid_wasm_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "bundle",
            package.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--profile",
            "minimal",
            "--platform",
            "web",
            "--out",
            invalid_wasm_bundle.to_str().unwrap(),
            "--web-player-wasm",
            invalid_wasm.to_str().unwrap(),
            "--web-player-glue",
            glue.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!invalid_wasm_output.status.success());
    assert!(String::from_utf8_lossy(&invalid_wasm_output.stderr)
        .contains("ASTRA_WEB_PLAYER_WASM_INVALID"));
    assert!(!invalid_wasm_bundle.exists());

    let bypass_glue = case_dir.join("bypass.js");
    let bypass_glue_bundle = case_dir.join("bypass-glue-bundle");
    fs::write(
        &bypass_glue,
        b"export default function init() { WebAssembly; return 'astra_player_web_bg.wasm astra-route-report'; }",
    )
    .unwrap();
    let bypass_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "bundle",
            package.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--profile",
            "minimal",
            "--platform",
            "web",
            "--out",
            bypass_glue_bundle.to_str().unwrap(),
            "--web-player-wasm",
            wasm.to_str().unwrap(),
            "--web-player-glue",
            bypass_glue.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!bypass_output.status.success());
    assert!(String::from_utf8_lossy(&bypass_output.stderr).contains("ASTRA_WEB_PLAYER_GLUE_BYPASS"));
    assert!(!bypass_glue_bundle.exists());
    let bundle = case_dir.join("bundle");
    let output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "bundle",
            package.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--profile",
            "minimal",
            "--platform",
            "web",
            "--out",
            bundle.to_str().unwrap(),
            "--web-player-wasm",
            wasm.to_str().unwrap(),
            "--web-player-glue",
            glue.to_str().unwrap(),
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let roles = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| file["role"].as_str())
        .collect::<Vec<_>>();
    assert!(roles.contains(&"web_player_wasm"));
    assert!(roles.contains(&"web_player_glue"));
    assert!(roles.contains(&"web_player_loader"));
    assert!(roles.contains(&"web_audio_worklet"));
    assert!(roles.contains(&"web_ui_component_host"));
    assert!(!bundle.join("AstraPlayer.route_model.json").exists());
    assert!(!bundle.join("astra-player.js").exists());
}

#[astra_headless_test::test]
fn package_build_writes_only_the_selected_game_target() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "package-target-filter");
    let project = case_dir.join("project.yaml");
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("game.astrapkg");

    fs::create_dir_all(&case_dir).unwrap();
    fs::write(
        &project,
        r#"
schema: astra.project.v1
id: com.example.multi
targets:
  - id: sample-game
    kind: game
    crate: astra-vn
    default_profile: desktop-release
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [windows, linux]
    packaged: true
  - id: sample-editor
    kind: editor
    binary: astra-editor
    platforms: [windows, linux]
    packaged: false
"#,
    )
    .unwrap();

    let cook_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "desktop-release",
            "--target",
            "sample-game",
            "--out",
            cooked.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        cook_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&cook_output.stderr)
    );
    let package_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "sample-game",
            "--out",
            package.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        package_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&package_output.stderr)
    );

    let bytes = fs::read(&package).unwrap();
    let package = PackageReader::open(&bytes).unwrap();
    let section = package.container().read_section("target.manifest").unwrap();
    let manifest: TargetManifest = serde_json::from_slice(&section).unwrap();
    assert_eq!(manifest.targets.len(), 1);
    assert_eq!(manifest.targets[0].id, "sample-game");
    assert_eq!(manifest.targets[0].kind, TargetKind::Game);
    assert!(manifest.targets[0].packaged);

    let policy: serde_json::Value =
        serde_json::from_slice(&package.container().read_section("provider.policy").unwrap())
            .unwrap();
    assert_eq!(policy["renderer"], "astra.renderer.wgpu");
    let bindings = policy["bindings"].as_array().unwrap();
    assert!(bindings.iter().any(|binding| {
        binding["slot"] == "presentation"
            && binding["provider_id"] == "astra.renderer.wgpu"
            && binding["context"]["required_capability"] == "renderer2d.wgpu"
    }));
    assert!(bindings.iter().any(|binding| {
        binding["slot"] == "vfs_provider"
            && binding["provider_id"] == "astra.vfs.package"
            && binding["context"]["required_capability"] == "vfs.backend.package"
    }));
    assert!(bindings.iter().any(|binding| {
        binding["slot"] == "game_runtime_provider"
            && binding["provider_id"] == "astra.runtime.native_vn"
            && binding["context"]["required_capability"] == "runtime.native_vn"
    }));

    let registry: serde_json::Value = serde_json::from_slice(
        &package
            .container()
            .read_section("plugin.extension_registry")
            .unwrap(),
    )
    .unwrap();
    assert!(registry["providers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|provider| {
            provider["provider_id"] == "astra.renderer.wgpu"
                && provider["capability"] == "renderer2d.wgpu"
                && provider["packaged"] == true
        }));
    assert!(registry["providers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|provider| {
            provider["provider_id"] == "astra.vfs.package"
                && provider["capability"] == "vfs.backend.package"
                && provider["packaged"] == true
        }));

    let wrong_target_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "sample-editor",
            "--out",
            case_dir.join("editor.astrapkg").to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!wrong_target_output.status.success());

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
fn nativevn_minimal_profile_cooks_packages_and_runs_headless() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "nativevn-minimal-profile");
    let project = write_nativevn_minimal_fixture(root, &case_dir);
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("nativevn-minimal.astrapkg");

    run_astra(
        root,
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "minimal",
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            cooked.to_str().unwrap(),
        ],
    );
    run_astra(
        root,
        [
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "nativevn-minimal-test-game",
            "--out",
            package.to_str().unwrap(),
        ],
    );

    let package_bytes = fs::read(&package).unwrap();
    let reader = PackageReader::open(&package_bytes).unwrap();
    for section in [
        "compiled.project",
        "vn.story",
        "vn.profile_manifest",
        "vn.presentation_provider_manifest",
        "vn.system_story_manifest",
        "media.font_manifest",
        "player.locale_config",
        "vn.localization.en",
    ] {
        assert!(reader.has_section(section), "missing section {section}");
    }
    #[derive(serde::Deserialize)]
    struct ProfileManifest {
        schema: String,
        target: String,
        profiles: Vec<String>,
    }
    let profile_manifest: ProfileManifest = postcard::from_bytes(
        &reader
            .container()
            .read_section("vn.profile_manifest")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(profile_manifest.schema, "astra.vn.profile_manifest.v1");
    assert_eq!(profile_manifest.target, "nativevn-minimal-test-game");
    assert_eq!(profile_manifest.profiles, ["minimal"]);

    let headless_report = run_nativevn_headless(&package, root, &case_dir, "minimal", "Digit1");
    assert_eq!(headless_report["schema"], "astra.headless_run_report.v2");
    assert_eq!(headless_report["status"], "passed");
    assert!(headless_report["submitted_frame_count"].as_u64().unwrap() > 0);
    assert!(headless_report["rasterized_frame_count"].as_u64().unwrap() > 0);
    assert_eq!(headless_report["checkpoint_results"][0]["id"], "final");
    assert_eq!(headless_report["checkpoint_results"][0]["passed"], true);

    fs::remove_dir_all(case_dir).unwrap();
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
}

fn write_nativevn_minimal_fixture(root: &Path, case_dir: &Path) -> PathBuf {
    let project = case_dir.join("project.yaml");
    for directory in [
        "Scripts",
        "UI",
        "Controllers",
        "Themes",
        "Assets/Fonts",
        "Localization",
    ] {
        fs::create_dir_all(case_dir.join(directory)).unwrap();
    }
    fs::write(
        case_dir.join("Scripts/main.astra"),
        nativevn_minimal_story(),
    )
    .unwrap();
    fs::copy(
        root.join("Examples/NativeVN/UI/flagship.astra"),
        case_dir.join("UI/flagship.astra"),
    )
    .unwrap();
    fs::copy(
        root.join("Examples/NativeVN/Controllers/standard_ui.luau"),
        case_dir.join("Controllers/standard_ui.luau"),
    )
    .unwrap();
    for theme in ["classic.json", "modern.json"] {
        fs::copy(
            root.join("Examples/NativeVN/Themes").join(theme),
            case_dir.join("Themes").join(theme),
        )
        .unwrap();
    }
    let font = fs::read(root.join("Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")).unwrap();
    fs::write(case_dir.join("Assets/Fonts/Poppins-Regular.ttf"), &font).unwrap();
    fs::write(
        case_dir.join("Assets/Fonts/Poppins-Regular.ttf.astra-asset.yaml"),
        format!(
            r#"schema: astra.asset.v1
id: asset:/font/ui
source: Assets/Fonts/Poppins-Regular.ttf
source_hash: {}
type: font.ttf
license: OFL-1.1
importer: astra.import.font
font:
  family: Poppins
  face_index: 0
  subset: latin-basic
  coverage:
    - {{ start: 32, end: 126 }}
cook:
  processor: astra.cook.font
  target_profiles: [minimal]
  params: {{}}
review: accepted
"#,
            Hash256::from_sha256(&font)
        ),
    )
    .unwrap();
    fs::write(
        case_dir.join("Localization/en.json"),
        br#"{"schema":"astra.vn.localization_table.v1","locale":"en","strings":{"speaker.narrator":"Narrator","line.hello":"Hello","line.library":"Library","line.rooftop":"Rooftop","choice.where":"Where?","choice.library":"Library","choice.rooftop":"Rooftop","system.back":"Back","system.backlog":"Backlog","system.config":"Config","system.config.auto_delay":"Auto delay","system.config.high_contrast":"High contrast","system.config.master_volume":"Master volume","system.config.text_speed":"Text speed","system.continue":"Continue","system.gallery":"Gallery","system.load":"Load","system.localization_preview":"Localization preview","system.replay":"Replay","system.route_chart":"Route chart","system.save":"Save","system.title":"Title","system.voice_replay":"Voice replay"}}"#,
    )
    .unwrap();
    fs::write(
        &project,
        r#"schema: astra.project.v1
id: com.astra.nativevn.engine-test
platform_profiles:
  web-release-chrome:
    schema: astra.platform_host_profile.v2
    id: web-release-chrome
    platform: web
    target: nativevn-minimal-test-game
    package_id: com.astra.nativevn.engine-test
    renderer: { providers: [webgpu], allow_software: false }
    decode: { providers: [webcodecs], allow_software: false }
    audio: { providers: [webaudio], allow_software: false }
    save: { providers: [opfs], allow_software: false }
    package_sources: [{ kind: bundled }]
    limits: { command_queue_capacity: 16, event_queue_capacity: 32, max_frame_bytes: 1048576, max_audio_frames: 48000, max_package_read_bytes: 1048576 }
    package_cache: { max_entry_bytes: 1048576, max_total_bytes: 4194304 }
targets:
  - id: nativevn-minimal-test-game
    kind: game
    crate: astra-vn
    default_profile: minimal
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [windows, web]
    packaged: true
nativevn:
  default_locale: en
  sources: [Scripts]
  ui_sources: [UI]
  ui_themes: [Themes]
  ui_controllers: [Controllers]
  profiles: [minimal]
  asset_roots: [Assets]
  display:
    original_resolution: { width: 320, height: 180 }
    scale_filter: nearest
package_sections:
  - id: vn.localization.en
    schema: astra.vn.localization_table.v1
    path: Localization/en.json
    codec: raw
    targets: [nativevn-minimal-test-game]
    profiles: [minimal]
"#,
    )
    .unwrap();
    project
}

fn nativevn_minimal_story() -> &'static str {
    r#"story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.hello speaker:narrator #@id line.hello
    choice key:choice.where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop
state library #@id state.library
  scene library #@id scene.library
    text key:line.library speaker:narrator #@id line.library
state rooftop #@id state.rooftop
  scene rooftop #@id scene.rooftop
    text key:line.rooftop speaker:narrator #@id line.rooftop

story system #@id story.system
state title #@id state.system.title
  scene title #@id scene.system.title
    system_page kind:title policy:astra.policy.standard #@id page.title
state save #@id state.system.save
  scene save #@id scene.system.save
    system_page kind:save policy:astra.policy.standard #@id page.save
state load #@id state.system.load
  scene load #@id scene.system.load
    system_page kind:load policy:astra.policy.standard #@id page.load
state config #@id state.system.config
  scene config #@id scene.system.config
    system_page kind:config policy:astra.policy.standard #@id page.config
state backlog #@id state.system.backlog
  scene backlog #@id scene.system.backlog
    system_page kind:backlog policy:astra.policy.standard #@id page.backlog
state gallery #@id state.system.gallery
  scene gallery #@id scene.system.gallery
    system_page kind:gallery policy:astra.policy.standard #@id page.gallery
state replay #@id state.system.replay
  scene replay #@id scene.system.replay
    system_page kind:replay policy:astra.policy.standard #@id page.replay
state voice_replay #@id state.system.voice_replay
  scene voice_replay #@id scene.system.voice_replay
    system_page kind:voice_replay policy:astra.policy.standard #@id page.voice_replay
state route_chart #@id state.system.route_chart
  scene route_chart #@id scene.system.route_chart
    system_page kind:route_chart policy:astra.policy.standard #@id page.route_chart
state localization_preview #@id state.system.localization_preview
  scene localization_preview #@id scene.system.localization_preview
    system_page kind:localization_preview policy:astra.policy.standard #@id page.localization_preview
"#
}

fn run_astra<const N: usize>(root: &Path, args: [&str; N]) -> Output {
    run_astra_in(root, args)
}

fn run_astra_in<const N: usize>(cwd: &Path, args: [&str; N]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn unique_case_dir(root: &Path, name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    root.join("target")
        .join("astra-cli-tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()))
}
