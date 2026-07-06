use astra_media::{
    LayoutConstraint, RubySpan, TextLayoutProvider, TextLayoutRequest, TextRun, VoiceReplayRef,
};

#[test]
fn text_layout_covers_cjk_ruby_wrap_and_voice_metadata() {
    let provider = astra_media::CosmicTextLayoutProvider::new_headless();
    let request = TextLayoutRequest {
        key: "line.opening".to_string(),
        runs: vec![TextRun {
            text: "空を見上げる".to_string(),
            locale: "ja-JP".to_string(),
            ruby: vec![RubySpan {
                base_range: 0..1,
                text: "そら".to_string(),
            }],
            voice: Some(VoiceReplayRef {
                asset: "asset:/voice/opening/001".parse().unwrap(),
                cue: "001".to_string(),
            }),
        }],
        constraint: LayoutConstraint {
            max_width: 48.0,
            font_size: 16.0,
            line_height: 20.0,
        },
        font_families: vec!["Missing Test Font".to_string()],
    };
    let layout = provider.layout(&request).unwrap();
    assert!(!layout.boxes.is_empty());
    assert!(layout
        .boxes
        .iter()
        .all(|line| !line.text.contains('\u{fffd}')));
    assert_eq!(layout.ruby_boxes.len(), 1);
    assert_eq!(layout.ruby_boxes[0].text, "そら");
    assert_eq!(layout.voice_refs.len(), 1);
    assert!(layout
        .diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_TEXT_FONT_FALLBACK"));
    assert_eq!(
        provider.layout_hash(&request).unwrap(),
        provider.layout_hash(&request).unwrap()
    );
}
