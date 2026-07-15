use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader};
use astra_vn::{
    compile_astra_sources, package_sections_for_story, AstraSource, StageModel,
    SystemStoryManifest, VnPlayerCommand, VnRunConfig, VnRuntime,
};

#[astra_headless_test::test]
fn vn_dylib_facade_reexports_runtime_story_and_package_api() {
    let compiled = compile_astra_sources([AstraSource::new(
        "facade.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:hello speaker:narrator #@id line.hello
"#,
    )])
    .unwrap();

    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("zh-Hans")).unwrap();
    let output = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();
    assert_eq!(output.presentation.len(), 1);

    let stage = StageModel::new(1280, 720);
    assert_eq!(stage.presentation_hash().to_hex().len(), 32);
    let system_manifest = SystemStoryManifest::from_compiled(&compiled).unwrap();
    assert_eq!(system_manifest.schema, "astra.vn.system_story_manifest.v1");

    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "facade-game").unwrap();
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.facade",
        "classic",
        sections,
    ))
    .unwrap();
    let reader = PackageReader::open(blob.as_bytes()).unwrap();
    assert!(reader.has_section("vn.compiled_story"));
    assert!(reader.has_section("vn.profile_manifest"));
    assert!(reader.has_section("vn.system_story_manifest"));
}
