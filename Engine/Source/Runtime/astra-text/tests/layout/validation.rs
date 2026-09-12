use super::*;

#[test]
fn bidi_empty_input_and_clip_policy_are_explicit() {
    let provider = provider();
    let empty = provider.layout(&request("")).unwrap();
    assert!(empty.shaped_runs.is_empty());
    assert!(empty.glyph_resources.is_empty());

    let mut bidi = request("\u{200f}ABC");
    bidi.runs[0].direction = TextDirection::RightToLeft;
    let bidi_layout = provider.layout(&bidi).unwrap();
    assert!(bidi_layout
        .lines
        .iter()
        .filter(|line| line.role == GlyphRole::Base)
        .all(|line| line.rtl));

    let mut clipped = request("this line is wider than the clipping rectangle");
    clipped.constraint.max_width = 48.0;
    clipped.constraint.wrap = WrapPolicy::None;
    clipped.constraint.overflow = OverflowPolicy::Clip;
    let clipped_layout = provider.layout(&clipped).unwrap();
    assert!(clipped_layout.clipped);
    assert_eq!(clipped_layout.clip.unwrap().width, 48);
    let mut owner = TextRenderResourceOwner::default();
    let commands = owner
        .update_layout("clip", &clipped_layout, [255; 4])
        .unwrap();
    assert!(commands
        .iter()
        .any(|command| matches!(command, SceneCommand::PushClip { .. })));
    assert!(commands
        .iter()
        .any(|command| matches!(command, SceneCommand::PopClip)));
}

#[test]
fn font_binding_hash_direction_and_fallback_fail_fast() {
    let bytes =
        include_bytes!("../../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
            .to_vec();
    let mut corrupt = font(bytes.clone());
    corrupt.hash = Hash256::from_sha256(b"wrong");
    let error = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        vec![corrupt],
        TextLayoutConfig::production_defaults(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("ASTRA_TEXT_PACKAGED_FONT_HASH"));

    let provider = provider();
    let mut missing = request("text");
    missing.font_families = vec!["Undeclared Family".into()];
    assert!(provider
        .layout(&missing)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_TEXT_FONT_CHAIN"));

    let mut wrong_direction = request("left to right");
    wrong_direction.runs[0].direction = TextDirection::RightToLeft;
    assert!(provider
        .layout(&wrong_direction)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_TEXT_DIRECTION"));
}

#[test]
fn font_replacement_is_transactional_and_invalidates_layout_cache() {
    let provider = provider();
    let request = request("cache identity");
    let first = provider.layout(&request).unwrap();
    let initial = provider.cache_stats().unwrap();
    assert_eq!(initial.font_generation, 1);
    assert_eq!(initial.entries, 1);

    let original =
        include_bytes!("../../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
            .to_vec();
    let original_hash = Hash256::from_sha256(&original);
    let mut replacement_bytes = original;
    replacement_bytes.push(0);
    let replacement = font(replacement_bytes);
    provider
        .replace_font("asset:/font/ui/poppins-regular", original_hash, replacement)
        .unwrap();
    let replaced = provider.cache_stats().unwrap();
    assert_eq!(replaced.font_generation, 2);
    assert_eq!(replaced.entries, 0);
    let second = provider.layout(&request).unwrap();
    assert_ne!(first.revision, second.revision);

    let before_failure = provider.cache_stats().unwrap();
    let invalid = font(vec![1, 2, 3]);
    let error = provider
        .replace_font(
            "asset:/font/ui/poppins-regular",
            Hash256::from_sha256(b"not-installed"),
            invalid,
        )
        .unwrap_err();
    assert!(error.to_string().contains("ASTRA_TEXT_FONT_HASH"));
    assert_eq!(provider.cache_stats().unwrap(), before_failure);
}
