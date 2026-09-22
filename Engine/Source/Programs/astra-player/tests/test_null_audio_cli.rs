#[test]
fn test_audio_is_rejected_for_automation_before_loading_files() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_astra-player"))
        .args(["--test-null-audio", "--script", "missing.json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a bundled Windows Player"));
}
