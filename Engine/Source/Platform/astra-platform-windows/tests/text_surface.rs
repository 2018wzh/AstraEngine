#![cfg(target_os = "windows")]

use astra_core::Hash256;
use astra_media::{
    BlendMode, CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy,
    PackagedFont, RectI, SceneCommand, TextDirection, TextLayoutConfig, TextLayoutProvider,
    TextLayoutRequest, TextRenderResourceOwner, TextRun, TextureFrame, Transform2D, UnicodeRange,
    WrapPolicy,
};
use astra_platform::{
    PlatformErrorCode, PlatformHostClient, PlatformHostFactory, PlatformHostProfile, SceneFrame,
    SurfaceHandle, SurfaceRequest, WindowRequest,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 180;
const BACKGROUND: [u8; 4] = [12, 18, 28, 255];

fn font(asset_id: &str, family: &str, file: &str, coverage: Vec<UnicodeRange>) -> PackagedFont {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../Fixtures/PublicDomainFonts")
            .join(file),
    )
    .unwrap();
    PackagedFont {
        asset_id: asset_id.into(),
        family: family.into(),
        face_index: 0,
        hash: Hash256::from_sha256(&bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage,
        targets: vec!["nativevn-game".into()],
        profiles: vec!["classic".into()],
        bytes,
    }
}

fn provider() -> CosmicTextLayoutProvider {
    CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "nativevn-game".into(),
            profile: "classic".into(),
            default_locale: "und".into(),
        },
        vec![
            font(
                "asset:/font/fallback/noto-sans-sc",
                "Noto Sans SC",
                "NotoSansSC-Variable.ttf",
                vec![
                    UnicodeRange {
                        start: 0x20,
                        end: 0x7e,
                    },
                    UnicodeRange {
                        start: 0x3000,
                        end: 0x30ff,
                    },
                    UnicodeRange {
                        start: 0x3400,
                        end: 0x9fff,
                    },
                ],
            ),
            font(
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
            font(
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
        ],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap()
}

fn request() -> TextLayoutRequest {
    TextLayoutRequest {
        key: "windows.gpu.multiscript".into(),
        runs: vec![TextRun {
            text: "Astra 字形 日本語 العربية 😀".into(),
            language: "und".into(),
            script: None,
            direction: TextDirection::Auto,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: 600.0,
            max_height: Some(140.0),
            max_lines: Some(2),
            font_size: 36.0,
            line_height: 48.0,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::Clip,
        },
        font_families: vec![
            "Noto Sans SC".into(),
            "Noto Sans Arabic".into(),
            "Noto Emoji".into(),
        ],
        features: Vec::new(),
    }
}

#[tokio::test]
async fn windows_wgpu_renders_multiscript_layout_through_live_glyph_atlas() {
    let golden: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../Fixtures/PublicDomainFonts/windows-glyph-golden.json"
    ))
    .unwrap();
    let font_manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../Fixtures/PublicDomainFonts/manifest.json"
    ))
    .unwrap();
    assert_eq!(golden["schema"], "astra.windows_gpu_glyph_golden.v1");
    assert_eq!(golden["renderer"], "wgpu_hardware.glyph_atlas");
    assert_eq!(golden["font_revision"], font_manifest["upstream_revision"]);
    assert_eq!(golden["width"], WIDTH);
    assert_eq!(golden["height"], HEIGHT);
    assert_eq!(golden["background_rgba"], serde_json::json!(BACKGROUND));
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.text");
    let host = astra_platform_windows::factory()
        .start(profile)
        .await
        .unwrap();
    let window = host
        .client
        .create_window(WindowRequest {
            title: "Astra Text Golden".into(),
            width: WIDTH,
            height: HEIGHT,
            visible: true,
        })
        .await
        .unwrap();
    let surface = host
        .client
        .create_surface(SurfaceRequest {
            window,
            width: WIDTH,
            height: HEIGHT,
        })
        .await
        .unwrap();

    let layout = provider().layout(&request()).unwrap();
    assert!(layout.shaped_runs.len() >= 3);
    assert_eq!(golden["layout_hash"], layout.hash.to_string());
    let mut resources = TextRenderResourceOwner::default();
    let commands = resources
        .update_layout("golden.multiscript", &layout, [238, 242, 255, 255])
        .unwrap();
    let duplicate_upload = commands
        .iter()
        .find(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
        .cloned()
        .unwrap();
    host.client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 1,
                width: WIDTH,
                height: HEIGHT,
                clear_rgba: BACKGROUND,
                commands,
            },
        )
        .await
        .unwrap();
    let captured = host.client.capture_surface(surface).await.unwrap();
    let changed_pixels = captured
        .rgba8
        .chunks_exact(4)
        .filter(|pixel| *pixel != BACKGROUND)
        .count();
    assert!(changed_pixels >= golden["minimum_changed_pixels"].as_u64().unwrap() as usize);
    let capture_hash = Hash256::from_sha256(&captured.rgba8);
    assert_eq!(golden["capture_hash"], capture_hash.to_string());

    host.client
        .inject_surface_device_loss(surface)
        .await
        .unwrap();
    let retained_commands = resources
        .update_layout("golden.multiscript", &layout, [238, 242, 255, 255])
        .unwrap();
    let lost = host
        .client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 2,
                width: WIDTH,
                height: HEIGHT,
                clear_rgba: BACKGROUND,
                commands: retained_commands.clone(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(lost.code, PlatformErrorCode::DeviceLost);
    host.client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 2,
                width: WIDTH,
                height: HEIGHT,
                clear_rgba: BACKGROUND,
                commands: retained_commands,
            },
        )
        .await
        .unwrap();
    let recovered = host.client.capture_surface(surface).await.unwrap();
    assert_eq!(Hash256::from_sha256(&recovered.rgba8), capture_hash);

    let duplicate = host
        .client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 3,
                width: WIDTH,
                height: HEIGHT,
                clear_rgba: BACKGROUND,
                commands: vec![duplicate_upload],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(duplicate.code, PlatformErrorCode::InvalidState);
    let after_failed_upload = host.client.capture_surface(surface).await.unwrap();
    assert_eq!(
        Hash256::from_sha256(&after_failed_upload.rgba8),
        capture_hash
    );

    let unsupported = host
        .client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 3,
                width: WIDTH,
                height: HEIGHT,
                clear_rgba: BACKGROUND,
                commands: vec![SceneCommand::PushTransform {
                    transform: Transform2D::IDENTITY,
                }],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(unsupported.code, PlatformErrorCode::InvalidState);

    let releases = resources.remove_layout("golden.multiscript").unwrap();
    host.client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 3,
                width: WIDTH,
                height: HEIGHT,
                clear_rgba: BACKGROUND,
                commands: releases,
            },
        )
        .await
        .unwrap();
    let cleared = host.client.capture_surface(surface).await.unwrap();
    assert!(cleared
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == BACKGROUND));

    exercise_scene_atlas(&host.client, surface).await;

    host.client.destroy_surface(surface).await.unwrap();
    host.client.destroy_window(window).await.unwrap();
    host.client.shutdown().await.unwrap();
}

