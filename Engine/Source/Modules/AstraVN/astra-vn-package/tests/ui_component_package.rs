use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader};
use astra_ui_plugin_abi::{UiComponentManifest, UI_COMPONENT_MANIFEST_SCHEMA};
use astra_vn_package::{
    load_ui_component_artifact, package_sections_for_project,
    package_sections_for_project_with_components, VnUiComponentArtifactInput, VnUiComponentTarget,
};
use astra_vn_script::{compile_astra_project, AstraSource};
use ed25519_dalek::SigningKey;

fn manifest(component_id: &str, artifact: &[u8], signing: &SigningKey) -> UiComponentManifest {
    let mut manifest = UiComponentManifest {
        schema: UI_COMPONENT_MANIFEST_SCHEMA.into(),
        component_id: component_id.into(),
        component_version: "1.0.0".into(),
        signer_id: "fixture.test_signer".into(),
        engine_version: "test".into(),
        rustc_fingerprint: "rustc.test".into(),
        feature_fingerprint: "features.test".into(),
        abi_fingerprint: "astra.ui_component_abi.v1".into(),
        artifact_hash: Hash256::from_sha256(artifact),
        input_schema: "astra.ui_component_request.v1".into(),
        output_schema: "astra.ui_component_response.v1".into(),
        capabilities: BTreeSet::from(["ui.render_frame".into()]),
        signature: Vec::new(),
    };
    manifest.sign(signing).unwrap();
    manifest
}

#[astra_headless_test::test]
fn component_artifacts_are_target_bound_signed_and_hash_verified() {
    let project = compile_astra_project(
        [
            AstraSource::story("story.astra", "story main\nstate start\n  scene room\n"),
            AstraSource::ui(
                "components.astra",
                "ui_component fixture.component #@id component.fixture\n",
            ),
        ],
        Default::default(),
    )
    .unwrap();
    assert!(package_sections_for_project(&project, &["classic".into()], "game").is_err());

    let signing = SigningKey::from_bytes(&[17; 32]);
    let public = signing.verifying_key().to_bytes();
    let windows = b"test-only-windows-dylib".to_vec();
    let web = b"test-only-web-component".to_vec();
    let inputs = vec![
        VnUiComponentArtifactInput {
            target: VnUiComponentTarget::Windows,
            manifest: manifest("fixture.component", &windows, &signing),
            artifact: windows.clone(),
            signer_public_key: public,
        },
        VnUiComponentArtifactInput {
            target: VnUiComponentTarget::Web,
            manifest: manifest("fixture.component", &web, &signing),
            artifact: web.clone(),
            signer_public_key: public,
        },
    ];
    let sections = package_sections_for_project_with_components(
        &project,
        &["classic".into()],
        "game",
        &inputs,
    )
    .unwrap();
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "component.package",
        "classic",
        sections,
    ))
    .unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    let allowlist = BTreeMap::from([("fixture.test_signer".into(), public)]);
    let loaded = load_ui_component_artifact(
        &package,
        "fixture.component",
        VnUiComponentTarget::Windows,
        &allowlist,
    )
    .unwrap();
    assert_eq!(loaded.artifact, windows);
    assert!(load_ui_component_artifact(
        &package,
        "fixture.component",
        VnUiComponentTarget::Web,
        &BTreeMap::new(),
    )
    .is_err());
}
