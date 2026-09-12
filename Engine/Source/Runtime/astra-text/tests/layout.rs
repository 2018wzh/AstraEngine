use astra_core::Hash256;
use astra_text::{
    CosmicTextLayoutProvider, FontBindingContext, GlyphRole, LayoutConstraint, OpenTypeFeature,
    OverflowPolicy, PackagedFont, RubySpan, SourceRange, TextDirection, TextLayoutConfig,
    TextLayoutProvider, TextLayoutRequest, TextRenderLayoutUpdate, TextRenderResourceOwner,
    TextRun, UnicodeRange, VoiceReplayRef, WrapPolicy,
};

use astra_media_core::{
    CpuRendererProvider, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
    SceneCommand,
};
use std::sync::{Arc, Barrier};

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

#[path = "layout/resources_and_concurrency.rs"]
mod resources_and_concurrency;

#[path = "layout/layout_cache.rs"]
mod layout_cache;

#[path = "layout/multiscript.rs"]
mod multiscript;

#[path = "layout/validation.rs"]
mod validation;