async fn exercise_scene_atlas(client: &PlatformHostClient, surface: SurfaceHandle) {
    let texture_bytes = vec![255, 0, 0, 255];
    let upload = SceneCommand::UploadTexture {
        resource_id: "texture:solid-red".into(),
        frame: TextureFrame {
            width: 1,
            height: 1,
            hash: Hash256::from_sha256(&texture_bytes),
            rgba8: texture_bytes,
        },
    };
    let draws = vec![
        SceneCommand::Sprite {
            id: "sprite:red".into(),
            texture_id: "texture:solid-red".into(),
            source: None,
            destination: RectI::new(0, 0, 64, 64),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::rect("rect:green", 64, 0, 64, 64, [0, 255, 0, 255]),
    ];
    let mut first_commands = vec![upload.clone()];
    first_commands.extend(draws.clone());
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 4,
                width: 128,
                height: 64,
                clear_rgba: [0, 0, 0, 255],
                commands: first_commands,
            },
        )
        .await
        .unwrap();
    let first = client.capture_surface(surface).await.unwrap();
    assert_eq!(&first.rgba8[(32 * 128 + 32) * 4..][..4], [255, 0, 0, 255]);
    assert_eq!(&first.rgba8[(32 * 128 + 96) * 4..][..4], [0, 255, 0, 255]);
    let first_hash = Hash256::from_sha256(&first.rgba8);

    client.inject_surface_device_loss(surface).await.unwrap();
    let lost = client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 5,
                width: 128,
                height: 64,
                clear_rgba: [0, 0, 0, 255],
                commands: draws.clone(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(lost.code, PlatformErrorCode::DeviceLost);
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 5,
                width: 128,
                height: 64,
                clear_rgba: [0, 0, 0, 255],
                commands: draws,
            },
        )
        .await
        .unwrap();
    let recovered = client.capture_surface(surface).await.unwrap();
    assert_eq!(Hash256::from_sha256(&recovered.rgba8), first_hash);

    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 6,
                width: 128,
                height: 64,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![SceneCommand::ReleaseResource {
                    resource_id: "texture:solid-red".into(),
                }],
            },
        )
        .await
        .unwrap();
    let cleared = client.capture_surface(surface).await.unwrap();
    assert!(cleared
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == [0, 0, 0, 255]));
}
