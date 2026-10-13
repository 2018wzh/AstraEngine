use super::*;

#[test]
pub(super) fn web_bundle_requires_wasm_bindgen_pair_and_embeds_canonical_host_scripts() {
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
    assert!(!roles.contains(&"web_ui_component_host"));
    assert!(!bundle.join("AstraPlayer.route_model.json").exists());
    assert!(!bundle.join("astra-player.js").exists());
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(bundle.join("AstraPlayer.config.json")).unwrap()).unwrap();
    assert!(config.get("ui_components").is_none());
    // A mixed-platform Cook remains valid; only the selected Web bundle rejects actions.
    let old_config = fs::read(bundle.join("AstraPlayer.config.json")).unwrap();
    fs::create_dir_all(case_dir.join("Policies")).unwrap();
    fs::write(
        case_dir.join("Policies/action.luau"),
        r#"
astra.command.register("native_action", {schema="demo.action.v1",fields={}}, function(args)
    return astra.action.complete()
end)
"#,
    )
    .unwrap();
    let mut project_config: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&project).unwrap()).unwrap();
    project_config["nativevn"]["policy_actions"] =
        serde_yaml::from_str("[{entry: Policies/action.luau}]").unwrap();
    fs::write(&project, serde_yaml::to_string(&project_config).unwrap()).unwrap();
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
    let rejected_bundle = case_dir.join("policy-bundle");
    for (destination, diagnostic) in [
        (&rejected_bundle, "ASTRA_WEB_POLICY_ACTION_UNSUPPORTED"),
        (&bundle, "ASTRA_BUNDLE_OUTPUT_EXISTS"),
    ] {
        let rejected = Command::new(env!("CARGO_BIN_EXE_astra"))
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
                destination.to_str().unwrap(),
                "--web-player-wasm",
                wasm.to_str().unwrap(),
                "--web-player-glue",
                glue.to_str().unwrap(),
            ])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }
    assert!(!rejected_bundle.exists());
    assert_eq!(
        fs::read(bundle.join("AstraPlayer.config.json")).unwrap(),
        old_config
    );
    fs::remove_dir_all(case_dir).unwrap();
}

#[test]
pub(super) fn package_build_writes_only_the_selected_game_target() {
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
    assert!(policy.get("runtime_provider").is_none());
    assert!(!bindings
        .iter()
        .any(|binding| binding["slot"] == "game_runtime_provider"));

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
