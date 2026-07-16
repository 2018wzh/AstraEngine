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
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
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
    let mut profile = HeadlessHostProfile::reference(
        "nativevn-game",
        "com.example.nativevn",
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
    let events = if cfg!(feature = "ffmpeg-vcpkg") {
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
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("game.astrapkg");
    run_astra(
        root,
        [
            "cook",
            "Examples/NativeVN/project.yaml",
            "--profile",
            "classic",
            "--target",
            "nativevn-game",
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
            "nativevn-game",
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
            "nativevn-game",
            "--profile",
            "classic",
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
            "nativevn-game",
            "--profile",
            "classic",
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
            "nativevn-game",
            "--profile",
            "classic",
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
            "nativevn-game",
            "--profile",
            "classic",
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
            "nativevn-game",
            "--profile",
            "classic",
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
fn package_build_includes_target_filtered_tsuinosora_sections() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "tsuinosora-package-sections");
    let project = case_dir.join("project.yaml");
    let scripts = case_dir.join("Scripts");
    let ui = case_dir.join("UI");
    let controllers = case_dir.join("Controllers");
    let themes = case_dir.join("Themes");
    let localization = case_dir.join("Localization");
    let fonts = case_dir.join("Assets/Fonts");
    let package_sections = case_dir.join("PackageSections");
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("tsuinosora.astrapkg");

    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&ui).unwrap();
    fs::create_dir_all(&controllers).unwrap();
    fs::create_dir_all(&themes).unwrap();
    fs::create_dir_all(&localization).unwrap();
    fs::create_dir_all(&fonts).unwrap();
    fs::create_dir_all(&package_sections).unwrap();
    fs::write(
        scripts.join("main.astra"),
        fs::read_to_string(root.join("Examples/NativeVN/Scripts/main.astra")).unwrap(),
    )
    .unwrap();
    for name in ["classic.astra", "modern.astra"] {
        fs::copy(root.join("Examples/NativeVN/UI").join(name), ui.join(name)).unwrap();
    }
    fs::copy(
        root.join("Examples/NativeVN/Controllers/standard_ui.luau"),
        controllers.join("standard_ui.luau"),
    )
    .unwrap();
    for name in ["classic.json", "modern.json"] {
        fs::copy(
            root.join("Examples/NativeVN/Themes").join(name),
            themes.join(name),
        )
        .unwrap();
    }
    fs::copy(
        root.join("Examples/NativeVN/Localization/en.json"),
        localization.join("en.json"),
    )
    .unwrap();
    for name in [
        "Poppins-Regular.ttf",
        "Poppins-Regular.ttf.astra-asset.yaml",
    ] {
        fs::copy(
            root.join("Examples/NativeVN/Assets/Fonts").join(name),
            fonts.join(name),
        )
        .unwrap();
    }
    write_tsuinosora_package_sections(&package_sections);
    fs::write(
        &project,
        r#"
schema: astra.project.v1
id: com.example.tsuinosora.stage3
targets:
  - id: tsuinosora-internal-game
    kind: game
    crate: astra-vn
    default_profile: classic
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [headless, windows, web]
    packaged: true
  - id: tsuinosora-patch-game
    kind: game
    crate: astra-vn
    default_profile: classic
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [headless, windows, web]
    packaged: true
nativevn:
  default_locale: en
  sources:
    - Scripts
  ui_sources:
    - UI
  controllers:
    - Controllers/standard_ui.luau
  themes:
    - Themes/classic.json
    - Themes/modern.json
  profiles: [classic, modern]
  asset_roots: [Assets]
  display:
    original_resolution:
      width: 800
      height: 600
    scale_filter: linear
package_sections:
  - id: vn.localization.en
    schema: astra.vn.localization_table.v1
    path: Localization/en.json
    codec: raw
    targets: [tsuinosora-internal-game]
    profiles: [classic]
  - id: tsuinosora.reference_evidence
    schema: tsuinosora.visual_reference_report.v1
    path: PackageSections/reference_evidence.json
    codec: raw
  - id: tsuinosora.asset_analysis
    schema: tsuinosora.asset_analysis.v1
    path: PackageSections/asset_analysis.json
    codec: raw
  - id: tsuinosora.conversion_manifest
    schema: tsuinosora.conversion_report.v1
    path: PackageSections/conversion_report.json
    codec: raw
  - id: tsuinosora.mount_policy
    schema: tsuinosora.mount_policy.v1
    path: PackageSections/mount_policy.internal.json
    codec: raw
    targets: [tsuinosora-internal-game]
  - id: tsuinosora.mount_policy
    schema: tsuinosora.mount_policy.v1
    path: PackageSections/mount_policy.patch.json
    codec: raw
    targets: [tsuinosora-patch-game]
"#,
    )
    .unwrap();

    let cook_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "classic",
            "--target",
            "tsuinosora-internal-game",
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
            "tsuinosora-internal-game",
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
    let reader = PackageReader::open(&bytes).unwrap();
    for section in [
        "tsuinosora.reference_evidence",
        "tsuinosora.asset_analysis",
        "tsuinosora.conversion_manifest",
        "tsuinosora.mount_policy",
    ] {
        assert!(reader.has_section(section), "missing section {section}");
    }
    let mount_policy: serde_json::Value = serde_json::from_slice(
        &reader
            .container()
            .read_section("tsuinosora.mount_policy")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mount_policy["target"], "tsuinosora-internal-game");

    let validate_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "validate",
            package.to_str().unwrap(),
            "--profile",
            "classic",
            "--target",
            "tsuinosora-internal-game",
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        validate_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&validate_output.stderr)
    );
    let release_report: serde_json::Value =
        serde_json::from_slice(&validate_output.stdout).unwrap();
    assert!(release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "tsuinosora.mount_policy" && check["status"] == "pass"));
    assert!(!release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["status"] == "blocked"));

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
#[ignore = "superseded by Tools/run_platform_host_acceptance.py real-host evidence"]
fn tsuinosora_synthetic_gate_runs_internal_and_patch_player_routes() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "tsuinosora-player-gate");
    let project = case_dir.join("project.yaml");
    let scripts = case_dir.join("Scripts");
    let package_sections = case_dir.join("PackageSections");

    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&package_sections).unwrap();
    fs::write(
        scripts.join("main.astra"),
        fs::read_to_string(root.join("Examples/NativeVN/Scripts/main.astra")).unwrap(),
    )
    .unwrap();
    write_tsuinosora_package_sections(&package_sections);
    write_tsuinosora_synthetic_scenarios(&case_dir);
    write_tsuinosora_synthetic_project(&project);

    for target in ["tsuinosora-internal-game", "tsuinosora-patch-game"] {
        for profile in ["classic", "modern"] {
            let cooked = case_dir.join(format!("cooked-{target}-{profile}"));
            let package = case_dir.join(format!("{target}-{profile}.astrapkg"));
            run_astra(
                root,
                [
                    "cook",
                    project.to_str().unwrap(),
                    "--profile",
                    profile,
                    "--target",
                    target,
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
                    target,
                    "--out",
                    package.to_str().unwrap(),
                ],
            );
            let validate_output = run_astra(
                root,
                [
                    "package",
                    "validate",
                    package.to_str().unwrap(),
                    "--profile",
                    profile,
                    "--target",
                    target,
                    "--format",
                    "json",
                ],
            );
            let release_report: serde_json::Value =
                serde_json::from_slice(&validate_output.stdout).unwrap();
            assert!(!release_report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["status"] == "blocked"));
            for id in [
                "tsuinosora.reference_evidence",
                "tsuinosora.asset_analysis",
                "tsuinosora.conversion_manifest",
                "tsuinosora.mount_policy",
            ] {
                assert!(
                    release_report["checks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|check| check["id"] == id && check["status"] == "pass"),
                    "missing release pass check {id} for {target}/{profile}"
                );
            }
            if profile == "modern" {
                assert!(release_report["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|check| check["id"] == "tsuinosora.modern_profile_report"
                        && check["status"] == "pass"));
            }

            let platforms: &[&str] = &["headless", "windows", "web"];
            for &platform in platforms {
                let scenario = tsuinosora_scenario_ref(target, profile, platform);
                let scenario_path = case_dir.join(&scenario);
                let run_output = run_astra(
                    root,
                    [
                        "test",
                        "run",
                        scenario_path.to_str().unwrap(),
                        "--headless",
                        "--target",
                        target,
                        "--profile",
                        profile,
                        "--platform",
                        platform,
                        "--package",
                        package.to_str().unwrap(),
                        "--format",
                        "json",
                    ],
                );
                let scenario_report: serde_json::Value =
                    serde_json::from_slice(&run_output.stdout).unwrap();
                assert_eq!(scenario_report["status"], "pass");
                assert_eq!(scenario_report["target"], target);
                assert_eq!(scenario_report["profile"], profile);
                assert_eq!(scenario_report["platform"], platform);
                assert!(scenario_report["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|check| check["id"] == "player_route.full" && check["status"] == "pass"));
            }

            let bundle_platforms: &[&str] = &["windows", "web"];
            for &platform in bundle_platforms {
                let bundle = case_dir.join(format!("bundle-{target}-{profile}-{platform}"));
                let bundle_output = run_astra_in(
                    &case_dir,
                    [
                        "package",
                        "bundle",
                        package.to_str().unwrap(),
                        "--target",
                        target,
                        "--profile",
                        profile,
                        "--platform",
                        platform,
                        "--out",
                        bundle.to_str().unwrap(),
                        "--format",
                        "json",
                    ],
                );
                let manifest: serde_json::Value =
                    serde_json::from_slice(&bundle_output.stdout).unwrap();
                assert_eq!(
                    manifest["mount_policy"], "AstraPlayer.mount_policy.json",
                    "bundle must carry sanitized mount policy for {target}/{profile}/{platform}"
                );
                assert!(bundle.join("AstraPlayer.mount_policy.json").exists());
                let scenario = tsuinosora_scenario_ref(target, profile, platform);
                let mut patch_mount_root = None;
                let route_report =
                    if target == "tsuinosora-patch-game" && platform == "windows" {
                        let no_mount_report = run_windows_bundle_route(&bundle, &scenario);
                        assert_eq!(no_mount_report["status"], "blocked");
                        assert!(no_mount_report["checks"].as_array().unwrap().iter().any(
                            |check| {
                                check["id"] == "player.patch_direct_read"
                                    && check["status"] == "blocked"
                            }
                        ));
                        let mount_root =
                            install_patch_mount_fixture(&case_dir, &bundle, &scenario, profile);
                        let report = run_windows_bundle_route_with_mount(
                            &bundle,
                            &scenario,
                            "original",
                            &mount_root,
                        );
                        assert_eq!(report["status"], "pass");
                        assert!(report["checks"].as_array().unwrap().iter().any(|check| {
                            check["id"] == "player.patch_mount_probe" && check["status"] == "pass"
                        }));
                        assert!(report["checks"].as_array().unwrap().iter().any(|check| {
                            check["id"] == "player.patch_mount_asset" && check["status"] == "pass"
                        }));
                        assert!(!serde_json::to_string(&report)
                            .unwrap()
                            .replace('\\', "/")
                            .contains(&mount_root.to_string_lossy().replace('\\', "/")));
                        let scenario_path = bundle.join(&scenario);
                        let scenario_text = fs::read_to_string(&scenario_path).unwrap();
                        fs::write(
                            &scenario_path,
                            scenario_text.replace("role: background", "role: not_a_classification"),
                        )
                        .unwrap();
                        let invalid_role_report = run_windows_bundle_route_with_mount(
                            &bundle,
                            &scenario,
                            "original",
                            &mount_root,
                        );
                        assert_eq!(invalid_role_report["status"], "blocked");
                        assert!(invalid_role_report["checks"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|check| {
                                check["id"] == "player.patch_mount_asset"
                                    && check["status"] == "blocked"
                            }));
                        fs::write(scenario_path, scenario_text).unwrap();
                        patch_mount_root = Some(mount_root);
                        report
                    } else if platform == "windows" {
                        run_windows_bundle_route(&bundle, &scenario)
                    } else {
                        run_web_bundle_route_in_browser(&bundle, &scenario)
                    };
                assert_eq!(route_report["schema"], "astra.player_route_report.v1");
                assert_eq!(route_report["status"], "pass");
                assert_eq!(route_report["target"], target);
                assert_eq!(route_report["profile"], profile);
                assert_eq!(route_report["platform"], platform);
                assert_eq!(route_report["package_hash"], manifest["package_hash"]);
                assert_eq!(route_report["scenario"], scenario);
                assert!(route_report["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|check| {
                        check["id"] == "player.route.full" && check["status"] == "pass"
                    }));
                assert!(route_report["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|check| {
                        check["id"] == "player.mount_policy" && check["status"] == "pass"
                    }));
                if target == "tsuinosora-patch-game" {
                    assert!(route_report["checks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|check| {
                            check["id"] == "player.patch_direct_read" && check["status"] == "pass"
                        }));
                    if profile == "classic" && platform == "web" {
                        fs::write(
                            bundle.join("AstraPlayer.mount_policy.json"),
                            serde_json::to_vec_pretty(&serde_json::json!({
                                "schema": "tsuinosora.mount_policy.v1",
                                "target": "tsuinosora-patch-game",
                                "status": "pass",
                                "aliases": [{
                                    "alias": "original",
                                    "value": "wrong_alias",
                                    "hash_policy": "manifest_required",
                                    "fallback": "blocking"
                                }],
                                "diagnostics": []
                            }))
                            .unwrap(),
                        )
                        .unwrap();
                        let blocked_report = run_web_bundle_route_in_browser(&bundle, &scenario);
                        assert_eq!(blocked_report["status"], "blocked");
                        assert!(blocked_report["checks"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|check| {
                                (check["id"] == "player.patch_direct_read"
                                    || check["id"] == "player.mount_policy"
                                    || check["id"] == "player.mount_policy_hash")
                                    && check["status"] == "blocked"
                            }));
                    }
                    if profile == "classic" && platform == "windows" {
                        let mut tampered_policy: serde_json::Value = serde_json::from_slice(
                            &fs::read(bundle.join("AstraPlayer.mount_policy.json")).unwrap(),
                        )
                        .unwrap();
                        tampered_policy["tamper_marker"] =
                            serde_json::json!("manifest_hash_must_match");
                        fs::write(
                            bundle.join("AstraPlayer.mount_policy.json"),
                            serde_json::to_vec_pretty(&tampered_policy).unwrap(),
                        )
                        .unwrap();
                        let mount_root = patch_mount_root.as_ref().unwrap();
                        let blocked_report = run_windows_bundle_route_with_mount(
                            &bundle, &scenario, "original", mount_root,
                        );
                        assert_eq!(blocked_report["status"], "blocked");
                        assert!(blocked_report["checks"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|check| {
                                check["id"] == "player.mount_policy_hash"
                                    && check["status"] == "blocked"
                            }));
                        assert!(blocked_report["checks"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|check| {
                                check["id"] == "player.patch_direct_read"
                                    && check["status"] == "blocked"
                            }));
                    }
                }
                assert!(!serde_json::to_string(&route_report)
                    .unwrap()
                    .replace('\\', "/")
                    .contains(&root.to_string_lossy().replace('\\', "/")));
            }
        }
    }

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
#[ignore = "superseded by Tools/run_platform_host_acceptance.py real-host evidence"]
fn tsuinosora_demo_slice_generates_playable_nativevn_and_player_routes() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "tsuinosora-demo-slice");
    let (project, work_root) = write_tsuinosora_demo_slice_fixture(root, &case_dir);

    let demo_report = run_tsuinosora_demo_slice_tool(root, &case_dir.join("demo.config.json"));
    assert_eq!(demo_report["schema"], "tsuinosora.demo_slice_report.v1");
    assert_eq!(demo_report["status"], "pass");
    assert_eq!(demo_report["route_count"], 1);
    assert!(work_root.join("nativevn").join("project.yaml").exists());
    assert!(!serde_json::to_string(&demo_report)
        .unwrap()
        .replace('\\', "/")
        .contains(&case_dir.to_string_lossy().replace('\\', "/")));
    assert_eq!(
        demo_report["automation_targets"],
        serde_json::json!([{
            "target": "tsuinosora-internal-game",
            "profiles": ["classic"],
            "platforms": ["headless", "windows", "web"]
        }])
    );

    let target = "tsuinosora-internal-game";
    let profile = "classic";
    let cooked = case_dir.join(format!("demo-cooked-{target}-{profile}"));
    let package = case_dir.join(format!("demo-{target}-{profile}.astrapkg"));
    run_astra(
        root,
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            profile,
            "--target",
            target,
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
            target,
            "--out",
            package.to_str().unwrap(),
        ],
    );
    assert_tsuinosora_demo_package_validates(root, &package, target, profile);
    assert_tsuinosora_demo_headless_route(root, &work_root, &package, target, profile, "headless");

    for platform in ["windows", "web"] {
        let bundle = case_dir.join(format!("demo-bundle-{target}-{profile}-{platform}"));
        let bundle_manifest = bundle_tsuinosora_demo_package(
            &work_root.join("nativevn"),
            &package,
            target,
            profile,
            platform,
            &bundle,
        );
        let scenario = tsuinosora_demo_scenario_ref(target, profile, platform);
        let route_report = if platform == "windows" {
            run_windows_bundle_route(&bundle, &scenario)
        } else {
            run_web_bundle_route_in_browser(&bundle, &scenario)
        };
        assert_eq!(route_report["schema"], "astra.player_route_report.v1");
        assert_eq!(route_report["status"], "pass");
        assert_eq!(route_report["target"], target);
        assert_eq!(route_report["profile"], profile);
        assert_eq!(route_report["platform"], platform);
        assert_eq!(
            route_report["package_hash"],
            bundle_manifest["package_hash"]
        );
        assert!(route_report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| { check["id"] == "player.route.full" && check["status"] == "pass" }));
        assert!(!serde_json::to_string(&route_report)
            .unwrap()
            .replace('\\', "/")
            .contains(&case_dir.to_string_lossy().replace('\\', "/")));
    }

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
#[ignore = "superseded by Tools/run_platform_host_acceptance.py real-host evidence"]
fn tsuinosora_internal_demo_builds_asset_package_and_bundles() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "tsuinosora-internal-assets");
    let (project, work_root) = write_tsuinosora_demo_slice_fixture(root, &case_dir);

    let demo_report = run_tsuinosora_demo_slice_tool(root, &case_dir.join("demo.config.json"));
    assert_eq!(demo_report["status"], "pass");

    let target = "tsuinosora-internal-game";
    let profile = "classic";
    let cooked = case_dir.join("internal-cooked");
    let package = case_dir.join("internal.astrapkg");
    run_astra(
        root,
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            profile,
            "--target",
            target,
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
            target,
            "--out",
            package.to_str().unwrap(),
        ],
    );

    let package_bytes = fs::read(&package).unwrap();
    let reader = PackageReader::open(&package_bytes).unwrap();
    assert!(!reader.has_section("asset.registry"));
    let windows_scenario_ref = tsuinosora_demo_scenario_ref(target, profile, "windows");
    assert!(
        reader.has_section(&windows_scenario_ref),
        "package must include scenario ref section {windows_scenario_ref}"
    );
    let vfs_manifest: serde_json::Value = serde_json::from_slice(
        &reader
            .container()
            .read_section("asset.vfs_manifest")
            .unwrap(),
    )
    .unwrap();
    let entries = vfs_manifest["entries"]
        .as_array()
        .expect("asset VFS manifest entries array");
    assert!(
        entries.iter().any(|entry| {
            entry["vfs_uri"] == "package:/native-assets/backgrounds/bg.png"
                && entry["source"]["section_id"]
                    .as_str()
                    .is_some_and(|section| reader.has_section(section))
                && entry["hash"].as_str().is_some_and(|hash| hash.starts_with("sha256:"))
                && entry["size"].as_u64().is_some_and(|bytes| bytes > 0)
        }),
        "asset.vfs_manifest should map native-assets/backgrounds/bg.png to a packaged section: {vfs_manifest:#?}"
    );
    let catalog: serde_json::Value =
        serde_json::from_slice(&reader.container().read_section("asset.catalog").unwrap()).unwrap();
    let assets = catalog["assets"].as_array().expect("asset catalog array");
    assert!(
        assets.iter().any(|asset| {
            asset["vfs_uri"] == "package:/native-assets/backgrounds/bg.png"
                && asset["media_kind"] == "background"
                && asset["asset_id"].as_str().is_some_and(|id| !id.is_empty())
        }),
        "asset.catalog should map native asset metadata to package VFS URI: {catalog:#?}"
    );
    fs::remove_dir_all(work_root.join("nativevn").join("scenarios")).unwrap();

    for platform in ["windows", "web"] {
        let bundle = case_dir.join(format!("internal-bundle-{platform}"));
        let manifest = bundle_tsuinosora_demo_package(
            &work_root.join("nativevn"),
            &package,
            target,
            profile,
            platform,
            &bundle,
        );
        assert_eq!(
            manifest["package_hash"],
            Hash256::from_sha256(&package_bytes).to_string()
        );
        assert!(bundle.join("package").join("nativevn.astrapkg").exists());
        assert!(!serde_json::to_string(&manifest)
            .unwrap()
            .replace('\\', "/")
            .contains(&case_dir.to_string_lossy().replace('\\', "/")));
    }

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
fn nativevn_sample_cooks_packages_validates_and_runs_full_playthrough() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "nativevn-full-playthrough");
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("nativevn.astrapkg");

    fs::create_dir_all(&case_dir).unwrap();

    let cook_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            "Examples/NativeVN/project.yaml",
            "--profile",
            "classic",
            "--target",
            "nativevn-game",
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
    let first_manifest_bytes = fs::read(cooked.join("cook_manifest.yaml")).unwrap();
    let first_manifest: serde_yaml::Value = serde_yaml::from_slice(&first_manifest_bytes).unwrap();
    assert_eq!(first_manifest["schema"], "astra.cook_manifest.v2");
    assert_eq!(
        first_manifest["asset_cook"]["schema"],
        "astra.cook_batch_summary.v1"
    );
    let artifact_count = first_manifest["asset_cook"]["artifact_count"]
        .as_u64()
        .unwrap();
    assert!(artifact_count > 0);

    fs::write(cooked.join("stale.partial"), b"must be replaced").unwrap();
    let recook_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            "Examples/NativeVN/project.yaml",
            "--profile",
            "classic",
            "--target",
            "nativevn-game",
            "--out",
            cooked.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        recook_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&recook_output.stderr)
    );
    assert!(!cooked.join("stale.partial").exists());
    let recooked_manifest_bytes = fs::read(cooked.join("cook_manifest.yaml")).unwrap();
    let recooked_manifest: serde_yaml::Value =
        serde_yaml::from_slice(&recooked_manifest_bytes).unwrap();
    assert_eq!(
        recooked_manifest["asset_cook"]["cache_hit_count"],
        artifact_count
    );
    assert_eq!(recooked_manifest["asset_cook"]["cooked_count"], 0);

    let failed_recook = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            "Examples/NativeVN/project.yaml",
            "--profile",
            "unsupported-profile",
            "--target",
            "nativevn-game",
            "--out",
            cooked.to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!failed_recook.status.success());
    assert_eq!(
        fs::read(cooked.join("cook_manifest.yaml")).unwrap(),
        recooked_manifest_bytes,
        "failed cook must preserve the previous complete output"
    );

    let package_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "nativevn-game",
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
    let reader = PackageReader::open(&bytes).unwrap();
    for section in [
        "compiled.project",
        "player.display_config",
        "vn.story",
        "vn.profile_manifest",
        "vn.standard_command_manifest",
        "vn.presentation_provider_manifest",
        "vn.commercial_baseline_manifest",
        "vn.system_story_manifest",
        "media.font_manifest",
        "player.locale_config",
        "vn.localization.en",
    ] {
        assert!(reader.has_section(section), "missing section {section}");
    }
    let display_config: serde_json::Value = serde_json::from_slice(
        &reader
            .container()
            .read_section("player.display_config")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(display_config["schema"], "astra.player_display_config.v1");
    assert_eq!(display_config["original_resolution"]["width"], 1920);
    assert_eq!(display_config["original_resolution"]["height"], 1080);
    assert_eq!(display_config["scale_filter"], "linear");
    let font_manifest: serde_json::Value = serde_json::from_slice(
        &reader
            .container()
            .read_section("media.font_manifest")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(font_manifest["schema"], "astra.font_manifest.v1");
    assert_eq!(font_manifest["target"], "nativevn-game");
    assert_eq!(font_manifest["profile"], "classic");
    assert_eq!(font_manifest["provider_binding"], "astra.vfs.package");
    assert_eq!(font_manifest["fonts"][0]["asset_id"], "asset:/font/ui");
    assert_eq!(font_manifest["fonts"][0]["family"], "Poppins");
    assert_eq!(font_manifest["fonts"][0]["coverage"][0]["start"], 32);
    let vfs_manifest: serde_json::Value = serde_json::from_slice(
        &reader
            .container()
            .read_section("asset.vfs_manifest")
            .unwrap(),
    )
    .unwrap();
    let font_uri = font_manifest["fonts"][0]["uri"].as_str().unwrap();
    let font_vfs_entry = vfs_manifest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["vfs_uri"] == font_uri)
        .expect("font manifest URI must resolve to a package VFS entry");
    assert_eq!(font_vfs_entry["media_kind"], "font");
    assert_eq!(font_vfs_entry["hash"], font_manifest["fonts"][0]["hash"]);
    let media_manifest: serde_json::Value =
        serde_json::from_slice(&reader.container().read_section("media.manifest").unwrap())
            .unwrap();
    assert_eq!(media_manifest["font_manifest_required"], true);
    assert_eq!(
        media_manifest["font_manifest_section"],
        "media.font_manifest"
    );
    let locale_config: serde_json::Value = serde_json::from_slice(
        &reader
            .container()
            .read_section("player.locale_config")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(locale_config["schema"], "astra.player_locale_config.v1");
    assert_eq!(locale_config["default_locale"], "en");
    assert_eq!(locale_config["available_locales"][0], "en");
    assert_eq!(locale_config["available_locales"][1], "ja");
    assert_eq!(locale_config["available_locales"][2], "zh-Hans");

    let validate_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "validate",
            package.to_str().unwrap(),
            "--profile",
            "classic",
            "--target",
            "nativevn-game",
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        validate_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&validate_output.stderr)
    );
    let release_report: serde_json::Value =
        serde_json::from_slice(&validate_output.stdout).unwrap();
    assert!(release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["id"] == "vn.commercial_baseline" && check["status"] == "pass" }));
    assert!(!release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["status"] == "blocked" }));

    let headless_report = run_nativevn_headless(&package, root, &case_dir, "classic", "Digit1");
    assert_eq!(headless_report["schema"], "astra.headless_run_report.v2");
    assert_eq!(headless_report["status"], "passed");
    assert!(headless_report["rasterized_frame_count"].as_u64().unwrap() > 0);
    assert!(headless_report["audio_frame_count"].as_u64().unwrap() > 0);
    assert_eq!(headless_report["checkpoint_results"][0]["id"], "final");
    assert_eq!(headless_report["checkpoint_results"][0]["passed"], true);

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
#[ignore = "superseded by Tools/run_platform_host_acceptance.py real-host evidence"]
fn nativevn_sample_builds_windows_and_web_bundles_and_runs_player_routes() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "nativevn-player-bundles");
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("nativevn.astrapkg");

    fs::create_dir_all(&case_dir).unwrap();

    let cook_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            "Examples/NativeVN/project.yaml",
            "--profile",
            "classic",
            "--target",
            "nativevn-game",
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
            "nativevn-game",
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
    let package_bytes = fs::read(&package).unwrap();
    let package_reader = PackageReader::open(&package_bytes).unwrap();
    let display_config: serde_json::Value = serde_json::from_slice(
        &package_reader
            .container()
            .read_section("player.display_config")
            .unwrap(),
    )
    .unwrap();

    for (platform, entrypoint) in [("windows", "AstraPlayer.exe"), ("web", "index.html")] {
        let bundle = case_dir.join(format!("bundle-{platform}"));
        let bundle_output = Command::new(env!("CARGO_BIN_EXE_astra"))
            .args([
                "package",
                "bundle",
                package.to_str().unwrap(),
                "--target",
                "nativevn-game",
                "--profile",
                "classic",
                "--platform",
                platform,
                "--out",
                bundle.to_str().unwrap(),
                "--format",
                "json",
            ])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            bundle_output.status.success(),
            "platform={platform} stderr={}",
            String::from_utf8_lossy(&bundle_output.stderr)
        );
        let manifest: serde_json::Value = serde_json::from_slice(&bundle_output.stdout).unwrap();
        assert_eq!(manifest["schema"], "astra.standalone_bundle_manifest.v2");
        assert_eq!(manifest["target"], "nativevn-game");
        assert_eq!(manifest["profile"], "classic");
        assert_eq!(manifest["platform"], platform);
        assert_eq!(manifest["entrypoint"], entrypoint);
        assert!(manifest["package_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        assert!(bundle.join("bundle_manifest.json").exists());
        assert!(bundle.join("package").join("nativevn.astrapkg").exists());
        assert!(bundle.join(entrypoint).exists());
        assert!(bundle.join("AstraPlayer.config.json").exists());
        let player_config: serde_json::Value =
            serde_json::from_slice(&fs::read(bundle.join("AstraPlayer.config.json")).unwrap())
                .unwrap();
        assert_eq!(player_config["display"], display_config);
        assert_eq!(player_config["schema"], "astra.player_config.v2");
        assert_eq!(player_config["locale"], "en");
        assert_eq!(
            manifest["observability"]["log_schema"],
            "astra.log_event.v1"
        );
        if platform == "windows" {
            assert_eq!(manifest["observability"]["crash_reporting"], "required");
            assert!(bundle.join("AstraCrashReporter.exe").exists());
            assert_eq!(player_config["observability"]["log_dir"], "Saved/Logs");
            assert_eq!(player_config["observability"]["crash_dir"], "Saved/Crashes");
        } else {
            assert_eq!(manifest["observability"]["crash_reporting"], "disabled");
        }
        assert!(bundle
            .join("scenarios")
            .join("full_playthrough.yaml")
            .exists());
        if platform == "windows" {
            let launch_output = Command::new(bundle.join(entrypoint))
                .arg("--launch-report")
                .current_dir(&bundle)
                .output()
                .unwrap();
            assert!(
                launch_output.status.success(),
                "stderr={}",
                String::from_utf8_lossy(&launch_output.stderr)
            );
            let launch_report: serde_json::Value =
                serde_json::from_slice(&launch_output.stdout).unwrap();
            assert_eq!(launch_report["schema"], "astra.player_launch_report.v1");
            assert_eq!(launch_report["status"], "ready");
            assert_eq!(launch_report["target"], "nativevn-game");
            assert_eq!(launch_report["profile"], "classic");
            assert_eq!(launch_report["platform"], "windows");
            assert_eq!(launch_report["package_hash"], manifest["package_hash"]);

            let route_output = Command::new(bundle.join(entrypoint))
                .args([
                    "--route-scenario",
                    "scenarios/full_playthrough.yaml",
                    "--format",
                    "json",
                ])
                .current_dir(&bundle)
                .output()
                .unwrap();
            assert!(
                route_output.status.success(),
                "stderr={}",
                String::from_utf8_lossy(&route_output.stderr)
            );
            let route_report: serde_json::Value =
                serde_json::from_slice(&route_output.stdout).unwrap();
            assert_eq!(route_report["schema"], "astra.player_route_report.v1");
            assert_eq!(route_report["status"], "pass");
            assert_eq!(route_report["target"], "nativevn-game");
            assert_eq!(route_report["profile"], "classic");
            assert_eq!(route_report["platform"], "windows");
            assert_eq!(route_report["input_surface"], "windows_player");
            assert_eq!(route_report["entrypoint"], "AstraPlayer.exe");
            assert_eq!(route_report["package_hash"], manifest["package_hash"]);
            assert_eq!(route_report["scenario"], "scenarios/full_playthrough.yaml");
            assert_eq!(
                route_report["scenario_report"]["schema"],
                "astra.scenario_report.v1"
            );
            assert_eq!(route_report["scenario_report"]["status"], "pass");
            assert!(route_report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| { check["id"] == "player.route.full" && check["status"] == "pass" }));
            assert!(!String::from_utf8_lossy(&route_output.stdout).contains(root.to_str().unwrap()));

            let config_path = bundle.join("AstraPlayer.config.json");
            let original_config = fs::read(&config_path).unwrap();
            let mut legacy_config: serde_json::Value =
                serde_json::from_slice(&original_config).unwrap();
            legacy_config["schema"] =
                serde_json::Value::String("astra.player_config.v1".to_string());
            fs::write(
                &config_path,
                serde_json::to_vec_pretty(&legacy_config).unwrap(),
            )
            .unwrap();
            let legacy = Command::new(bundle.join(entrypoint))
                .arg("--launch-report")
                .current_dir(&bundle)
                .output()
                .unwrap();
            assert!(!legacy.status.success());
            assert!(String::from_utf8_lossy(&legacy.stderr)
                .contains("unsupported player config schema; rebuild the bundle"));
            fs::write(&config_path, original_config).unwrap();

            fs::write(bundle.join("AstraCrashReporter.exe"), b"tampered").unwrap();
            let tampered = Command::new(bundle.join(entrypoint))
                .arg("--launch-report")
                .current_dir(&bundle)
                .output()
                .unwrap();
            assert!(!tampered.status.success());
            assert!(String::from_utf8_lossy(&tampered.stderr)
                .contains("crash reporter hash or byte size mismatch"));
            assert!(!String::from_utf8_lossy(&tampered.stderr).contains(root.to_str().unwrap()));
        } else {
            assert!(bundle.join("AstraPlayer.route_model.json").exists());
            assert!(bundle
                .join("scenarios")
                .join("full_playthrough.yaml.json")
                .exists());
            let route_report =
                run_web_bundle_route_in_browser(&bundle, "scenarios/full_playthrough.yaml");
            assert_eq!(route_report["schema"], "astra.player_route_report.v1");
            assert_eq!(route_report["status"], "pass");
            assert_eq!(route_report["target"], "nativevn-game");
            assert_eq!(route_report["profile"], "classic");
            assert_eq!(route_report["platform"], "web");
            assert_eq!(route_report["input_surface"], "web_player");
            assert_eq!(route_report["entrypoint"], "index.html");
            assert_eq!(route_report["package_hash"], manifest["package_hash"]);
            assert_eq!(route_report["scenario"], "scenarios/full_playthrough.yaml");
            assert_eq!(
                route_report["scenario_report"]["schema"],
                "astra.scenario_report.v1"
            );
            assert_eq!(route_report["scenario_report"]["status"], "pass");
            assert!(route_report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| {
                    check["id"] == "player.web.dom_report" && check["status"] == "pass"
                }));
            assert!(route_report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| { check["id"] == "player.route.full" && check["status"] == "pass" }));
        }

        let run_output = Command::new(env!("CARGO_BIN_EXE_astra"))
            .args([
                "test",
                "run",
                "scenarios/full_playthrough.yaml",
                "--headless",
                "--target",
                "nativevn-game",
                "--platform",
                platform,
                "--package",
                bundle
                    .join("package")
                    .join("nativevn.astrapkg")
                    .to_str()
                    .unwrap(),
                "--format",
                "json",
            ])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            run_output.status.success(),
            "platform={platform} stderr={}",
            String::from_utf8_lossy(&run_output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&run_output.stdout).unwrap();
        assert_eq!(report["status"], "pass");
        assert_eq!(report["platform"], platform);
        assert!(report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| { check["id"] == "player_route.full" && check["status"] == "pass" }));
    }

    let _ = fs::remove_dir_all(case_dir);
}

