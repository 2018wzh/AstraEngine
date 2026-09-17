use astra_media_core::{
    BlendMode, MeshDraw2D, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand, SceneCompositing2D,
    TextureFilter2D, TextureFrame,
};
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn screen_blends_premultiplied_colors_and_opacity_on_hardware() {
    let mut renderer = WgpuOffscreenRenderer::new().await.unwrap();
    assert!(matches!(
        renderer.identity().device_type.as_str(),
        "integrated_gpu" | "discrete_gpu"
    ));
    let cases = [
        ([128, 64, 32, 128], 1.0, [160, 160, 200, 255]),
        ([128, 64, 32, 128], 0.5, [112, 144, 196, 255]),
        ([0, 0, 0, 0], 1.0, [64, 128, 192, 255]),
        ([255, 255, 255, 255], 1.0, [255, 255, 255, 255]),
    ];
    for (index, (color, opacity, expected)) in cases.into_iter().enumerate() {
        let frame = SceneFrame {
            sequence: index as u64 + 1,
            width: 2,
            height: 2,
            clear_rgba: [64, 128, 192, 255],
            commands: vec![SceneCommand::MeshBatch2D {
                vertices: [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]]
                    .map(|position| MeshVertex2D {
                        position,
                        uv: [0.0, 0.0],
                        premultiplied_rgba: color,
                    })
                    .to_vec()
                    .into(),
                indices: vec![0, 1, 2, 2, 1, 3].into(),
                draws: vec![MeshDraw2D {
                    vertex_start: 0,
                    vertex_count: 4,
                    index_start: 0,
                    index_count: 6,
                    material: MeshMaterial2D::Solid,
                    texture_id: None,
                    texture_filter: TextureFilter2D::Nearest,
                    opacity,
                    blend: BlendMode::Screen,
                    scissor: None,
                }]
                .into(),
                compositing: SceneCompositing2D::EncodedSrgb,
            }],
            semantics: None,
        };
        let captured = renderer.render(&frame).unwrap();
        for pixel in captured.rgba8.as_chunks::<4>().0 {
            assert_eq!(*pixel, expected, "case {index}");
        }
    }
}

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn screen_texture_respects_color_space_and_preserves_transparent_pixels() {
    for (compositing, middle_red) in [
        (SceneCompositing2D::LinearSrgb, 192),
        (SceneCompositing2D::EncodedSrgb, 160),
    ] {
        let mut renderer = WgpuOffscreenRenderer::new()
            .await
            .unwrap()
            .with_default_compositing(compositing);
        assert!(matches!(
            renderer.identity().device_type.as_str(),
            "integrated_gpu" | "discrete_gpu"
        ));
        let captured = renderer
            .render(&SceneFrame {
                sequence: 1,
                width: 3,
                height: 1,
                clear_rgba: [64, 128, 192, 255],
                commands: vec![
                    SceneCommand::UploadTexture {
                        resource_id: "screen.texture".into(),
                        frame: TextureFrame {
                            width: 3,
                            height: 1,
                            rgba8: vec![255, 0, 0, 255, 255, 0, 0, 128, 255, 0, 0, 0].into(),
                        },
                    },
                    SceneCommand::Sprite {
                        id: "screen.sprite".into(),
                        texture_id: "screen.texture".into(),
                        source: None,
                        destination: RectI::new(0, 0, 3, 1),
                        opacity: 1.0,
                        blend: BlendMode::Screen,
                    },
                ],
                semantics: None,
            })
            .unwrap();
        assert_eq!(
            captured.rgba8.as_ref(),
            [255, 128, 192, 255, middle_red, 128, 192, 255, 64, 128, 192, 255]
        );
    }
}
