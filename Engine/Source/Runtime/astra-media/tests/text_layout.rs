use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, FontPackageEntry, FontPackageManifest, GlyphRole,
    LayoutConstraint, OpenTypeFeature, OverflowPolicy, PackagedFont, RubySpan, SourceRange,
    TextDirection, TextLayoutConfig, TextLayoutProvider, TextLayoutRequest,
    TextRenderResourceOwner, TextRun, UnicodeRange, VoiceReplayRef, WrapPolicy,
    FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_media_core::{
    HeadlessRendererProvider, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
    SceneCommand,
};
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};

fn open_font_fixture(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainFonts")
        .join(file);
    std::fs::read(path).unwrap()
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

#[test]
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

    let mut renderer = HeadlessRendererProvider
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
    conflicting.glyph_resources[0].bitmap.pixels[0] ^= 0xff;
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

#[test]
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

#[test]
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

#[test]
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

#[test]
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

#[test]
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

#[test]
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
