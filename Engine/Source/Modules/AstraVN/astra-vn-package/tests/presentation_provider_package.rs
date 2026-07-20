use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_vn_package::{
    decode_compiled_project, load_presentation_provider_manifest, package_sections_for_project,
    VnCompiledProjectRoot, VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA,
};
use astra_vn_script::{compile_astra_project, AstraSource};

fn compiled_story() -> astra_vn_script::CompiledVnProject {
    compile_astra_project([AstraSource::story(
        "story.astra",
        "story main\nstate start\n  scene room\n    background asset:asset:/bg layer:bg preset:soft_fade duration:300\n",
    )], Default::default())
    .unwrap()
}

#[astra_headless_test::test]
fn package_persists_profile_bound_presentation_policy() {
    let sections = package_sections_for_project(
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
fn compiled_project_v3_requires_the_v2_root_without_reader_fallback() {
    let project = compiled_story();
    let mut sections =
        package_sections_for_project(&project, &["classic".to_string()], "game").unwrap();
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "compiled.project.v3",
        "classic",
        sections.clone(),
    ))
    .unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    let root: VnCompiledProjectRoot = package
        .container()
        .decode_postcard("vn.compiled_project")
        .unwrap();
    assert_eq!(root.schema, "astra.vn.compiled_project_root.v2");
    assert_eq!(root.compiled_project_schema, "astra.vn.compiled_project.v3");
    assert_eq!(
        decode_compiled_project(&package).unwrap().schema,
        "astra.vn.compiled_project.v3"
    );

    let legacy_root = VnCompiledProjectRoot {
        schema: "astra.vn.compiled_project_root.v1".to_string(),
        compiled_project_schema: "astra.vn.compiled_project.v2".to_string(),
        ..root
    };
    sections.retain(|section| section.id != "vn.compiled_project");
    sections.push(
        SectionPayload::postcard(
            "vn.compiled_project",
            "astra.vn.compiled_project_root.v1",
            &legacy_root,
        )
        .unwrap(),
    );
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "compiled.project.legacy",
        "classic",
        sections,
    ))
    .unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    assert!(decode_compiled_project(&package)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_VN_COMPILED_PROJECT_ROOT"));
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
