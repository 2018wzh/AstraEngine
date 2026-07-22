use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, FontPackageEntry, FontPackageManifest, GlyphRole,
    LayoutConstraint, OpenTypeFeature, OverflowPolicy, PackagedFont, RubySpan, SourceRange,
    TextDirection, TextLayoutBindingIdentity, TextLayoutConfig, TextLayoutProvider,
    TextLayoutReplayInput, TextLayoutReplayLimits, TextLayoutReplaySession,
    TextLayoutReplaySnapshot, TextLayoutRequest, TextRenderLayoutUpdate, TextRenderResourceOwner,
    TextRun, UnicodeRange, VoiceReplayRef, WrapPolicy, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_media_core::{
    CpuRendererProvider, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
    SceneCommand,
};
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use std::sync::Arc;

fn open_font_fixture(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainFonts")
        .join(file);
    std::fs::read(path).unwrap()
}

#[astra_headless_test::test]
fn frame_resource_journal_is_incremental_shared_and_transactional() {
    let provider = provider();
    let layout = provider.layout(&request("shared frame glyphs")).unwrap();
    let mut owner = TextRenderResourceOwner::default();
    let initial = owner
        .update_frame(
            &[
                TextRenderLayoutUpdate {
                    layout_id: "frame.a",
                    layout: &layout,
                    rgba: [255; 4],
                },
                TextRenderLayoutUpdate {
                    layout_id: "frame.b",
                    layout: &layout,
                    rgba: [255; 4],
                },
            ],
            &[],
        )
        .unwrap();
    assert_eq!(initial.layouts.len(), 2);
    assert_eq!(
        initial
            .lifecycle
            .iter()
            .filter(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
            .count(),
        layout.glyph_resources.len()
    );

    let stable = owner
        .update_frame(
            &[
                TextRenderLayoutUpdate {
                    layout_id: "frame.a",
                    layout: &layout,
                    rgba: [255; 4],
                },
                TextRenderLayoutUpdate {
                    layout_id: "frame.b",
                    layout: &layout,
                    rgba: [255; 4],
                },
            ],
            &[],
        )
        .unwrap();
    assert!(stable.lifecycle.is_empty());

    let mut conflicting = layout.clone();
    std::sync::Arc::make_mut(&mut conflicting.glyph_resources[0].bitmap.pixels)[0] ^= 0xff;
    conflicting.glyph_resources[0].bitmap.hash =
        Hash256::from_sha256(&conflicting.glyph_resources[0].bitmap.pixels);
    let error = owner
        .update_frame(
            &[TextRenderLayoutUpdate {
                layout_id: "frame.a",
                layout: &conflicting,
                rgba: [255; 4],
            }],
            &["frame.b"],
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_TEXT_RENDER_RESOURCE_CONFLICT"));

    let after_failure = owner
        .update_frame(
            &[
                TextRenderLayoutUpdate {
                    layout_id: "frame.a",
                    layout: &layout,
                    rgba: [255; 4],
                },
                TextRenderLayoutUpdate {
                    layout_id: "frame.b",
                    layout: &layout,
                    rgba: [255; 4],
                },
            ],
            &[],
        )
        .unwrap();
    assert!(after_failure.lifecycle.is_empty());

    let removed = owner.update_frame(&[], &["frame.a", "frame.b"]).unwrap();
    assert_eq!(
        removed
            .lifecycle
            .iter()
            .filter(|command| matches!(command, SceneCommand::ReleaseResource { .. }))
            .count(),
        layout.glyph_resources.len()
    );
}

fn fixture_font(
    asset_id: &str,
    family: &str,
    file: &str,
    coverage: Vec<UnicodeRange>,
) -> PackagedFont {
    let bytes = open_font_fixture(file);
    PackagedFont {
        asset_id: asset_id.into(),
        family: family.into(),
        face_index: 0,
        hash: Hash256::from_sha256(&bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage,
        targets: vec!["windows".into()],
        profiles: vec!["classic".into()],
        bytes,
    }
}

fn multiscript_fonts() -> Vec<PackagedFont> {
    vec![
        font(
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
                .to_vec(),
        ),
        fixture_font(
            "asset:/font/fallback/noto-sans-sc",
            "Noto Sans SC",
            "NotoSansSC-Variable.ttf",
            vec![
                UnicodeRange {
                    start: 0x3000,
                    end: 0x30ff,
                },
                UnicodeRange {
                    start: 0x3400,
                    end: 0x9fff,
                },
                UnicodeRange {
                    start: 0xff00,
                    end: 0xffef,
                },
            ],
        ),
        fixture_font(
            "asset:/font/fallback/noto-sans-arabic",
            "Noto Sans Arabic",
            "NotoSansArabic-Variable.ttf",
            vec![
                UnicodeRange {
                    start: 0x0600,
                    end: 0x06ff,
                },
                UnicodeRange {
                    start: 0x0750,
                    end: 0x077f,
                },
                UnicodeRange {
                    start: 0x08a0,
                    end: 0x08ff,
                },
            ],
        ),
        fixture_font(
            "asset:/font/fallback/noto-emoji",
            "Noto Emoji",
            "NotoEmoji-Variable.ttf",
            vec![
                UnicodeRange {
                    start: 0x200d,
                    end: 0x200d,
                },
                UnicodeRange {
                    start: 0x2600,
                    end: 0x27bf,
                },
                UnicodeRange {
                    start: 0xfe0f,
                    end: 0xfe0f,
                },
                UnicodeRange {
                    start: 0x1f300,
                    end: 0x1faff,
                },
            ],
        ),
    ]
}

fn font(bytes: Vec<u8>) -> PackagedFont {
    PackagedFont {
        asset_id: "asset:/font/ui/poppins-regular".into(),
        family: "Poppins".into(),
        face_index: 0,
        hash: Hash256::from_sha256(&bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage: vec![
            UnicodeRange {
                start: 0,
                end: 0x036f,
            },
            UnicodeRange {
                start: 0x2000,
                end: 0x206f,
            },
        ],
        targets: vec!["windows".into()],
        profiles: vec!["classic".into()],
        bytes,
    }
}

fn provider() -> CosmicTextLayoutProvider {
    CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        vec![font(
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
                .to_vec(),
        )],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap()
}

#[astra_headless_test::test]
fn overlapping_cjk_fallback_honors_the_explicit_family_order() {
    let jp_bytes =
        include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
            .to_vec();
    let jp = PackagedFont {
        asset_id: "asset:/font/fallback/noto-sans-jp".into(),
        family: "Noto Sans JP".into(),
        face_index: 0,
        hash: Hash256::from_sha256(&jp_bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage: vec![
            UnicodeRange {
                start: 32,
                end: 126,
            },
            UnicodeRange {
                start: 0x3000,
                end: 0x9fff,
            },
        ],
        targets: vec!["windows".into()],
        profiles: vec!["classic".into()],
        bytes: jp_bytes,
    };
    let sc = fixture_font(
        "asset:/font/fallback/noto-sans-sc",
        "Noto Sans SC",
        "NotoSansSC-Variable.ttf",
        vec![
            UnicodeRange {
                start: 32,
                end: 126,
            },
            UnicodeRange {
                start: 0x3000,
                end: 0x9fff,
            },
        ],
    );
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "ja".into(),
        },
        vec![jp, sc],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let sample = "CONFIG SAVE LOAD EXIT　設定文字を隠す通常早送り音声ありなし";
    for character in sample.chars() {
        let mut request = request(&character.to_string());
        request.runs[0].language = "ja".into();
        request.runs[0].script = Some("Jpan".into());
        request.font_families = vec!["Noto Sans JP".into(), "Noto Sans SC".into()];
        provider.layout(&request).unwrap_or_else(|error| {
            panic!(
                "CJK fallback failed for U+{:04X}: {error}",
                character as u32
            )
        });
    }
    let mut sample_request = request(sample);
    sample_request.runs[0].language = "ja".into();
    sample_request.runs[0].script = Some("Jpan".into());
    sample_request.font_families = vec!["Noto Sans JP".into(), "Noto Sans SC".into()];
    provider.layout(&sample_request).unwrap();

    let mut short_request = request("設定");
    short_request.runs[0].language = "ja".into();
    short_request.runs[0].script = Some("Jpan".into());
    short_request.font_families = vec!["Noto Sans JP".into(), "Noto Sans SC".into()];
    provider.layout(&short_request).unwrap();
}

fn request(text: &str) -> TextLayoutRequest {
    TextLayoutRequest {
        key: "line.production".into(),
        runs: vec![TextRun {
            text: text.into(),
            language: "en-US".into(),
            script: Some("Latn".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: 240.0,
            max_height: None,
            max_lines: None,
            font_size: 24.0,
            line_height: 32.0,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec!["Poppins".into()],
        features: vec![
            OpenTypeFeature {
                tag: "kern".into(),
                value: 1,
            },
            OpenTypeFeature {
                tag: "liga".into(),
                value: 1,
            },
        ],
    }
}

fn replay_binding(provider: &CosmicTextLayoutProvider) -> TextLayoutBindingIdentity {
    TextLayoutBindingIdentity {
        provider_id: "astra.media.cosmic_text".into(),
        provider_fingerprint: Hash256::from_sha256(b"cosmic-text-test-provider-v1"),
        package_hash: Hash256::from_sha256(b"text-layout-test-package-v1"),
        build_fingerprint: Hash256::from_sha256(b"text-layout-test-build-v1"),
        session_id: "session.text-layout-test".into(),
        provider: provider.identity().unwrap(),
    }
}

#[astra_headless_test::test]
fn shaped_clusters_fonts_ruby_voice_and_glyph_bitmaps_reach_renderer() {
    let provider = provider();
    let mut request = request("AV cafe\u{301} office");
    request.runs[0].ruby = vec![RubySpan {
        base_range: SourceRange { start: 0, end: 2 },
        text: "pair".into(),
    }];
    request.runs[0].voice = Some(VoiceReplayRef {
        asset: "asset:/voice/opening/001".into(),
        cue: "001".into(),
    });

    let layout = provider.layout(&request).unwrap();
    assert_eq!(layout.schema, "astra.text_layout.v2");
    assert!(!layout.shaped_runs.is_empty());
    assert!(!layout.glyph_resources.is_empty());
    assert_eq!(layout.ruby_boxes.len(), 1);
    assert_eq!(layout.voice_refs.len(), 1);
    assert!(layout
        .shaped_runs
        .iter()
        .any(|run| run.role == GlyphRole::Base));
    assert!(layout
        .shaped_runs
        .iter()
        .any(|run| { run.role == GlyphRole::Ruby { span_index: 0 } }));
    assert!(layout
        .shaped_runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .all(|glyph| {
            glyph.font_family == "Poppins"
                && glyph.font_hash
                    == Hash256::from_sha256(include_bytes!(
                        "../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf"
                    ))
                && glyph.source.start <= glyph.source.end
        }));
    assert!(layout.diagnostics.is_empty());

    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 320,
            height: 96,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "text-golden".into(),
        })
        .unwrap();
    let mut resource_owner = TextRenderResourceOwner::default();
    let mut commands = vec![SceneCommand::clear([8, 8, 8, 255])];
    commands.extend(
        resource_owner
            .update_layout("line.production", &layout, [255, 255, 255, 255])
            .unwrap(),
    );
    let frame = renderer.capture_frame(&commands).unwrap();
    assert!(frame
        .bytes
        .chunks_exact(4)
        .any(|pixel| pixel != [8, 8, 8, 255]));
    let redraw = resource_owner
        .update_layout("line.production", &layout, [255, 255, 255, 255])
        .unwrap();
    assert_eq!(redraw.len(), 1);
    renderer.capture_frame(&redraw).unwrap();
    let mut conflicting = layout.clone();
    std::sync::Arc::make_mut(&mut conflicting.glyph_resources[0].bitmap.pixels)[0] ^= 0xff;
    conflicting.glyph_resources[0].bitmap.hash =
        Hash256::from_sha256(&conflicting.glyph_resources[0].bitmap.pixels);
    assert!(resource_owner
        .update_layout("line.production", &conflicting, [255; 4])
        .unwrap_err()
        .to_string()
        .contains("ASTRA_TEXT_RENDER_RESOURCE_CONFLICT"));
    assert_eq!(
        resource_owner
            .update_layout("line.production", &layout, [255; 4])
            .unwrap()
            .len(),
        1
    );
    renderer
        .capture_frame(&resource_owner.remove_layout("line.production").unwrap())
        .unwrap();

    let repeated = provider.layout(&request).unwrap();
    assert_eq!(layout.hash, repeated.hash);
    let stats = provider.cache_stats().unwrap();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
}

#[astra_headless_test::test]
fn measurement_reuses_the_authoritative_layout_cache_without_glyph_clone_contract() {
    let provider = provider();
    let request = request("measure this line");
    let measured = provider.measure(&request).unwrap();
    let stats_after_measure = provider.cache_stats().unwrap();
    assert_eq!(stats_after_measure.entries, 1);
    assert_eq!(stats_after_measure.misses, 1);

    let layout = provider.layout(&request).unwrap();
    assert_eq!(measured.width, layout.width);
    assert_eq!(measured.height, layout.height);
    assert_eq!(measured.hash, layout.hash);
    let stats_after_layout = provider.cache_stats().unwrap();
    assert_eq!(stats_after_layout.misses, 1);
    assert_eq!(stats_after_layout.hits, 1);
}

#[astra_headless_test::test]
fn shared_layout_reuses_the_authoritative_cached_allocation() {
    let provider = provider();
    let request = request("share this shaped line");

    let first = provider.layout_shared(&request).unwrap();
    let second = provider.layout_shared(&request).unwrap();

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(provider.cache_stats().unwrap().entries, 1);
}

#[astra_headless_test::test]
fn licensed_multiscript_fallback_shapes_cjk_arabic_and_emoji_clusters() {
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        multiscript_fonts(),
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let mut request = request("Latin cafe\u{301}");
    request.key = "multiscript.production".into();
    request.font_families = vec![
        "Poppins".into(),
        "Noto Sans SC".into(),
        "Noto Sans Arabic".into(),
        "Noto Emoji".into(),
    ];
    request.runs.extend([
        TextRun {
            text: "中文かなカナ".into(),
            language: "zh-CN".into(),
            script: Some("Hans".into()),
            direction: TextDirection::LeftToRight,
            ruby: vec![RubySpan {
                base_range: SourceRange { start: 0, end: 6 },
                text: "ちゅうぶん".into(),
            }],
            voice: None,
        },
        TextRun {
            text: "السَّلَامُ".into(),
            language: "ar".into(),
            script: Some("Arab".into()),
            direction: TextDirection::RightToLeft,
            ruby: Vec::new(),
            voice: None,
        },
        TextRun {
            text: "☀️👩‍💻😀".into(),
            language: "und".into(),
            script: Some("Zyyy".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        },
    ]);
    request.constraint.max_width = 640.0;

    let layout = provider.layout(&request).unwrap();
    let families = layout
        .shaped_runs
        .iter()
        .map(|run| run.font_family.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for family in ["Poppins", "Noto Sans SC", "Noto Sans Arabic", "Noto Emoji"] {
        assert!(families.contains(family), "missing shaped family {family}");
    }
    assert!(layout
        .shaped_runs
        .iter()
        .filter(|run| run.run_index == 2)
        .all(|run| run.direction == TextDirection::RightToLeft));
    assert!(layout
        .shaped_runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .all(|glyph| glyph.glyph_id != 0 && glyph.source.start <= glyph.source.end));
    assert!(
        layout
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "ASTRA_TEXT_FONT_FALLBACK")
            .count()
            >= 3
    );
    assert!(layout
        .glyph_resources
        .iter()
        .any(|resource| resource.font_asset_id.ends_with("noto-emoji")));
    assert_eq!(layout.hash, provider.layout(&request).unwrap().hash);
}

#[astra_headless_test::test]
fn cjk_vertical_layout_places_columns_ruby_rotated_glyphs_and_tate_chu_yoko() {
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "ja".into(),
        },
        multiscript_fonts(),
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    for (direction, language) in [
        (TextDirection::VerticalRightToLeft, "ja"),
        (TextDirection::VerticalLeftToRight, "zh-Hans"),
    ] {
        let mut vertical = request("終12A端");
        vertical.key = format!("vertical.{language}");
        vertical.font_families = vec!["Poppins".into(), "Noto Sans SC".into()];
        vertical.constraint.max_width = 240.0;
        vertical.constraint.max_height = Some(160.0);
        vertical.constraint.wrap = WrapPolicy::Glyph;
        vertical.runs[0] = TextRun {
            text: "終12A端".into(),
            language: language.into(),
            script: Some(if language == "ja" { "Jpan" } else { "Hans" }.into()),
            direction,
            ruby: vec![RubySpan {
                base_range: SourceRange { start: 0, end: 3 },
                text: "しゅう".into(),
            }],
            voice: None,
        };
        let layout = provider.layout(&vertical).unwrap();
        assert!(layout.width >= vertical.constraint.line_height);
        assert!(layout.height > 0.0);
        assert_eq!(layout.ruby_boxes.len(), 1);
        assert!(layout
            .shaped_runs
            .iter()
            .all(|run| run.direction == direction));
        let glyphs = layout
            .shaped_runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .collect::<Vec<_>>();
        let minimum_x = glyphs
            .iter()
            .filter_map(|glyph| glyph.render_x)
            .min()
            .expect("vertical glyph x") as f32;
        match direction {
            TextDirection::VerticalRightToLeft => assert!(minimum_x > 100.0),
            TextDirection::VerticalLeftToRight => assert!(minimum_x < 100.0),
            _ => unreachable!(),
        }
        assert!(glyphs.iter().any(|glyph| glyph.tate_chu_yoko));
        assert!(glyphs
            .iter()
            .any(|glyph| glyph.rotation_quadrants == 1 && !glyph.tate_chu_yoko));
        assert!(glyphs
            .iter()
            .all(|glyph| glyph.render_x.is_some() && glyph.render_y.is_some()));
        assert_eq!(layout.hash, provider.layout(&vertical).unwrap().hash);
    }
}

#[astra_headless_test::test]
fn open_font_fixture_manifest_is_revision_hash_and_license_bound() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainFonts");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema"], "astra.open_font_fixture_manifest.v1");
    assert_eq!(
        manifest["source_revision"],
        "ec0464b978de222073645d6d3366f3fdf03376d8"
    );
    let fonts = manifest["fonts"].as_array().unwrap();
    assert_eq!(fonts.len(), 3);
    for entry in fonts {
        let bytes = std::fs::read(root.join(entry["file"].as_str().unwrap())).unwrap();
        assert_eq!(entry["byte_size"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(
            entry["sha256"].as_str().unwrap(),
            Hash256::from_sha256(&bytes).to_string()
        );
        let license = std::fs::read(root.join(entry["license_file"].as_str().unwrap())).unwrap();
        assert!(!license.is_empty());
        assert!(entry["source_url"]
            .as_str()
            .unwrap()
            .contains(manifest["source_revision"].as_str().unwrap()));
    }
}

#[astra_headless_test::test]
fn wrap_ellipsis_and_source_clusters_are_semantic() {
    let provider = provider();
    let mut request = request("A very long production text line with multiple words and clusters");
    request.constraint.max_width = 96.0;
    request.constraint.max_lines = Some(1);
    request.constraint.overflow = OverflowPolicy::EllipsisEnd;
    let layout = provider.layout(&request).unwrap();
    assert!(layout.ellipsized);
    assert!(!layout.clipped);
    assert_eq!(
        layout
            .lines
            .iter()
            .filter(|line| line.run_index == 0)
            .count(),
        1
    );
    assert!(layout
        .shaped_runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .all(|glyph| glyph.source.end <= request.runs[0].text.len()));
}

#[astra_headless_test::test]
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

#[astra_headless_test::test]
fn font_binding_hash_direction_and_fallback_fail_fast() {
    let bytes = include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
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

#[astra_headless_test::test]
fn font_replacement_is_transactional_and_invalidates_layout_cache() {
    let provider = provider();
    let request = request("cache identity");
    let first = provider.layout(&request).unwrap();
    let initial = provider.cache_stats().unwrap();
    assert_eq!(initial.font_generation, 1);
    assert_eq!(initial.entries, 1);

    let original =
        include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
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
    assert_ne!(first.hash, second.hash);

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

#[astra_headless_test::test]
fn verified_package_vfs_is_the_font_database_authority() {
    let font_bytes =
        include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
            .to_vec();
    let font_hash = Hash256::from_sha256(&font_bytes);
    let font_section =
        SectionPayload::raw("asset.font.ui", "astra.cooked_asset.v1", font_bytes.clone());
    let mut build =
        PackageBuildRequest::fixture("com.example.font-authority", "classic", vec![font_section]);
    build.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": { "kind": "package_section", "section_id": "package.manifest" },
            "targets": ["native-smoke-game"],
            "profiles": ["classic"]
        }],
        "entries": [{
            "vfs_uri": "package:/asset/font/ui",
            "layer_id": "package.base",
            "source": { "kind": "package_section", "section_id": "asset.font.ui" },
            "offset": 0,
            "size": font_bytes.len(),
            "hash": font_hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }],
        "whiteouts": []
    }))
    .unwrap();
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.into(),
        target: "native-smoke-game".into(),
        profile: "classic".into(),
        provider_binding: "astra.vfs.package".into(),
        fonts: vec![FontPackageEntry {
            asset_id: "asset:/font/ui/poppins-regular".into(),
            uri: VfsUri::parse("package:/asset/font/ui").unwrap(),
            family: "Poppins".into(),
            face_index: 0,
            hash: font_hash,
            license_id: "OFL-1.1".into(),
            subset: None,
            coverage: vec![UnicodeRange {
                start: 0,
                end: 0x036f,
            }],
            targets: vec!["native-smoke-game".into()],
            profiles: vec!["classic".into()],
        }],
    };
    build.extra_sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&manifest).unwrap(),
    ));
    let blob = PackageBuilder::build(build).unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    let provider = CosmicTextLayoutProvider::from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "native-smoke-game".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    assert!(!provider
        .layout(&request("package font"))
        .unwrap()
        .glyph_resources
        .is_empty());

    let wrong_context = CosmicTextLayoutProvider::from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "other-game".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .err()
    .unwrap();
    assert!(wrong_context
        .to_string()
        .contains("ASTRA_TEXT_PACKAGE_MANIFEST_IDENTITY"));
}

