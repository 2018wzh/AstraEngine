use std::collections::BTreeMap;

use astra_media_core::{
    BlendMode, Canvas2D, Extent2D, FilterGraph, FilterNode, FilterParam, FilterTarget, RectI,
    SceneCommand, TextureFrame, Transform2D,
};
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn native_gpu_preserves_aspect_fit_bars_for_arbitrary_rasters() {
    let mut renderer = WgpuOffscreenRenderer::new().await.unwrap();
    assert_eq!(renderer.identity().provider, "wgpu_offscreen");
    assert_ne!(renderer.identity().device_type, "cpu");
    assert!(matches!(
        renderer.identity().device_type.as_str(),
        "integrated_gpu" | "discrete_gpu"
    ));
    assert_eq!(
        renderer.identity().backend,
        if cfg!(target_os = "windows") {
            "dx12"
        } else if cfg!(target_os = "linux") {
            "vulkan"
        } else {
            "metal"
        }
    );

    for (sequence, raster) in [
        Extent2D::new(1600, 900),
        Extent2D::new(1920, 1200),
        Extent2D::new(1001, 777),
        Extent2D::new(720, 1280),
        Extent2D::new(640, 360),
        Extent2D::new(96, 54),
        Extent2D::new(1, 720),
        Extent2D::new(720, 1),
        Extent2D::new(1279, 719),
    ]
    .into_iter()
    .enumerate()
    {
        let canvas = Canvas2D::new(Extent2D::new(1280, 720), raster).unwrap();
        renderer
            .validate_output_extent(raster.width, raster.height)
            .unwrap();
        let viewport = canvas.viewport();
        let commands = vec![
            SceneCommand::PushClip {
                rect: canvas.viewport_rect().unwrap(),
            },
            SceneCommand::PushTransform {
                transform: canvas.logical_to_raster_transform(),
            },
            SceneCommand::rect("full.logical", 0, 0, 1280, 720, [255, 255, 255, 255]),
            // A displaced oversized primitive models sprite crop plus shake;
            // The raster viewport clip must still keep it out of the bars.
            SceneCommand::PushTransform {
                transform: Transform2D::translation(40.0, 40.0),
            },
            SceneCommand::Texture {
                id: "shaken.oversized".into(),
                frame: TextureFrame::from_vec(1, 1, vec![255, 0, 0, 255]).unwrap(),
                destination: RectI::new(-200, -200, 1800, 1200),
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
            SceneCommand::PopTransform,
            SceneCommand::PopClip,
            SceneCommand::PopTransform,
        ];
        let capture = renderer
            .render(&SceneFrame {
                sequence: sequence as u64 + 1,
                width: raster.width,
                height: raster.height,
                clear_rgba: [0, 0, 0, 255],
                commands,
                semantics: None,
            })
            .unwrap();
        assert_eq!(
            (capture.width, capture.height),
            (raster.width, raster.height)
        );
        assert_eq!(
            capture.rgba8.len(),
            (raster.width * raster.height * 4) as usize
        );
        let pixels = capture.rgba8.as_chunks::<4>().0;
        for (index, pixel) in pixels.iter().enumerate() {
            let x = (index as u32) % raster.width;
            let y = (index as u32) / raster.width;
            let inside = x >= viewport.x
                && x < viewport.x + viewport.width
                && y >= viewport.y
                && y < viewport.y + viewport.height;
            if inside {
                assert_ne!(
                    *pixel,
                    [0, 0, 0, 255],
                    "logical content vanished at {x},{y} for {}x{}",
                    raster.width,
                    raster.height
                );
            } else {
                assert_eq!(
                    *pixel,
                    [0, 0, 0, 255],
                    "letterbox painted at {x},{y} for {}x{}",
                    raster.width,
                    raster.height
                );
            }
        }

        // The filter operates on the complete raster. Black bars must remain
        // black while the logical content changes, proving the viewport is
        // preserved through the Manager's shared filter path.
        let mut params = BTreeMap::new();
        params.insert("amount".to_owned(), FilterParam::Float(0.5));
        let filtered = renderer
            .render(&SceneFrame {
                sequence: sequence as u64 + 100,
                width: raster.width,
                height: raster.height,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![
                    SceneCommand::PushClip {
                        rect: canvas.viewport_rect().unwrap(),
                    },
                    SceneCommand::PushTransform {
                        transform: canvas.logical_to_raster_transform(),
                    },
                    SceneCommand::rect("filtered.logical", 0, 0, 1280, 720, [255; 4]),
                    SceneCommand::PopTransform,
                    SceneCommand::PopClip,
                    SceneCommand::FilterGraph {
                        graph: FilterGraph {
                            schema: "astra.filter_graph.v1".into(),
                            nodes: vec![FilterNode {
                                id: "fade".into(),
                                kind: "astra.filter.fade".into(),
                                input: FilterTarget::Final,
                                output: FilterTarget::Final,
                                params,
                                deterministic: true,
                                allow_cpu_fallback: false,
                            }],
                        },
                    },
                ],
                semantics: None,
            })
            .unwrap();
        let filtered_pixels = filtered.rgba8.as_chunks::<4>().0;
        for (index, pixel) in filtered_pixels.iter().enumerate() {
            let x = (index as u32) % raster.width;
            let y = (index as u32) / raster.width;
            if x < viewport.x
                || x >= viewport.x + viewport.width
                || y < viewport.y
                || y >= viewport.y + viewport.height
            {
                assert_eq!(*pixel, [0, 0, 0, 255], "filter painted a bar at {x},{y}");
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn native_gpu_keeps_tiny_root_and_fractional_nested_clip_edges() {
    let mut renderer = WgpuOffscreenRenderer::new().await.unwrap();
    assert_ne!(renderer.identity().device_type, "cpu");

    let tiny = Canvas2D::new(Extent2D::new(1280, 720), Extent2D::new(1, 1)).unwrap();
    let tiny_frame = renderer
        .render(&SceneFrame {
            sequence: 1,
            width: 1,
            height: 1,
            clear_rgba: [0, 0, 0, 255],
            commands: vec![
                SceneCommand::PushClip {
                    rect: tiny.viewport_rect().unwrap(),
                },
                SceneCommand::PushTransform {
                    transform: tiny.logical_to_raster_transform(),
                },
                SceneCommand::rect("tiny.logical", 0, 0, 1280, 720, [255, 0, 0, 255]),
                SceneCommand::PopTransform,
                SceneCommand::PopClip,
            ],
            semantics: None,
        })
        .unwrap();
    assert_eq!(tiny_frame.rgba8.as_ref(), &[255, 0, 0, 255]);

    let fractional = Transform2D::translation(0.25, 0.25);
    let nested_frame = renderer
        .render(&SceneFrame {
            sequence: 2,
            width: 4,
            height: 4,
            clear_rgba: [0, 0, 0, 255],
            commands: vec![
                SceneCommand::PushClip {
                    rect: RectI::new(0, 0, 4, 4),
                },
                SceneCommand::PushTransform {
                    transform: fractional,
                },
                SceneCommand::PushClip {
                    rect: RectI::new(0, 0, 2, 2),
                },
                SceneCommand::rect("fractional.clip", 0, 0, 4, 4, [0, 255, 0, 255]),
                SceneCommand::PopClip,
                SceneCommand::PopTransform,
                SceneCommand::PopClip,
            ],
            semantics: None,
        })
        .unwrap();
    let pixels = nested_frame.rgba8.as_chunks::<4>().0;
    assert_eq!(pixels[0], [0, 255, 0, 255]);
    assert_eq!(pixels[1], [0, 255, 0, 255]);
    assert_eq!(pixels[2], [0, 255, 0, 255]);
    assert_eq!(pixels[3], [0, 0, 0, 255]);
    assert_eq!(pixels[4], [0, 255, 0, 255]);
    assert_eq!(pixels[5], [0, 255, 0, 255]);
    assert_eq!(pixels[6], [0, 255, 0, 255]);
    assert_eq!(pixels[7], [0, 0, 0, 255]);
    assert_eq!(pixels[8], [0, 255, 0, 255]);
    assert_eq!(pixels[9], [0, 255, 0, 255]);
    assert_eq!(pixels[10], [0, 255, 0, 255]);
    assert_eq!(pixels[11], [0, 0, 0, 255]);
}
