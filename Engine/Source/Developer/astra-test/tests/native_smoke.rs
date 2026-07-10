use astra_test::{ScenarioRunOptions, ScenarioRunner, ScenarioStatus};
use std::path::Path;

#[test]
fn native_smoke_runs_headless_and_matches_replay_hash() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let report = ScenarioRunner::run_file(root.join("scenarios/native_smoke.yaml")).unwrap();
    assert_eq!(report.status, ScenarioStatus::Pass, "{report:#?}");
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "runtime.determinism" && check.status == ScenarioStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "plugin.ffi_action_provider"
            && check.status == ScenarioStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "runtime.delayed_event" && check.status == ScenarioStatus::Pass));
    assert!(report.hashes.state.to_hex().len() == 32);
}

#[test]
fn unsupported_vn_actions_block_instead_of_being_ignored() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let report = ScenarioRunner::run_file_with_options(
        root.join("Docs/samples/astra-vn-script/full_playthrough.yaml"),
        ScenarioRunOptions::default(),
    )
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Blocked);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_SCENARIO_ACTION_UNSUPPORTED"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "assert.unsupported_schema"
            && check.status == ScenarioStatus::Blocked));
}

#[test]
fn package_target_scenario_refs_are_required_when_declared() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scenario_path = root.join("target/astra-test/missing-package-scenario.yaml");
    if let Some(parent) = scenario_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(
        &scenario_path,
        r#"
schema: astra.scenario.v1
id: missing-package
stage: stage2-media-package
package: target/astra-test/missing.astrapkg
target: native-smoke-game
profile: desktop-release
locale: zh-Hans
seed: 1
actions:
  - launch: {}
assertions:
  - no_blocking_diagnostics: true
"#,
    )
    .unwrap();

    let report =
        ScenarioRunner::run_file_with_options(&scenario_path, ScenarioRunOptions::default())
            .unwrap();

    assert_eq!(report.status, ScenarioStatus::Blocked);
    assert_eq!(
        report.package.as_deref(),
        Some("target/astra-test/missing.astrapkg")
    );
    assert_eq!(report.target.as_deref(), Some("native-smoke-game"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_SCENARIO_PACKAGE_MISSING"));
}