#[astra_headless_test::test]
fn multiscript_fallback_database_is_loaded_from_verified_package_sections() {
    let mut fonts = multiscript_fonts();
    for font in &mut fonts {
        font.targets = vec!["native-smoke-game".into()];
    }
    let mut sections = Vec::new();
    let mut entries = Vec::new();
    let mut manifest_fonts = Vec::new();
    for (index, font) in fonts.iter().enumerate() {
        let section_id = format!("asset.font.multiscript.{index}");
        let uri = VfsUri::parse(&format!("package:/font/multiscript/{index}")).unwrap();
        sections.push(SectionPayload::raw(
            section_id.clone(),
            "astra.cooked_asset.v1",
            font.bytes.clone(),
        ));
        entries.push(serde_json::json!({
            "vfs_uri": uri,
            "layer_id": "package.base",
            "source": { "kind": "package_section", "section_id": section_id },
            "offset": 0,
            "size": font.bytes.len(),
            "hash": font.hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }));
        manifest_fonts.push(FontPackageEntry {
            asset_id: font.asset_id.clone(),
            uri,
            family: font.family.clone(),
            face_index: font.face_index,
            hash: font.hash,
            license_id: font.license_id.clone(),
            subset: font.subset.clone(),
            coverage: font.coverage.clone(),
            targets: font.targets.clone(),
            profiles: font.profiles.clone(),
        });
    }
    let mut build =
        PackageBuildRequest::fixture("com.example.multiscript-fonts", "classic", sections);
    build.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": { "kind": "package_section", "section_id": "package.manifest" },
            "targets": ["native-smoke-game"],
            "profiles": ["classic"]
        }],
        "entries": entries,
        "whiteouts": []
    }))
    .unwrap();
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.into(),
        target: "native-smoke-game".into(),
        profile: "classic".into(),
        provider_binding: "astra.vfs.package".into(),
        fonts: manifest_fonts,
    };
    build.extra_sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&manifest).unwrap(),
    ));
    let blob = PackageBuilder::build(build).unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    let provider = CosmicTextLayoutProvider::from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "native-smoke-game".into(),
            profile: "classic".into(),
            default_locale: "und".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let mut mixed = request("A中ا😀");
    mixed.runs[0].language = "und".into();
    mixed.runs[0].script = None;
    mixed.runs[0].direction = TextDirection::Auto;
    mixed.font_families = vec![
        "Poppins".into(),
        "Noto Sans SC".into(),
        "Noto Sans Arabic".into(),
        "Noto Emoji".into(),
    ];
    let layout = provider.layout(&mixed).unwrap();
    let families = layout
        .shaped_runs
        .iter()
        .map(|run| run.font_family.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(families.len(), 4);
    assert_eq!(layout.hash, provider.layout(&mixed).unwrap().hash);
}

