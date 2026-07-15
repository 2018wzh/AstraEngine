use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_ui_core::{
    UiAccessibilityReport, UiPoint, UiRect, UiSemanticAction, UiSemanticNode, UiSemanticRole,
    UiSemanticSnapshot, ValidateUi,
};

#[astra_headless_test::test]
fn accessibility_report_redacts_commercial_text_and_bounds() {
    let mut snapshot = UiSemanticSnapshot {
        schema: "astra.ui_semantic_snapshot.v1".into(),
        session_id: "session.report".into(),
        generation: 7,
        root_id: "root".into(),
        nodes: vec![UiSemanticNode {
            id: "root".into(),
            parent_id: None,
            role: UiSemanticRole::Button,
            bounds_points: UiRect {
                min: UiPoint { x: 123.0, y: 456.0 },
                max: UiPoint { x: 789.0, y: 900.0 },
            },
            name: Some("commercial dialogue must not enter evidence".into()),
            description: Some("private description".into()),
            value: Some("private value".into()),
            enabled: true,
            hidden: false,
            focused: true,
            selected: false,
            checked: None,
            actions: BTreeSet::from([UiSemanticAction::Activate]),
            properties: BTreeMap::new(),
        }],
        hash: Hash256::from_sha256(&[]),
    };
    snapshot.hash = snapshot.compute_hash().expect("snapshot hash");
    let report = UiAccessibilityReport::from_snapshot("windows.uia", "passed", &snapshot)
        .expect("redacted report");
    report.validate().expect("report validates");
    let json = serde_json::to_string(&report).expect("report json");
    assert!(!json.contains("commercial dialogue"));
    assert!(!json.contains("private description"));
    assert!(!json.contains("private value"));
    assert!(!json.contains("123"));
    assert_eq!(report.semantic_snapshot_hash, snapshot.hash);
}
