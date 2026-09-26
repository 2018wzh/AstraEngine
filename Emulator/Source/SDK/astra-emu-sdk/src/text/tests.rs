use super::*;
use astra_core::Hash256;
use astra_text::*;

#[test]
fn measuring_shares_layout_without_consuming_glyph_lifecycle() {
    let bytes =
        include_bytes!("../../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf");
    let font = PackagedFont {
        asset_id: "asset:/test/noto".into(),
        family: "Noto Sans JP".into(),
        face_index: 0,
        hash: Hash256::from_sha256(bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage: font_unicode_coverage(bytes, 0).unwrap(),
        targets: vec!["test".into()],
        profiles: vec!["test".into()],
        bytes: bytes.to_vec(),
    };
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "test".into(),
            profile: "test".into(),
            default_locale: "ja-JP".into(),
        },
        vec![font],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let mut scene = TextScene::new(provider);
    let request = TextLayoutRequest {
        key: "native-character".into(),
        runs: vec![TextRun {
            text: "W".into(),
            language: "ja-JP".into(),
            script: Some("Jpan".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: 128.0,
            max_height: None,
            max_lines: None,
            font_size: 30.0,
            line_height: 30.0,
            wrap: WrapPolicy::None,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec!["Noto Sans JP".into()],
        features: Vec::new(),
    };
    let first = scene.layout_shared(&request).unwrap();
    let second = scene.layout_shared(&request).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert!(first.shaped_runs[0].glyphs[0].advance > 15.0);
    assert!(scene.clear().is_empty());
    let region = TextSceneLayout {
        request,
        translation: (0, 0),
        rgba: [255; 4],
        outline: None,
    };
    assert!(scene
        .frame(std::slice::from_ref(&region))
        .unwrap()
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    assert!(!scene
        .frame(&[region])
        .unwrap()
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    assert!(!scene.clear().is_empty());
}
