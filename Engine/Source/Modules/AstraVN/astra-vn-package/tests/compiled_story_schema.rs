use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_vn_package::decode_compiled_story;
use astra_vn_script::{compile_astra_sources, AstraSource};

#[astra_headless_test::test]
fn legacy_compiled_story_schema_requires_recook() {
    let compiled = compile_astra_sources([AstraSource::new(
        "story.astra",
        "story main\nstate start\n  scene room\n    text key:line\n",
    )])
    .unwrap();
    let section =
        SectionPayload::postcard("vn.compiled_story", "astra.vn.compiled_story.v1", &compiled)
            .unwrap();
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "legacy.story",
        "classic",
        vec![section],
    ))
    .unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();

    let error = decode_compiled_story(&package).unwrap_err();
    assert!(error.to_string().contains("ASTRA_VN_RECOOK_REQUIRED"));
}
