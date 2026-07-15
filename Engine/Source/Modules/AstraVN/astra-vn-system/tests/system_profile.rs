use astra_vn_system::{
    compile_astra_sources, AstraSource, SystemPageKind, SystemStoryManifest,
    SystemStoryValidationStatus, VnSystemUiProfileManifest,
};

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

#[astra_headless_test::test]
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

#[astra_headless_test::test]
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