#[astra_headless_test::test]
fn layout_snapshot_restore_and_provider_free_replay_are_deterministic() {
    let provider = provider();
    let binding = replay_binding(&provider);
    let limits = TextLayoutReplayLimits::production_defaults();
    let first = request("first e\u{301} line");
    let second = request("second deterministic line");

    let mut uninterrupted = TextLayoutReplaySession::live(binding.clone(), limits).unwrap();
    let first_layout = uninterrupted.record_live(&provider, &first).unwrap();
    let checkpoint = uninterrupted.snapshot().unwrap();
    let second_layout = uninterrupted.record_live(&provider, &second).unwrap();
    let uninterrupted_hash = uninterrupted.transcript_hash();

    let mut restored =
        TextLayoutReplaySession::restore_live(&checkpoint, &binding, limits.max_snapshot_bytes)
            .unwrap();
    assert_eq!(
        restored.record_live(&provider, &second).unwrap(),
        second_layout
    );
    assert_eq!(restored.transcript_hash(), uninterrupted_hash);

    let transcript = restored.snapshot().unwrap();
    let mut replay =
        TextLayoutReplaySession::restore_replay(&transcript, &binding, limits.max_snapshot_bytes)
            .unwrap();
    let first_request_hash = provider.request_hash(&first).unwrap();
    let second_request_hash = provider.request_hash(&second).unwrap();
    assert_eq!(
        replay
            .replay_next(TextLayoutReplayInput {
                sequence: 1,
                request_hash: first_request_hash,
            })
            .unwrap(),
        first_layout
    );
    let replay_checkpoint = replay.snapshot().unwrap();
    let mut replay = TextLayoutReplaySession::restore_replay(
        &replay_checkpoint,
        &binding,
        limits.max_snapshot_bytes,
    )
    .unwrap();
    assert_eq!(replay.replayed_layouts(), 1);
    assert_eq!(
        replay
            .replay_next(TextLayoutReplayInput {
                sequence: 2,
                request_hash: second_request_hash,
            })
            .unwrap(),
        second_layout
    );
    assert_eq!(replay.replayed_layouts(), 2);
    assert!(replay
        .replay_next(TextLayoutReplayInput {
            sequence: 3,
            request_hash: second_request_hash,
        })
        .is_err());
}

