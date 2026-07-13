use astra_asset::{
    normalize_source_path, AssetId, AssetRegistry, AssetSidecar, CookSettings, ReviewStatus,
};
use astra_core::DiagnosticSeverity;

#[test]
fn sidecar_schema_valid_sidecar_roundtrips_and_registers_asset_uri() {
    let yaml = r#"
schema: astra.asset.v1
id: asset:/characters/hero/main
source: content/characters/hero/main.png
source_hash: sha256:936a185caaa266bb9cbe981e9e05cb78e716a8667b53e07f1f2f37de2f0368c5
type: image.rgba
license: project-owned
importer: astra.import.image
cook:
  processor: astra.cook.texture2d
  target_profiles: [desktop-release]
  params:
    color_space: srgb
review: accepted
"#;
    let sidecar = AssetSidecar::from_yaml(yaml).unwrap();
    assert_eq!(
        sidecar.id,
        AssetId::parse("asset:/characters/hero/main").unwrap()
    );
    assert_eq!(
        normalize_source_path("content\\characters\\hero\\main.png").unwrap(),
        "content/characters/hero/main.png"
    );

    let encoded = sidecar.to_yaml().unwrap();
    let decoded = AssetSidecar::from_yaml(&encoded).unwrap();
    assert_eq!(decoded, sidecar);

    let mut registry = AssetRegistry::default();
    registry.insert(sidecar).unwrap();
    assert!(registry
        .get(&AssetId::parse("asset:/characters/hero/main").unwrap())
        .is_some());
}

#[test]
fn sidecar_schema_invalid_sidecar_reports_blocking_diagnostics() {
    let sidecar = AssetSidecar {
        schema: "astra.asset.v1".to_string(),
        id: AssetId::parse("asset:/bad").unwrap(),
        source: "../secret.png".to_string(),
        source_hash: None,
        asset_type: "image.rgba".to_string(),
        license: None,
        importer: String::new(),
        dependencies: vec![
            AssetId::parse("asset:/bad").unwrap(),
            AssetId::parse("asset:/bad").unwrap(),
        ],
        cook: CookSettings {
            processor: String::new(),
            target_profiles: vec![],
            params: Default::default(),
        },
        review: ReviewStatus::Accepted,
    };
    let diagnostics = sidecar.validate();
    assert!(diagnostics
        .iter()
        .all(|diag| diag.severity == DiagnosticSeverity::Blocking));
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_LICENSE_MISSING"));
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_SOURCE_PATH"));
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_SOURCE_HASH_MISSING"));
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_DEPENDENCY_SELF"));
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_DEPENDENCY_DUPLICATE"));
}

#[test]
fn sidecar_schema_duplicate_asset_id_blocks_registry_insert() {
    let sidecar = AssetSidecar::new_test(
        "asset:/characters/hero/main",
        "content/characters/hero/main.png",
        "image.rgba",
    );
    let mut registry = AssetRegistry::default();
    registry.insert(sidecar.clone()).unwrap();
    let err = registry.insert(sidecar).unwrap_err();
    assert!(err
        .diagnostics()
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_DUPLICATE_ID"));
}
