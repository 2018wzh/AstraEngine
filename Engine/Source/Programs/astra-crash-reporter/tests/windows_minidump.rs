#![cfg(target_os = "windows")]

use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use astra_observability::{
    install_windows_crash_reporter, CrashReportingMode, WindowsCrashReporterConfig,
};

#[test]
fn reporter_writes_real_out_of_process_minidump_and_manifest() {
    let output = tempfile::tempdir().unwrap();
    let dump = output.path().join("astra.dmp");
    let manifest = output.path().join("manifest.json");
    let mut target = Command::new(env!("CARGO_BIN_EXE_AstraCrashReporter"))
        .arg("--fixture-target")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(150));

    let result = Command::new(env!("CARGO_BIN_EXE_AstraCrashReporter"))
        .args([
            "--write-dump",
            "--pid",
            &target.id().to_string(),
            "--output",
        ])
        .arg(&dump)
        .arg("--manifest")
        .arg(&manifest)
        .args(["--session-id", "test-session"])
        .output()
        .unwrap();
    let _ = target.kill();
    let _ = target.wait();
    assert!(
        result.status.success(),
        "status={:?}\nstderr={}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    );

    let bytes = fs::read(&dump).unwrap();
    assert!(bytes.len() > 32);
    assert_eq!(&bytes[..4], b"MDMP");
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(report["schema"], "astra.crash_bundle.v1");
    assert_eq!(report["session_id"], "test-session");
    assert_eq!(report["process_role"], "crash_reporter");
    assert!(report["minidump"]["sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(report["minidump"]["byte_size"], bytes.len() as u64);
    assert!(!String::from_utf8_lossy(&fs::read(&manifest).unwrap())
        .contains(output.path().to_str().unwrap()));
}

#[test]
fn prelaunched_reporter_captures_panics_through_shared_request() {
    let output = tempfile::tempdir().unwrap();
    let guard = install_windows_crash_reporter(WindowsCrashReporterConfig {
        reporter_path: env!("CARGO_BIN_EXE_AstraCrashReporter").into(),
        crash_dir: output.path().to_path_buf(),
        log_file: None,
        session_id: "panic-session".to_string(),
        mode: CrashReportingMode::Required,
        handshake_timeout: Duration::from_secs(5),
        completion_timeout: Duration::from_secs(15),
    })
    .unwrap()
    .unwrap();
    let panic = std::panic::catch_unwind(|| panic!("private panic payload"));
    assert!(panic.is_err());
    drop(guard);

    let bundle = fs::read_dir(output.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("crash-"))
        .unwrap();
    let manifest = fs::read(bundle.path().join("manifest.json")).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(report["schema"], "astra.crash_bundle.v1");
    assert_eq!(report["reason_code"], "ASTRA_PANIC");
    assert!(!String::from_utf8_lossy(&manifest).contains("private panic payload"));
    assert_eq!(
        &fs::read(bundle.path().join("astra.dmp")).unwrap()[..4],
        b"MDMP"
    );
}

#[test]
fn prelaunched_reporter_captures_unhandled_seh_through_shared_request() {
    let output = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_AstraCrashReporter"))
        .arg("--seh-fixture")
        .arg("--crash-dir")
        .arg(output.path())
        .status()
        .unwrap();
    assert!(!status.success());

    let bundle = fs::read_dir(output.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("crash-"))
        .unwrap();
    let manifest = fs::read(bundle.path().join("manifest.json")).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(report["schema"], "astra.crash_bundle.v1");
    assert_eq!(report["reason_code"], "ASTRA_SEH");
    assert_eq!(
        &fs::read(bundle.path().join("astra.dmp")).unwrap()[..4],
        b"MDMP"
    );
}