#[astra_headless_test::test]
fn nativevn_sample_runs_opt_in_advanced_presentation_gate() {
    let root = workspace_root();
    let case_dir = unique_case_dir(root, "advanced-vn-presentation");
    let cooked = case_dir.join("cooked");
    let package = case_dir.join("nativevn-advanced.astrapkg");

    fs::create_dir_all(&case_dir).unwrap();

    let cook_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "cook",
            "Examples/NativeVN/project.yaml",
            "--profile",
            "advanced-vn",
            "--target",
            "nativevn-game",
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
            "nativevn-game",
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

    let validate_output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "package",
            "validate",
            package.to_str().unwrap(),
            "--profile",
            "advanced-vn",
            "--target",
            "nativevn-game",
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        validate_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&validate_output.stderr)
    );
    let release_report: serde_json::Value =
        serde_json::from_slice(&validate_output.stdout).unwrap();
    assert!(!release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["status"] == "blocked" }));
    assert!(release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["id"] == "vn.advanced_presentation" && check["status"] == "pass" }));

    #[cfg(feature = "ffmpeg-vcpkg")]
    {
        let headless_report =
            run_nativevn_headless(&package, root, &case_dir, "advanced-vn", "Digit2");
        assert_eq!(headless_report["schema"], "astra.headless_run_report.v2");
        assert_eq!(headless_report["status"], "passed");
        assert!(headless_report["rasterized_frame_count"].as_u64().unwrap() > 0);
        assert!(headless_report["audio_frame_count"].as_u64().unwrap() > 0);
        assert_eq!(headless_report["checkpoint_results"][0]["id"], "final");
        assert_eq!(headless_report["checkpoint_results"][0]["passed"], true);
    }

    let _ = fs::remove_dir_all(case_dir);
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
}

