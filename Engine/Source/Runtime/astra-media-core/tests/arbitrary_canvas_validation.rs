use astra_media_core::{Canvas2D, Extent2D, Viewport2D};

const LOGICAL: Extent2D = Extent2D {
    width: 1280,
    height: 720,
};

#[test]
fn arbitrary_rasters_keep_exact_extents_and_centered_aspect_fit() {
    let cases = [
        (
            Extent2D::new(1600, 900),
            Viewport2D {
                x: 0,
                y: 0,
                width: 1600,
                height: 900,
            },
        ),
        (
            Extent2D::new(1920, 1200),
            Viewport2D {
                x: 0,
                y: 60,
                width: 1920,
                height: 1080,
            },
        ),
        (
            Extent2D::new(1001, 777),
            Viewport2D {
                x: 0,
                y: 107,
                width: 1001,
                height: 563,
            },
        ),
        (
            Extent2D::new(720, 1280),
            Viewport2D {
                x: 0,
                y: 437,
                width: 720,
                height: 405,
            },
        ),
        (
            Extent2D::new(640, 360),
            Viewport2D {
                x: 0,
                y: 0,
                width: 640,
                height: 360,
            },
        ),
        (
            Extent2D::new(96, 54),
            Viewport2D {
                x: 0,
                y: 0,
                width: 96,
                height: 54,
            },
        ),
        (
            Extent2D::new(1279, 719),
            Viewport2D {
                x: 0,
                y: 0,
                width: 1278,
                height: 719,
            },
        ),
    ];

    for (raster, expected_viewport) in cases {
        let canvas = Canvas2D::new(LOGICAL, raster).expect("positive raster extent");
        assert_eq!(canvas.raster, raster, "raster extent must not be rounded");
        assert_eq!(canvas.viewport(), expected_viewport);

        let center = canvas
            .logical_to_raster_point([640.0, 360.0])
            .expect("logical center maps");
        let expected_center = [
            expected_viewport.x as f32 + expected_viewport.width as f32 / 2.0,
            expected_viewport.y as f32 + expected_viewport.height as f32 / 2.0,
        ];
        // Floor rounding leaves at most one pixel of asymmetry on the right
        // or bottom edge, so the transformed logical center may be fractional
        // relative to the integer viewport center.
        assert!((center[0] - expected_center[0]).abs() < 1.0);
        assert!((center[1] - expected_center[1]).abs() < 1.0);

        let round_trip = canvas
            .raster_to_logical_point(center)
            .expect("content center maps back");
        assert!((round_trip[0] - 640.0).abs() < 0.01);
        assert!((round_trip[1] - 360.0).abs() < 0.01);
    }
}

#[test]
fn bars_and_non_finite_points_are_rejected_at_both_edges() {
    for raster in [
        Extent2D::new(1920, 1200),
        Extent2D::new(1001, 777),
        Extent2D::new(720, 1280),
        Extent2D::new(1279, 719),
    ] {
        let canvas = Canvas2D::new(LOGICAL, raster).expect("positive raster extent");
        let viewport = canvas.viewport();
        assert!(canvas.raster_to_logical_point([f32::NAN, 0.0]).is_err());
        assert!(canvas
            .raster_to_logical_point([0.0, f32::INFINITY])
            .is_err());
        assert!(canvas
            .logical_to_raster_point([f32::NEG_INFINITY, 1.0])
            .is_err());

        if viewport.y > 0 {
            assert!(!canvas.contains_raster_point([0.0, viewport.y as f32 - 0.5]));
            assert!(canvas
                .raster_to_logical_point([0.0, viewport.y as f32 - 0.5])
                .is_err());
            assert!(!canvas.contains_raster_point([0.0, (viewport.y + viewport.height) as f32,]));
        }
        if viewport.x > 0 {
            assert!(!canvas.contains_raster_point([viewport.x as f32 - 0.5, 0.0]));
            assert!(canvas
                .raster_to_logical_point([viewport.x as f32 - 0.5, 0.0])
                .is_err());
            assert!(!canvas.contains_raster_point([(viewport.x + viewport.width) as f32, 0.0,]));
        }
    }
}

#[test]
fn minimum_viewport_and_rect_rounding_remain_positive() {
    for raster in [
        Extent2D::new(1, 1),
        Extent2D::new(1, 2),
        Extent2D::new(2, 1),
    ] {
        let canvas = Canvas2D::new(LOGICAL, raster).expect("one-pixel raster is valid");
        let viewport = canvas.viewport();
        assert!(viewport.width >= 1 && viewport.height >= 1);
        assert!(viewport.x + viewport.width <= raster.width);
        assert!(viewport.y + viewport.height <= raster.height);
        let mapped = canvas
            .logical_to_raster_rect(astra_media_core::RectI::new(0, 0, 1, 1))
            .expect("positive logical rect maps");
        assert!(mapped.width >= 1 && mapped.height >= 1);
    }
}