#[astra_headless_test::test]
fn layout_replay_blocks_request_provider_and_payload_drift_without_advancing() {
    let provider = provider();
    let binding = replay_binding(&provider);
    let limits = TextLayoutReplayLimits::production_defaults();
    let request = request("recorded layout");
    let request_hash = provider.request_hash(&request).unwrap();
    let mut live = TextLayoutReplaySession::live(binding.clone(), limits).unwrap();
    let expected = live.record_live(&provider, &request).unwrap();
    let bytes = live.snapshot().unwrap();

    let mut replay =
        TextLayoutReplaySession::restore_replay(&bytes, &binding, limits.max_snapshot_bytes)
            .unwrap();
    let error = replay
        .replay_next(TextLayoutReplayInput {
            sequence: 1,
            request_hash: Hash256::from_sha256(b"different request"),
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_TEXT_REPLAY_REQUEST_DRIFT"));
    assert_eq!(replay.replayed_layouts(), 0);
    assert_eq!(
        replay
            .replay_next(TextLayoutReplayInput {
                sequence: 1,
                request_hash,
            })
            .unwrap(),
        expected
    );

    let mut drifted_binding = binding.clone();
    drifted_binding.provider_fingerprint = Hash256::from_sha256(b"changed provider");
    let error =
        TextLayoutReplaySession::restore_live(&bytes, &drifted_binding, limits.max_snapshot_bytes)
            .err()
            .unwrap();
    assert!(error.to_string().contains("ASTRA_TEXT_PROVIDER_DRIFT"));

    let mut decoded: TextLayoutReplaySnapshot = postcard::from_bytes(&bytes).unwrap();
    std::sync::Arc::make_mut(&mut decoded.records[0].layout.glyph_resources[0].bitmap.pixels)[0] ^=
        0xff;
    let tampered = postcard::to_allocvec(&decoded).unwrap();
    let error =
        TextLayoutReplaySession::restore_replay(&tampered, &binding, limits.max_snapshot_bytes)
            .err()
            .unwrap();
    assert!(error.to_string().contains("ASTRA_TEXT_REPLAY_RECORD"));
}

#[astra_headless_test::test]
fn layout_replay_enforces_record_and_snapshot_budgets_transactionally() {
    let provider = provider();
    let binding = replay_binding(&provider);
    let limits = TextLayoutReplayLimits {
        max_records: 1,
        max_snapshot_bytes: 64 * 1024 * 1024,
        max_glyph_resources_per_record: 65_536,
        max_glyph_bytes_per_record: 64 * 1024 * 1024,
    };
    let mut live = TextLayoutReplaySession::live(binding, limits).unwrap();
    live.record_live(&provider, &request("bounded record"))
        .unwrap();
    let before = live.transcript_hash();
    let error = live
        .record_live(&provider, &request("overflow record"))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_TEXT_REPLAY_RECORD_BUDGET"));
    assert_eq!(live.recorded_layouts(), 1);
    assert_eq!(live.transcript_hash(), before);
}
