use astra_vn_editor::{
    compile_astra_project, AstraSource, EditorVisualMetadata, GraphNodeMetadata,
    TimelineTrackMetadata,
};

const STORY: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.start speaker:narrator #@id line.start
    call common #@id call.common
    text key:line.after speaker:narrator #@id line.after
    jump ending.good #@id jump.good

state common #@id state.common
  scene common #@id scene.common
    text key:line.common speaker:narrator #@id line.common
    return #@id return.common
"#;

#[astra_headless_test::test]
fn graph_timeline_metadata_roundtrips_command_ids_to_source_map() {
    let compiled = compile_astra_project(
        [AstraSource::story("story.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let metadata = EditorVisualMetadata {
        schema: "astra.vn.editor_visual_metadata.v1".to_string(),
        graph_nodes: vec![
            GraphNodeMetadata {
                id: "node.start".to_string(),
                command_id: "line.start".to_string(),
                x: 10.0,
                y: 20.0,
            },
            GraphNodeMetadata {
                id: "node.call".to_string(),
                command_id: "call.common".to_string(),
                x: 80.0,
                y: 20.0,
            },
        ],
        timeline_tracks: vec![TimelineTrackMetadata {
            id: "track.dialogue".to_string(),
            command_ids: vec!["line.start".to_string(), "line.after".to_string()],
            lane: "dialogue".to_string(),
        }],
    };

    let report = metadata.validate_against(&compiled);
    assert!(report.passed, "{report:?}");
    let patch = metadata.to_patch_manifest();
    assert_eq!(
        patch.command_ids,
        ["call.common", "line.after", "line.start"]
    );

    let mut broken = metadata;
    broken.timeline_tracks[0]
        .command_ids
        .push("missing.command".to_string());
    let report = broken.validate_against(&compiled);
    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_EDITOR_METADATA_SOURCE_MISSING"));
}
