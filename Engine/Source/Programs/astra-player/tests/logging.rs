use std::{fs, process::Command};

#[astra_headless_test::test]
fn player_host_uses_shared_stable_logging_pipeline() {
    let logs = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_astra-player"))
        .args(["--log-filter", "trace", "--log-format", "json", "--log-dir"])
        .arg(logs.path())
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status={:?}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_to_string(logs.path().join("astra.jsonl")).unwrap();
    assert!(log.contains("\"schema\":\"astra.log_event.v1\""));
    assert!(log.contains("\"event\":\"player.host.start\""));
    assert!(log.contains("\"process_role\":\"player\""));
}
