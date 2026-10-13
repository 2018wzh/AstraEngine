use std::{fs, process::Command};

#[test]
fn explain_rejects_retired_and_unknown_scenario_shaped_reports() {
    let root = tempfile::tempdir().unwrap();
    let report_path = root.path().join("report.json");
    for schema in ["astra.scenario_report.v1", "unknown.report.v1"] {
        fs::write(
            &report_path,
            serde_json::to_vec(&serde_json::json!({
                "schema": schema,
                "stage": "obsolete.scenario",
                "status": "pass",
                "hashes": {
                    "state": "hash128:00000000000000000000000000000000",
                    "event": "hash128:00000000000000000000000000000000",
                    "presentation": "hash128:00000000000000000000000000000000"
                },
                "checks": [],
                "diagnostics": []
            }))
            .unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_astra"))
            .args(["report", "explain"])
            .arg(&report_path)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {schema}");
        assert!(output.stdout.is_empty(), "formatted {schema}");
    }
}
