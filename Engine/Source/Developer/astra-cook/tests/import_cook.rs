use astra_asset::AssetSidecar;
use astra_cook::{
    ArtifactState, CookAudit, CookRequest, DefaultCookProcessor, DefaultMetadataImporter,
    ImportRequest,
};
use image::{ImageBuffer, Rgba};

#[test]
fn import_cook_classifies_fresh_stale_and_blocked_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("hero.png");
    let image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
    image.save(&source).unwrap();

    let importer = DefaultMetadataImporter::new("astra.import.image");
    let import = importer
        .import(ImportRequest {
            asset_id: "asset:/characters/hero/main".parse().unwrap(),
            source_path: source.clone(),
            asset_type: "image.rgba".to_string(),
            license: "project-owned".to_string(),
            font: None,
            target_profiles: vec!["desktop-release".to_string()],
        })
        .unwrap();
    assert!(import.diagnostics.is_empty());
    assert_eq!(import.metadata.kind, "image");

    let processor = DefaultCookProcessor::new("astra.cook.texture2d", "1.0.0");
    let request = CookRequest {
        sidecar: import.sidecar.clone(),
        source_bytes: std::fs::read(&source).unwrap(),
        target_profile: "desktop-release".to_string(),
        processor_version: "1.0.0".to_string(),
        dependency_artifacts: Default::default(),
    };
    let artifact = processor.cook(request.clone()).unwrap();
    let source_bytes = std::fs::read(&source).unwrap();
    assert_eq!(artifact.payload, source_bytes);
    assert_eq!(artifact.to_section().payload, source_bytes);
    assert_eq!(artifact.payload_hash, artifact.source_hash);
    assert_eq!(
        CookAudit::classify(&request, &artifact),
        ArtifactState::Fresh
    );

    let stale_request = CookRequest {
        processor_version: "1.0.1".to_string(),
        ..request
    };
    assert_eq!(
        CookAudit::classify(&stale_request, &artifact),
        ArtifactState::Stale
    );

    let mut blocked_sidecar: AssetSidecar = import.sidecar;
    blocked_sidecar.license = None;
    let blocked = processor.cook(CookRequest {
        sidecar: blocked_sidecar,
        source_bytes: std::fs::read(&source).unwrap(),
        target_profile: "desktop-release".to_string(),
        processor_version: "1.0.0".to_string(),
        dependency_artifacts: Default::default(),
    });
    assert!(blocked
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diag| diag.code == "ASTRA_ASSET_LICENSE_MISSING"));
}

#[test]
fn text_import_rejects_invalid_utf8_and_preserves_valid_source() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("note.txt");
    let request = ImportRequest {
        asset_id: "asset:/text/note".parse().unwrap(),
        source_path: path.clone(),
        asset_type: "text".into(),
        license: "CC0-1.0".into(),
        font: None,
        target_profiles: vec!["classic".into()],
    };
    let importer = DefaultMetadataImporter::new("astra.import.text");
    std::fs::write(&path, "文本").unwrap();
    let imported = importer.import(request.clone()).unwrap();
    assert_eq!(imported.metadata.kind, "text");
    assert_eq!(imported.metadata.codec, "utf8");
    assert_eq!(imported.sidecar.cook.processor, "astra.cook.binary");
    std::fs::write(&path, [0xff]).unwrap();
    assert!(importer
        .import(request)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_IMPORT_TEXT_UTF8"));
}
