use std::{path::Path, process::Command};

#[astra_headless_test::test]
fn retired_headless_alias_returns_stable_migration_diagnostic() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_astra"))
        .args([
            "test",
            "run",
            "scenarios/native_smoke.yaml",
            "--headless",
            "--format",
            "json",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("ASTRA_TEST_HEADLESS_MIGRATED"), "{stderr}");
    assert!(!contains_local_absolute_path(&stderr), "{stderr}");
}

fn contains_local_absolute_path(text: &str) -> bool {
    text.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_alphabetic()
            && window[1] == b':'
            && (window[2] == b'\\' || window[2] == b'/')
    })
}
