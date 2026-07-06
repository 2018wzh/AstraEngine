use astra_package::PackageReader;
use astra_target::{TargetKind, TargetManifest};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
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
        "astra.platform_capability_report.v1"
    );
    assert_eq!(platform_report["platform"], "windows");
    if cfg!(windows) {
        let smoke = platform_report["smoke"].as_array().unwrap();
        assert!(smoke
            .iter()
            .any(|check| { check["id"] == "windowed_smoke" && check["status"] == "pass" }));
        assert!(smoke
            .iter()
            .any(|check| check["id"] == "decode.wmf" && check["status"] == "pass"));
    } else {
        assert_eq!(platform_report["sdk_status"], "missing");
    }
}

#[test]
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
runtime: astra-vn
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

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
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
