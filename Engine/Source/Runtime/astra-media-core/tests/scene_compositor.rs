use astra_core::Hash256;
use astra_media_core::{
    BlendMode, CpuRendererProvider, DrawCommand, GlyphBitmap, GlyphBitmapFormat, MeshMaterial2D,
    MeshVertex2D, RectI, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
    TextureFrame, Transform2D,
};

#[astra_headless_test::test]
fn cpu_reference_compositor_screens_premultiplied_ui_meshes() {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 1,
            height: 1,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "mesh-screen".into(),
        })
        .unwrap();
    let vertex = |position| MeshVertex2D {
        position,
        uv: [0.0, 0.0],
        premultiplied_rgba: [128, 64, 32, 128],
    };
    let frame = renderer
        .capture_frame(&[
            DrawCommand::clear([64, 128, 192, 255]),
            DrawCommand::Mesh2D {
                id: "screen-mesh".into(),
                vertices: vec![vertex([0.0, 0.0]), vertex([2.0, 0.0]), vertex([0.0, 2.0])],
                indices: vec![0, 1, 2],
                material: MeshMaterial2D::Solid,
                texture_id: None,
                opacity: 1.0,
                blend: BlendMode::Screen,
            },
        ])
        .unwrap();
    assert_eq!(frame.bytes, vec![160, 160, 200, 255]);
}

#[astra_headless_test::test]
fn cpu_reference_compositor_executes_texture_glyph_clip_transform_and_blend() {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 8,
            height: 8,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "golden".into(),
        })
        .unwrap();
    let texture_bytes = vec![255, 0, 0, 255, 0, 255, 0, 128];
    let texture = TextureFrame {
        width: 2,
        height: 1,
        hash: Hash256::from_sha256(&texture_bytes),
        rgba8: texture_bytes,
    };
    let commands = vec![
        DrawCommand::clear([0, 0, 32, 255]),
        DrawCommand::PushClip {
            rect: RectI::new(2, 2, 4, 4),
        },
        DrawCommand::PushTransform {
            transform: Transform2D::translation(2.0, 2.0),
        },
        DrawCommand::Texture {
            id: "sprite".into(),
            frame: texture,
            destination: RectI::new(0, 0, 4, 2),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        DrawCommand::Glyph {
            id: "glyph".into(),
            glyph: GlyphBitmap {
                width: 2,
                height: 2,
                format: GlyphBitmapFormat::Alpha8,
                pixels: vec![255, 0, 0, 255],
                hash: Hash256::from_sha256(&[255, 0, 0, 255]),
            },
            x: 0,
            y: 2,
            rgba: [255, 255, 255, 255],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        DrawCommand::PopTransform,
        DrawCommand::PopClip,
    ];

    let frame = renderer.capture_frame(&commands).unwrap();
    let pixel = |x: usize, y: usize| &frame.bytes[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4];
    assert_eq!(pixel(2, 2), &[255, 0, 0, 255]);
    assert_eq!(pixel(5, 2), &[0, 128, 16, 255]);
    assert_eq!(pixel(2, 4), &[255, 255, 255, 255]);
    assert_eq!(pixel(3, 4), &[0, 0, 32, 255]);
    assert_eq!(pixel(1, 1), &[0, 0, 32, 255]);
}

#[astra_headless_test::test]
fn scene_resources_are_uploaded_reused_cropped_and_released_explicitly() {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 2,
            height: 1,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "golden".into(),
        })
        .unwrap();
    let rgba8 = vec![255, 0, 0, 255, 0, 255, 0, 255];
    let frame = TextureFrame {
        width: 2,
        height: 1,
        hash: Hash256::from_sha256(&rgba8),
        rgba8,
    };
    let rendered = renderer
        .capture_frame(&[
            DrawCommand::UploadTexture {
                resource_id: "atlas".into(),
                frame,
            },
            DrawCommand::Sprite {
                id: "right".into(),
                texture_id: "atlas".into(),
                source: Some(RectI::new(1, 0, 1, 1)),
                destination: RectI::new(0, 0, 2, 1),
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
        ])
        .unwrap();
    assert_eq!(rendered.bytes, vec![0, 255, 0, 255, 0, 255, 0, 255]);
    renderer
        .capture_frame(&[DrawCommand::ReleaseResource {
            resource_id: "atlas".into(),
        }])
        .unwrap();
    let missing = renderer
        .capture_frame(&[DrawCommand::Sprite {
            id: "missing".into(),
            texture_id: "atlas".into(),
            source: None,
            destination: RectI::new(0, 0, 1, 1),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        }])
        .unwrap_err();
    assert!(missing.to_string().contains("ASTRA_MEDIA_RESOURCE_UNKNOWN"));
}

#[astra_headless_test::test]
fn compositor_blocks_corrupt_texture_and_unbalanced_state() {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 2,
            height: 2,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "golden".into(),
        })
        .unwrap();
    let corrupt = TextureFrame {
        width: 1,
        height: 1,
        rgba8: vec![1, 2, 3, 4],
        hash: Hash256::from_sha256(b"wrong"),
    };
    assert!(renderer
        .capture_frame(&[DrawCommand::Texture {
            id: "corrupt".into(),
            frame: corrupt,
            destination: RectI::new(0, 0, 1, 1),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        }])
        .unwrap_err()
        .to_string()
        .contains("ASTRA_MEDIA_TEXTURE_HASH"));
    assert!(renderer
        .capture_frame(&[DrawCommand::PopClip])
        .unwrap_err()
        .to_string()
        .contains("ASTRA_MEDIA_CLIP_STACK"));
}

#[astra_headless_test::test]
fn resource_updates_are_transactional_and_color_glyphs_preserve_rgba() {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 1,
            height: 1,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "golden".into(),
        })
        .unwrap();
    let color = vec![200, 100, 50, 128];
    let glyph = GlyphBitmap {
        width: 1,
        height: 1,
        format: GlyphBitmapFormat::Rgba8,
        hash: Hash256::from_sha256(&color),
        pixels: color,
    };
    let failure = renderer
        .capture_frame(&[
            DrawCommand::UploadGlyph {
                resource_id: "color".into(),
                glyph: glyph.clone(),
            },
            DrawCommand::PopClip,
        ])
        .unwrap_err();
    assert!(failure.to_string().contains("ASTRA_MEDIA_CLIP_STACK"));
    let missing = renderer
        .capture_frame(&[DrawCommand::GlyphRun {
            id: "must-not-exist".into(),
            glyphs: vec![astra_media_core::GlyphInstance {
                resource_id: "color".into(),
                x: 0,
                y: 0,
                rotation_quadrants: 0,
            }],
            rgba: [255; 4],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        }])
        .unwrap_err();
    assert!(missing.to_string().contains("ASTRA_MEDIA_RESOURCE_UNKNOWN"));

    let frame = renderer
        .capture_frame(&[
            DrawCommand::UploadGlyph {
                resource_id: "color".into(),
                glyph,
            },
            DrawCommand::GlyphRun {
                id: "color-run".into(),
                glyphs: vec![astra_media_core::GlyphInstance {
                    resource_id: "color".into(),
                    x: 0,
                    y: 0,
                    rotation_quadrants: 0,
                }],
                rgba: [255; 4],
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
        ])
        .unwrap();
    assert_eq!(frame.bytes, vec![200, 100, 50, 128]);
}
