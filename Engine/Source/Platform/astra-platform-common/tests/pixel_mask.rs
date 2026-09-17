use astra_media_core::SceneCommand;
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn pixel_mask_tiles_intersects_and_restores_without_geometry_growth() {
    let mut renderer = WgpuOffscreenRenderer::new().await.unwrap();
    assert!(matches!(
        renderer.identity().device_type.as_str(),
        "discrete_gpu" | "integrated_gpu"
    ));
    let mut frame = SceneFrame {
        sequence: 1,
        width: 16,
        height: 16,
        clear_rgba: [0, 0, 0, 255],
        commands: vec![],
        semantics: None,
    };
    for (sequence, bits) in [0, 0x8000000000000001, 0xaa55aa55aa55aa55, u64::MAX]
        .into_iter()
        .enumerate()
    {
        frame.sequence = sequence as u64 + 1;
        frame.commands = vec![
            SceneCommand::PushPixelMask { bits },
            SceneCommand::rect("masked", 0, 0, 16, 16, [255, 255, 255, 255]),
            SceneCommand::PopPixelMask,
        ];
        let capture = renderer.render(&frame).unwrap();
        for y in 0..16usize {
            for x in 0..16usize {
                let on = bits & (1 << (63 - (y % 8 * 8 + x % 8))) != 0;
                let expected = if on {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(
                    &capture.rgba8[(y * 16 + x) * 4..][..4],
                    &expected,
                    "pixel {x},{y}"
                );
            }
        }
    }
    frame.sequence += 1;
    frame.commands = vec![
        SceneCommand::PushPixelMask {
            bits: 0xff00000000000000,
        },
        SceneCommand::PushPixelMask {
            bits: 0x8080808080808080,
        },
        SceneCommand::rect("intersection", 0, 0, 16, 16, [255, 0, 0, 255]),
        SceneCommand::PopPixelMask,
        SceneCommand::rect("restored-parent", 1, 0, 1, 16, [0, 255, 0, 255]),
        SceneCommand::PopPixelMask,
        SceneCommand::rect("outside", 2, 2, 1, 1, [0, 0, 255, 255]),
    ];
    let capture = renderer.render(&frame).unwrap();
    assert_eq!(&capture.rgba8[..4], &[255, 0, 0, 255]);
    assert_eq!(&capture.rgba8[4..8], &[0, 255, 0, 255]);
    assert_eq!(&capture.rgba8[64..68], &[0, 0, 0, 255]);
    assert_eq!(&capture.rgba8[(2 * 16 + 2) * 4..][..4], &[0, 0, 255, 255]);
    frame.sequence += 1;
    frame.commands = vec![SceneCommand::PopPixelMask];
    assert!(renderer.render(&frame).is_err());
    frame.sequence += 1;
    frame.commands = vec![SceneCommand::PushPixelMask { bits: u64::MAX }];
    assert!(renderer.render(&frame).is_err());
    frame.sequence += 1;
    frame.width = 2560;
    frame.height = 1440;
    frame.commands = vec![SceneCommand::PushPixelMask {
        bits: 0xaa55aa55aa55aa55,
    }];
    for layer in 0..8 {
        frame.commands.push(SceneCommand::rect(
            format!("layer.{layer}"),
            0,
            0,
            2560,
            1440,
            [255, 255, 255, 255],
        ));
    }
    frame.commands.push(SceneCommand::PopPixelMask);
    renderer.render(&frame).unwrap();
    assert!(renderer.performance_counters().gpu_resource_bytes < 128 * 1024 * 1024);
}
