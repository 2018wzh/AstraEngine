use astra_vn::{
    compile_astra_sources, AstraSource, CameraState, EditorVisualMetadata, GraphNodeMetadata,
    LayerKind, PresentationTimeline, StageModel, StandardPresentationCommand, SystemPageKind,
    SystemStoryManifest, SystemStoryValidationStatus, TimelineJoinPolicy, TimelineTrackMetadata,
    VnPlayerCommand, VnRunConfig, VnRuntime, VnSystemUiProfileManifest,
};

const CALL_RETURN: &str = r#"
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

const SYSTEM_STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.start #@id line.start

story system #@id story.system
state title #@id state.system.title
  scene title #@id scene.system.title
    system_page kind:title policy:astra.policy.standard #@id page.title
state save #@id state.system.save
  scene save #@id scene.system.save
    system_page kind:save policy:astra.policy.standard #@id page.save
state load #@id state.system.load
  scene load #@id scene.system.load
    system_page kind:load policy:astra.policy.standard #@id page.load
state config #@id state.system.config
  scene config #@id scene.system.config
    system_page kind:config policy:astra.policy.standard #@id page.config
state gallery #@id state.system.gallery
  scene gallery #@id scene.system.gallery
    system_page kind:gallery policy:astra.policy.standard #@id page.gallery
state replay #@id state.system.replay
  scene replay #@id scene.system.replay
    system_page kind:replay policy:astra.policy.standard #@id page.replay
state voice_replay #@id state.system.voice_replay
  scene voice_replay #@id scene.system.voice_replay
    system_page kind:voice_replay policy:astra.policy.standard #@id page.voice_replay
state route_chart #@id state.system.route_chart
  scene route_chart #@id scene.system.route_chart
    system_page kind:route_chart policy:astra.policy.standard #@id page.route_chart
state backlog #@id state.system.backlog
  scene backlog #@id scene.system.backlog
    system_page kind:backlog policy:astra.policy.standard #@id page.backlog
state localization_preview #@id state.system.localization_preview
  scene localization_preview #@id scene.system.localization_preview
    system_page kind:localization_preview policy:astra.policy.standard #@id page.localization_preview
"#;

#[test]
fn runtime_supports_call_return_stack_and_resume_cursor() {
    let compiled =
        compile_astra_sources([AstraSource::new("call_return.astra", CALL_RETURN)]).unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();

    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();
    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime.state().current_state.as_deref(),
        Some("state.common")
    );
    assert_eq!(runtime.state().call_stack.len(), 1);

    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime.state().current_state.as_deref(),
        Some("state.prologue")
    );
    assert!(runtime.state().call_stack.is_empty());
    assert_eq!(
        runtime
            .state()
            .backlog
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        ["line.start", "line.common", "line.after"]
    );

    let terminal = runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert!(terminal.coverage.reached.contains("ending.good"));
}

#[test]
fn system_story_manifest_validates_required_entries_and_sources() {
    let compiled = compile_astra_sources([AstraSource::new("system.astra", SYSTEM_STORY)]).unwrap();

    let manifest = SystemStoryManifest::from_compiled(&compiled).unwrap();
    let required = SystemStoryManifest::commercial_required_pages();
    let report = manifest.validate_required(&required);

    assert_eq!(report.status, SystemStoryValidationStatus::Pass);
    assert_eq!(manifest.entries.len(), required.len());
    assert_eq!(
        manifest
            .entries
            .get(&SystemPageKind::Title)
            .unwrap()
            .source_id,
        "page.title"
    );

    let mut missing = manifest.clone();
    missing.entries.remove(&SystemPageKind::Gallery);
    let report = missing.validate_required(&required);
    assert_eq!(report.status, SystemStoryValidationStatus::Blocked);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_SYSTEM_ENTRY_MISSING"));
}

#[test]
fn system_ui_profile_manifest_validates_migration_unlock_and_localization() {
    let compiled = compile_astra_sources([AstraSource::new("system.astra", SYSTEM_STORY)]).unwrap();
    let manifest = VnSystemUiProfileManifest::from_compiled(&compiled, vec!["zh-Hans".to_string()]);

    let report = manifest.validate();
    assert_eq!(report.status, SystemStoryValidationStatus::Pass);

    let mut missing_migration = manifest.clone();
    missing_migration.save_migration.migrator_id.clear();
    assert_eq!(
        missing_migration.validate().diagnostics[0].code,
        "ASTRA_VN_SYSTEM_MIGRATION"
    );

    let mut missing_unlock = manifest.clone();
    missing_unlock.unlock_sources.clear();
    assert!(missing_unlock
        .validate()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_UNLOCK_SOURCE_POLICY"));

    let mut missing_localization = manifest;
    missing_localization.localization.locales.clear();
    assert_eq!(
        missing_localization.validate().diagnostics[0].code,
        "ASTRA_VN_LOCALIZATION_COVERAGE"
    );
}

#[test]
fn presentation_model_applies_standard_commands_and_hashes_timeline() {
    let mut stage = StageModel::new(1920, 1080);
    stage.apply(StandardPresentationCommand::SetCamera(CameraState {
        x: 0.0,
        y: 12.0,
        zoom: 1.2,
        rotation: 0.0,
    }));
    stage.apply(StandardPresentationCommand::ShowLayer {
        id: "bg.school".to_string(),
        kind: LayerKind::Background,
        asset: "native-assets/bg/school.png".to_string(),
        z: 0,
        x: 0.0,
        y: 0.0,
    });
    stage.apply(StandardPresentationCommand::ShowLayer {
        id: "char.hero.body".to_string(),
        kind: LayerKind::Character,
        asset: "native-assets/ch/hero_atlas.png#body.normal".to_string(),
        z: 100,
        x: 960.0,
        y: 540.0,
    });
    stage.apply(StandardPresentationCommand::SetTextWindow {
        id: "main".to_string(),
        x: 128.0,
        y: 756.0,
        width: 1664.0,
        height: 236.0,
    });
    stage.apply(StandardPresentationCommand::RunTimeline(
        PresentationTimeline {
            id: "tl.fade_in".to_string(),
            join_policy: TimelineJoinPolicy::BlockUntilComplete,
            tracks: vec![astra_vn::TimelineTrack {
                target: "char.hero.body".to_string(),
                property: "opacity".to_string(),
                keyframes: vec![
                    astra_vn::TimelineKeyframe {
                        time_ms: 0,
                        value: 0.0,
                    },
                    astra_vn::TimelineKeyframe {
                        time_ms: 260,
                        value: 1.0,
                    },
                ],
            }],
        },
    ));

    assert_eq!(stage.layers[0].id, "bg.school");
    assert_eq!(stage.layers[1].id, "char.hero.body");
    assert_eq!(stage.text_windows[0].id, "main");
    assert_eq!(stage.timelines[0].stable_hash().to_hex().len(), 32);
    assert_eq!(stage.presentation_hash().to_hex().len(), 32);
}

#[test]
fn graph_timeline_metadata_roundtrips_command_ids_to_source_map() {
    let compiled =
        compile_astra_sources([AstraSource::new("call_return.astra", CALL_RETURN)]).unwrap();
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
