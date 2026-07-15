use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_vn_package::{
    load_presentation_provider_manifest, package_sections_for_story,
    VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA,
};
use astra_vn_script::{compile_astra_sources, AstraSource};

fn compiled_story() -> astra_vn_script::CompiledStory {
    compile_astra_sources([AstraSource::new(
        "story.astra",
        "story main\nstate start\n  scene room\n    background asset:asset:/bg layer:bg preset:soft_fade duration:300\n",
    )])
    .unwrap()
}

#[astra_headless_test::test]
fn package_persists_profile_bound_presentation_policy() {
    let sections = package_sections_for_story(
        &compiled_story(),
        &["classic".to_string(), "advanced-vn".to_string()],
        "game",
    )
    .unwrap();
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "presentation.policy",
        "classic",
        sections,
    ))
    .unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();

    let manifest = load_presentation_provider_manifest(&package, "classic").unwrap();
    assert_eq!(manifest.schema, VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA);
    assert!(manifest
        .resolve_preset("classic", "background", "soft_fade")
        .is_ok());
    assert_eq!(
        load_presentation_provider_manifest(&package, "undeclared")
            .unwrap_err()
            .to_string(),
        "ASTRA_VN_PRESENTATION_PROFILE_UNDECLARED: requested presentation profile is not declared by the package"
    );
}

#[astra_headless_test::test]
fn legacy_presentation_policy_requires_recook() {
    let legacy = astra_vn_package::VnPresentationProviderManifest::standard();
    let section = SectionPayload::postcard(
        "vn.presentation_provider_manifest",
        "astra.vn.presentation_provider_manifest.v1",
        &legacy,
    )
    .unwrap();
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "presentation.legacy",
        "classic",
        vec![section],
    ))
    .unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();

    let error = load_presentation_provider_manifest(&package, "classic").unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_VN_PRESENTATION_PROVIDER_SCHEMA"));
}
