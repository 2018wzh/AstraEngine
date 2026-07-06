use astra_test::{ScenarioRunner, ScenarioStatus};
use std::path::Path;

#[test]
fn native_smoke_runs_headless_and_matches_replay_hash() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let report = ScenarioRunner::run_file(root.join("scenarios/native_smoke.yaml")).unwrap();
    assert_eq!(report.status, ScenarioStatus::Pass);
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
