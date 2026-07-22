use astra_core::Hash256;
use astra_media_core::{BlendMode, GlyphBitmap, GlyphBitmapFormat, GlyphInstance, SceneCommand};
use astra_platform::CapturedFrame;
use astra_player_core::{
    PlayerPlatform, PlayerPresentationReport, PlayerPresentationRunIdentity,
    PLAYER_PRESENTATION_REPORT_SCHEMA,
};

fn identity() -> PlayerPresentationRunIdentity {
    PlayerPresentationRunIdentity {
        target: "native-smoke-game".into(),
        profile: "classic".into(),
        platform: PlayerPlatform::Windows,
        package_hash: Hash256::from_sha256(b"package").to_string(),
        profile_hash: Hash256::from_sha256(b"profile").to_string(),
        build_fingerprint: Hash256::from_sha256(b"build").to_string(),
        session_id: "session.presentation.1".into(),
        renderer_provider: "wgpu_hardware".into(),
        presentation_path: "glyph_atlas".into(),
        font_provider_hash: Hash256::from_sha256(b"font-provider").to_string(),
    }
}

fn commands() -> Vec<SceneCommand> {
    let pixels = vec![255_u8; 4];
    vec![
        SceneCommand::UploadGlyph {
            resource_id: "glyph:test".into(),
            glyph: GlyphBitmap {
                width: 2,
                height: 2,
                format: GlyphBitmapFormat::Alpha8,
                hash: Hash256::from_sha256(&pixels),
                pixels: pixels.into(),
            },
        },
        SceneCommand::GlyphRun {
            id: "layout:test".into(),
            glyphs: vec![GlyphInstance {
                resource_id: "glyph:test".into(),
                x: 0,
                y: 0,
                rotation_quadrants: 0,
            }]
            .into(),
            rgba: [255; 4],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
    ]
}

fn present() -> astra_player_core::PlayerHostCommand {
    astra_player_core::PlayerHostCommand::PresentScene {
        sequence: 1,
        surface: astra_player_core::PlayerHostResourceId(1),
        width: 2,
        height: 2,
        clear_rgba: [0; 4],
        semantics: None,
        commands: commands(),
    }
}

#[astra_headless_test::test]
fn live_presentation_report_binds_command_and_capture_identity() {
    let report = PlayerPresentationReport::from_live_capture(
        identity(),
        Hash256::from_sha256(b"layout"),
        &present(),
        &CapturedFrame {
            width: 2,
            height: 2,
            rgba8: vec![255; 16].into(),
        },
        [0; 4],
    )
    .unwrap();
    assert_eq!(report.schema, PLAYER_PRESENTATION_REPORT_SCHEMA);
    assert_eq!(report.changed_pixels, 4);
    assert!(report.command_hash.starts_with("sha256:"));
    assert!(report.capture_hash.starts_with("sha256:"));
}

#[astra_headless_test::test]
fn presentation_report_rejects_headless_identity_and_empty_visual_output() {
    let mut headless = identity();
    headless.renderer_provider = "astra.renderer.headless".into();
    assert!(PlayerPresentationReport::from_live_capture(
        headless,
        Hash256::from_sha256(b"layout"),
        &present(),
        &CapturedFrame {
            width: 2,
            height: 2,
            rgba8: vec![255; 16].into(),
        },
        [0; 4],
    )
    .is_err());

    assert!(PlayerPresentationReport::from_live_capture(
        identity(),
        Hash256::from_sha256(b"layout"),
        &present(),
        &CapturedFrame {
            width: 2,
            height: 2,
            rgba8: vec![0; 16].into(),
        },
        [0; 4],
    )
    .is_err());
}
