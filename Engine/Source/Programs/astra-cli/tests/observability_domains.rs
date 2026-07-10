use std::{fs, path::Path, process::Command};

#[test]
fn toolchain_domains_emit_structured_lifecycle_events() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let case = tempfile::tempdir().unwrap();
    let project = case.path().join("project.yaml");
    let cooked = case.path().join("cooked");
    let package = case.path().join("game.astrapkg");
    fs::write(
        &project,
        r#"schema: astra.project.v1
id: com.example.observability
targets:
  - id: sample-game
    kind: game
    crate: astra-vn
    default_profile: desktop-release
    runtime_provider: native_vn
    platforms: [windows]
    packaged: true
"#,
    )
    .unwrap();

    let cook_log = run(
        root,
        &case.path().join("cook-logs"),
        [
            "cook",
            project.to_str().unwrap(),
            "--profile",
            "desktop-release",
            "--target",
            "sample-game",
            "--out",
            cooked.to_str().unwrap(),
        ],
    );
    let package_log = run(
        root,
        &case.path().join("package-logs"),
        [
            "package",
            "build",
            cooked.to_str().unwrap(),
            "--target",
            "sample-game",
            "--out",
            package.to_str().unwrap(),
        ],
    );
    let target_log = run(
        root,
        &case.path().join("target-logs"),
        [
            "target",
            "validate",
            project.to_str().unwrap(),
            "--target",
            "sample-game",
            "--format",
            "json",
        ],
    );

    for (log, target, event) in [
        (&cook_log, "astra_cook", "cook.run.start"),
        (&package_log, "astra_package", "package.build.complete"),
        (&target_log, "astra_target", "target.validate.complete"),
    ] {
        assert!(
            log.contains(&format!("\"target\":\"{target}")),
            "{target}: {log}"
        );
        assert!(
            log.contains(&format!("\"event\":\"{event}\"")),
            "{event}: {log}"
        );
    }
}

fn run<const N: usize>(root: &Path, logs: &Path, args: [&str; N]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args(args)
        .args(["--log-filter", "trace", "--log-dir"])
        .arg(logs)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    read_all_logs(logs)
}

fn read_all_logs(logs: &Path) -> String {
    let mut output = String::new();
    for entry in fs::read_dir(logs).unwrap() {
        let entry = entry.unwrap();
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("astra.jsonl")
        {
            output.push_str(&fs::read_to_string(entry.path()).unwrap());
        }
    }
    output
}
