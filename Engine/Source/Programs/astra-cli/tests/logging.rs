use std::{path::Path, process::Command};

#[test]
fn test_run_writes_report_to_stdout_and_json_logs_to_stderr() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let fixture_status = Command::new("cargo")
        .args(["build", "-p", "headless-presentation-provider"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(fixture_status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "test",
            "run",
            "scenarios/native_smoke.yaml",
            "--headless",
            "--format",
            "json",
            "--log-format",
            "json",
            "--log-filter",
            "astra_runtime=debug,astra_test=debug,astra_plugin=debug",
        ])
        .current_dir(root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "status={:?}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["schema"], "astra.scenario_report.v1");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("\"target\":\"astra_test::runner\""));
    assert!(stderr.contains("\"target\":\"astra_runtime::world\""));
    assert!(stderr.contains("\"target\":\"astra_plugin::loader\""));
    assert!(stderr.contains("runtime.tick"));
    assert!(stderr.contains("scenario.run"));
    assert!(stderr.contains("plugin.load"));
    assert!(!contains_local_absolute_path(&stderr), "{stderr}");
}

fn contains_local_absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic()
            && window[1] == b':'
            && (window[2] == b'\\' || window[2] == b'/')
    })
}