fn run_web_bundle_route_in_browser(bundle: &Path, route: &str) -> serde_json::Value {
    static WEB_BROWSER_GATE: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = WEB_BROWSER_GATE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap();
    let browser = find_browser_for_web_gate();
    let stop = Arc::new(AtomicBool::new(false));
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server_root = bundle.to_path_buf();
    let server_stop = Arc::clone(&stop);
    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => serve_bundle_request(stream, &server_root),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let profile = bundle.join(".browser-profile");
    let url = format!(
        "http://{}/index.html?route={}",
        addr,
        route.replace('\\', "/")
    );
    let mut dom = None;
    for flag in ["--headless=new", "--headless"] {
        for _ in 0..2 {
            let Some(output) = run_browser_dump_dom(&browser, &profile, &url, flag) else {
                continue;
            };
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            if text.contains("astra-route-report") {
                dom = Some(text);
                break;
            }
        }
        if dom.is_some() {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = TcpStream::connect(addr);
    let _ = server.join();
    let dom =
        dom.expect("Stage 3 Web player gate requires a working headless Chrome or Edge browser");
    extract_route_report_from_dom(&dom)
}

fn run_browser_dump_dom(
    browser: &Path,
    profile: &Path,
    url: &str,
    headless_flag: &str,
) -> Option<std::process::Output> {
    let output = Command::new(browser)
        .args([
            headless_flag,
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-extensions",
            "--virtual-time-budget=60000",
            "--dump-dom",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(url)
        .output()
        .ok()?;
    if output.status.success() {
        Some(output)
    } else {
        None
    }
}

fn serve_bundle_request(mut stream: TcpStream, root: &Path) {
    let mut request = [0u8; 4096];
    let Ok(read) = stream.read(&mut request) else {
        return;
    };
    let text = String::from_utf8_lossy(&request[..read]);
    let Some(line) = text.lines().next() else {
        return;
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or("/");
    if method != "GET" {
        write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        );
        return;
    }
    let path = target.split('?').next().unwrap_or("/");
    let relative = if path == "/" {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(path.trim_start_matches('/'))
    };
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        write_response(&mut stream, "400 Bad Request", "text/plain", b"bad path");
        return;
    }
    let file = root.join(relative);
    match fs::read(&file) {
        Ok(bytes) => write_response(&mut stream, "200 OK", content_type(&file), &bytes),
        Err(_) => write_response(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn write_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("yaml") | Some("yml") => "text/yaml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn extract_route_report_from_dom(dom: &str) -> serde_json::Value {
    let marker = "id=\"astra-route-report\"";
    let marker_pos = dom
        .find(marker)
        .or_else(|| dom.find("id='astra-route-report'"))
        .expect("browser DOM did not contain astra route report");
    let start = dom[marker_pos..]
        .find('>')
        .map(|index| marker_pos + index + 1)
        .unwrap();
    let end = dom[start..]
        .find("</script>")
        .map(|index| start + index)
        .unwrap();
    serde_json::from_str(dom[start..end].trim()).unwrap()
}

fn find_browser_for_web_gate() -> PathBuf {
    if let Ok(path) = env::var("ASTRA_WEB_BROWSER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    for name in [
        "chrome",
        "google-chrome",
        "chromium",
        "msedge",
        "chrome.exe",
        "msedge.exe",
    ] {
        if let Some(path) = find_on_path(name) {
            return path;
        }
    }
    #[cfg(windows)]
    {
        for name in ["chrome.exe", "msedge.exe"] {
            if let Some(path) = find_windows_app_path(name) {
                return path;
            }
        }
    }
    panic!("Stage 3 Web player gate requires ASTRA_WEB_BROWSER or Chrome/Edge on PATH/App Paths");
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        if !name.ends_with(".exe") {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn find_windows_app_path(name: &str) -> Option<PathBuf> {
    for hive in ["HKCU", "HKLM"] {
        let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{name}");
        let output = Command::new("reg")
            .args(["query", &key, "/ve"])
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some((_, value)) = line.split_once("REG_SZ") {
                let path = PathBuf::from(value.trim());
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn write_tsuinosora_demo_slice_fixture(root: &Path, case_dir: &Path) -> (PathBuf, PathBuf) {
    let original = case_dir.join("original");
    let unpacked = case_dir.join("unpacked");
    let work = case_dir.join("work");
    fs::create_dir_all(original.join("DATA")).unwrap();
    fs::create_dir_all(&unpacked).unwrap();
    fs::write(original.join("READY.dxr"), b"synthetic director container").unwrap();
    fs::write(
        original.join("DATA").join("SCENE.dxr"),
        b"synthetic scene container",
    )
    .unwrap();
    fs::write(unpacked.join("bg.png"), tiny_rgba_png()).unwrap();
    fs::write(
        unpacked.join("cast_map.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "tsuinosora.cast_map.v1",
            "members": [{
                "member_id": "cast.bg.title",
                "kind": "background",
                "source": "bg.png",
                "container_entry_id": "ready.0001",
                "route_ids": ["classic.main"],
                "command_ids": ["cmd.bg.title"]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        unpacked.join("route_graph.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "tsuinosora.route_graph.v1",
            "routes": [{
                "route_id": "classic.main",
                "terminal": "ending.good",
                "choices": ["choice.start"],
                "coverage": "covered"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        case_dir.join("demo.config.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "tsuinosora.demo_slice_config.v1",
            "original_install_root": original,
            "local_work_root": work,
            "unpacked_root": unpacked,
            "title_png": root.join("Examples/TsuiNoSora/Docs/Title.png"),
            "game_png": root.join("Examples/TsuiNoSora/Docs/Game.png"),
            "modern_features": [{
                "feature_id": "remake_overlay.hero",
                "feature_kind": "portrait_overlay",
                "input_hash": "sha256:input",
                "output_hash": "sha256:output",
                "fallback_hash": "sha256:fallback",
                "independent_switch": true,
                "affects_core_state": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    (work.join("nativevn").join("project.yaml"), work)
}

fn run_tsuinosora_demo_slice_tool(root: &Path, config: &Path) -> serde_json::Value {
    let mut last_error = String::new();
    let mut candidates = Vec::new();
    if let Ok(python) = env::var("PYTHON") {
        candidates.push(python);
    }
    candidates.extend(["python".to_string(), "python3".to_string()]);
    for python in candidates {
        let output = Command::new(&python)
            .args([
                "Tools/TsuiNoSora/tsuinosora_tools.py",
                "demo-slice",
                "--config",
                config.to_str().unwrap(),
            ])
            .current_dir(root)
            .output();
        let Ok(output) = output else {
            continue;
        };
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout
                    .replace('\\', "/")
                    .contains(&config.to_string_lossy().replace('\\', "/")),
                "demo-slice report leaked private config path"
            );
            return serde_json::from_slice(&output.stdout).unwrap();
        }
        last_error = String::from_utf8_lossy(&output.stderr).into_owned();
    }
    panic!("failed to run TsuiNoSora demo-slice tool: {last_error}");
}

fn assert_tsuinosora_demo_package_validates(
    root: &Path,
    package: &Path,
    target: &str,
    profile: &str,
) {
    let validate_output = run_astra(
        root,
        [
            "package",
            "validate",
            package.to_str().unwrap(),
            "--profile",
            profile,
            "--target",
            target,
            "--format",
            "json",
        ],
    );
    let release_report: serde_json::Value =
        serde_json::from_slice(&validate_output.stdout).unwrap();
    assert!(!release_report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["status"] == "blocked"));
    for id in [
        "tsuinosora.reference_evidence",
        "tsuinosora.asset_analysis",
        "tsuinosora.conversion_manifest",
        "tsuinosora.mount_policy",
    ] {
        assert!(
            release_report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["id"] == id && check["status"] == "pass"),
            "missing release pass check {id} for {target}/{profile}"
        );
    }
}

fn assert_tsuinosora_demo_headless_route(
    root: &Path,
    work_root: &Path,
    package: &Path,
    target: &str,
    profile: &str,
    platform: &str,
) {
    let scenario = work_root
        .join("nativevn")
        .join(tsuinosora_demo_scenario_ref(target, profile, platform));
    let output = run_astra(
        root,
        [
            "test",
            "run",
            scenario.to_str().unwrap(),
            "--headless",
            "--target",
            target,
            "--profile",
            profile,
            "--platform",
            platform,
            "--package",
            package.to_str().unwrap(),
            "--format",
            "json",
        ],
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "pass");
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["id"] == "player_route.full" && check["status"] == "pass" }));
}

fn bundle_tsuinosora_demo_package(
    cwd: &Path,
    package: &Path,
    target: &str,
    profile: &str,
    platform: &str,
    bundle: &Path,
) -> serde_json::Value {
    let output = run_astra_in(
        cwd,
        [
            "package",
            "bundle",
            package.to_str().unwrap(),
            "--target",
            target,
            "--profile",
            profile,
            "--platform",
            platform,
            "--out",
            bundle.to_str().unwrap(),
            "--format",
            "json",
        ],
    );
    let manifest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest["schema"], "astra.standalone_bundle_manifest.v2");
    assert_eq!(manifest["target"], target);
    assert_eq!(manifest["profile"], profile);
    assert_eq!(manifest["platform"], platform);
    assert_eq!(manifest["mount_policy"], "AstraPlayer.mount_policy.json");
    manifest
}

fn tsuinosora_demo_scenario_ref(target: &str, profile: &str, platform: &str) -> String {
    format!("scenarios/{target}.{profile}.{platform}.classic_main.json")
}

fn tiny_rgba_png() -> &'static [u8] {
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 6,
        0, 0, 0, 196, 15, 190, 139, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99, 208, 8, 168, 248,
        143, 15, 51, 140, 12, 5, 0, 126, 25, 123, 193, 234, 229, 222, 200, 0, 0, 0, 0, 73, 69, 78,
        68, 174, 66, 96, 130,
    ]
}

fn write_tsuinosora_package_sections(root: &Path) {
    let reports = [
        (
            "reference_evidence.json",
            serde_json::json!({
                "schema": "tsuinosora.visual_reference_report.v1",
                "status": "pass",
                "references": [
                    {
                        "logical_id": "title",
                        "dimensions": {"width": 1386, "height": 1040},
                        "hash": "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
                        "allowed_regions": ["title_background", "title_menu_buttons"]
                    },
                    {
                        "logical_id": "game",
                        "dimensions": {"width": 1403, "height": 1053},
                        "hash": "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84",
                        "allowed_regions": ["background_viewport", "text_window"]
                    }
                ],
                "prohibited_outputs": ["new_commercial_screenshot", "commercial_audio"]
            }),
        ),
        (
            "asset_analysis.json",
            serde_json::json!({
                "schema": "tsuinosora.asset_analysis.v1",
                "status": "pass",
                "reference_hashes": [
                    "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
                    "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84"
                ],
                "assets": [{
                    "relative_path": "native-assets/bg/opening.png",
                    "classification": "background",
                    "confidence": 0.91,
                    "sha256": "sha256:bg"
                }],
                "quarantine": [],
                "diagnostics": []
            }),
        ),
        (
            "conversion_report.json",
            serde_json::json!({
                "schema": "tsuinosora.conversion_report.v1",
                "status": "pass",
                "inputs": {"original_install_root": "original_install_root"},
                "counts": {
                    "source_files": 2,
                    "asset_count": 1,
                    "quarantine_count": 0,
                    "route_count": 1
                },
                "resources": [{
                    "source": "containers/ready/0001_png.png",
                    "native_path": "native-assets/backgrounds/0001_png.png",
                    "classification": "background",
                    "source_hash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                    "converted_hash": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
                    "byte_size": 68
                }],
                "routes": [{
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good"
                }],
                "diagnostics": [],
                "redaction": {"paths": "alias_only", "payload": "omitted"}
            }),
        ),
        (
            "mount_policy.internal.json",
            serde_json::json!({
                "schema": "tsuinosora.mount_policy.v1",
                "target": "tsuinosora-internal-game",
                "status": "pass",
                "aliases": [
                    {
                        "alias": "original",
                        "value": "original_install_root",
                        "hash_policy": "manifest_required",
                        "fallback": "blocking"
                    },
                    {
                        "alias": "remake",
                        "value": "remake_install_root.optional",
                        "hash_policy": "manifest_required",
                        "fallback": "blocking"
                    }
                ],
                "diagnostics": []
            }),
        ),
        (
            "mount_policy.patch.json",
            serde_json::json!({
                "schema": "tsuinosora.mount_policy.v1",
                "target": "tsuinosora-patch-game",
                "status": "pass",
                "aliases": [
                    {
                        "alias": "original",
                        "value": "original_install_root",
                        "hash_policy": "manifest_required",
                        "fallback": "blocking"
                    },
                    {
                        "alias": "remake",
                        "value": "remake_install_root.optional",
                        "hash_policy": "manifest_required",
                        "fallback": "blocking"
                    }
                ],
                "diagnostics": []
            }),
        ),
        (
            "modern_profile_report.json",
            serde_json::json!({
                "schema": "tsuinosora.modern_profile_report.v1",
                "status": "pass",
                "base_conversion_status": "pass",
                "counts": {"feature_count": 1, "route_count": 1},
                "features": [{
                    "feature_id": "remake_overlay.hero",
                    "feature_kind": "portrait_overlay",
                    "input_hash": "sha256:input",
                    "output_hash": "sha256:output",
                    "fallback_hash": "sha256:fallback",
                    "independent_switch": true,
                    "affects_core_state": false
                }],
                "diagnostics": [],
                "redaction": {"paths": "alias_or_hash_only", "payload": "omitted"}
            }),
        ),
    ];
    for (name, value) in reports {
        fs::write(root.join(name), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
}

fn write_tsuinosora_synthetic_project(project: &Path) {
    fs::write(
        project,
        r#"
schema: astra.project.v1
id: com.example.tsuinosora.stage3
targets:
  - id: tsuinosora-internal-game
    kind: game
    crate: astra-vn
    default_profile: classic
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [headless, windows, web]
    packaged: true
  - id: tsuinosora-patch-game
    kind: game
    crate: astra-vn
    default_profile: classic
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [headless, windows, web]
    packaged: true
nativevn:
  sources:
    - Scripts
  profiles: [classic, modern]
  display:
    original_resolution:
      width: 800
      height: 600
    scale_filter: linear
  scenario_refs:
    - scenarios/tsuinosora-internal-game.classic.headless.yaml
    - scenarios/tsuinosora-internal-game.classic.windows.yaml
    - scenarios/tsuinosora-internal-game.classic.web.yaml
    - scenarios/tsuinosora-internal-game.modern.headless.yaml
    - scenarios/tsuinosora-internal-game.modern.windows.yaml
    - scenarios/tsuinosora-internal-game.modern.web.yaml
    - scenarios/tsuinosora-patch-game.classic.headless.yaml
    - scenarios/tsuinosora-patch-game.classic.windows.yaml
    - scenarios/tsuinosora-patch-game.classic.web.yaml
    - scenarios/tsuinosora-patch-game.modern.headless.yaml
    - scenarios/tsuinosora-patch-game.modern.windows.yaml
    - scenarios/tsuinosora-patch-game.modern.web.yaml
package_sections:
  - id: tsuinosora.reference_evidence
    schema: tsuinosora.visual_reference_report.v1
    path: PackageSections/reference_evidence.json
    codec: raw
  - id: tsuinosora.asset_analysis
    schema: tsuinosora.asset_analysis.v1
    path: PackageSections/asset_analysis.json
    codec: raw
  - id: tsuinosora.conversion_manifest
    schema: tsuinosora.conversion_report.v1
    path: PackageSections/conversion_report.json
    codec: raw
  - id: tsuinosora.mount_policy
    schema: tsuinosora.mount_policy.v1
    path: PackageSections/mount_policy.internal.json
    codec: raw
    targets: [tsuinosora-internal-game]
  - id: tsuinosora.mount_policy
    schema: tsuinosora.mount_policy.v1
    path: PackageSections/mount_policy.patch.json
    codec: raw
    targets: [tsuinosora-patch-game]
  - id: tsuinosora.modern_profile_report
    schema: tsuinosora.modern_profile_report.v1
    path: PackageSections/modern_profile_report.json
    codec: raw
    profiles: [modern]
"#,
    )
    .unwrap();
}

fn write_tsuinosora_synthetic_scenarios(root: &Path) {
    let scenarios = root.join("scenarios");
    fs::create_dir_all(&scenarios).unwrap();
    for target in ["tsuinosora-internal-game", "tsuinosora-patch-game"] {
        for profile in ["classic", "modern"] {
            let platforms: &[&str] = &["headless", "windows", "web"];
            for &platform in platforms {
                let path = root.join(tsuinosora_scenario_ref(target, profile, platform));
                fs::write(path, tsuinosora_scenario_yaml(target, profile, platform)).unwrap();
            }
        }
    }
}

fn tsuinosora_scenario_ref(target: &str, profile: &str, platform: &str) -> String {
    format!("scenarios/{target}.{profile}.{platform}.yaml")
}

fn tsuinosora_scenario_yaml(target: &str, profile: &str, platform: &str) -> String {
    format!(
        r#"
schema: astra.scenario.v1
stage: stage3-astra-vn
target: {target}
profile: {profile}
platform: {platform}
generated_route_id: route.library
seed: 42
locale: zh-Hans
mount_aliases:
  original: original_install_root
  remake: remake_install_root.optional
actions:
  - launch: {{}}
  - player_input:
      kind: complete_wait
      value: movie.opening.end
  - player_input:
      kind: complete_wait
      value: voice.opening.end
  - player_input:
      kind: advance
  - player_input:
      kind: choose
      value: choice.library
  - player_input:
      kind: complete_wait
      value: library.pause
  - player_input:
      kind: advance
  - player_input:
      kind: replay_voice
      value: voice.hero.0002
  - player_input:
      kind: open_system
      value: route_chart
  - player_input:
      kind: save
      slot: slot.full
  - player_input:
      kind: load
      slot: slot.full
  - replay_from_start: {{}}
assertions:
  - coverage:
      routes: [ending.good]
      backlog_keys: [prologue.hello, library.followup]
      read_state: [line.prologue.hello, line.library.followup]
      voice_replay: [voice.hero.0001, voice.hero.0002]
  - replay_hash_match: true
  - no_blocking_diagnostics: true
"#
    )
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

fn run_windows_bundle_route(bundle: &Path, scenario: &str) -> serde_json::Value {
    let route_output = Command::new(bundle.join("AstraPlayer.exe"))
        .args(["--route-scenario", scenario, "--format", "json"])
        .current_dir(bundle)
        .output()
        .unwrap();
    assert!(
        route_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&route_output.stderr)
    );
    serde_json::from_slice(&route_output.stdout).unwrap()
}

fn run_windows_bundle_route_with_mount(
    bundle: &Path,
    scenario: &str,
    alias: &str,
    root: &Path,
) -> serde_json::Value {
    let mount_arg = format!("{alias}={}", root.to_string_lossy());
    let route_output = Command::new(bundle.join("AstraPlayer.exe"))
        .args([
            "--route-scenario",
            scenario,
            "--format",
            "json",
            "--mount-root",
            &mount_arg,
        ])
        .current_dir(bundle)
        .output()
        .unwrap();
    assert!(
        route_output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&route_output.stderr)
    );
    serde_json::from_slice(&route_output.stdout).unwrap()
}

fn install_patch_mount_fixture(
    case_dir: &Path,
    bundle: &Path,
    scenario: &str,
    profile: &str,
) -> PathBuf {
    let mount_root = case_dir.join(format!("mount-original-{profile}"));
    let probe_bytes = br#"{"schema":"tsuinosora.mount_probe.v1","alias":"original"}"#;
    fs::create_dir_all(mount_root.join("probe")).unwrap();
    fs::write(mount_root.join("probe").join("manifest.json"), probe_bytes).unwrap();
    let probe_hash = Hash256::from_sha256(probe_bytes).to_string();
    let asset_bytes = br#"{"schema":"tsuinosora.mount_asset.v1","role":"background"}"#;
    fs::create_dir_all(mount_root.join("native-assets").join("backgrounds")).unwrap();
    fs::write(
        mount_root
            .join("native-assets")
            .join("backgrounds")
            .join("opening.png"),
        asset_bytes,
    )
    .unwrap();
    let asset_hash = Hash256::from_sha256(asset_bytes).to_string();
    fs::OpenOptions::new()
        .append(true)
        .open(bundle.join(scenario))
        .unwrap()
        .write_all(
            format!(
                "\nmount_probes:\n  - alias: original\n    path: probe/manifest.json\n    sha256: {probe_hash}\nmount_assets:\n  - alias: original\n    path: native-assets/backgrounds/opening.png\n    role: background\n    route_id: route.library\n    sha256: {asset_hash}\n"
            )
            .as_bytes(),
        )
        .unwrap();
    mount_root
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
