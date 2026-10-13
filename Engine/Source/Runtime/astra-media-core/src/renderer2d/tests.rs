use super::validate_texture_metadata::sample_texture;
use super::*;

#[test]
fn aspect_fit_root_clip_uses_the_integer_viewport_edge() {
    let transform = Transform2D {
        m11: 1001.0 / 1280.0,
        m22: 1001.0 / 1280.0,
        tx: 0.0,
        ty: 107.0,
        ..Transform2D::IDENTITY
    };

    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 1001,
            height: 777,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "aspect-fit-root-clip".into(),
        })
        .unwrap();
    let frame = renderer
        .capture_frame(&[
            SceneCommand::clear([0, 0, 0, 255]),
            SceneCommand::PushClip {
                rect: RectI::new(0, 107, 1001, 563),
            },
            SceneCommand::PushTransform { transform },
            SceneCommand::rect("logical", 0, 0, 1280, 720, [255, 0, 0, 255]),
            SceneCommand::PopTransform,
            SceneCommand::PopClip,
        ])
        .unwrap();
    let pixel = |x: usize, y: usize| &frame.bytes[(y * 1001 + x) * 4..(y * 1001 + x) * 4 + 4];
    assert_eq!(pixel(0, 669), &[255, 0, 0, 255]);
    assert_eq!(pixel(0, 670), &[0, 0, 0, 255]);
}

#[test]
fn rotated_clip_keeps_conservative_bounds_rounding() {
    let transform = Transform2D {
        m11: 0.0,
        m12: 1.0,
        m21: -1.0,
        m22: 0.0,
        tx: 10.25,
        ty: 20.25,
    };
    let clip = transformed_bounds(transform, RectI::new(0, 0, 4, 2)).unwrap();
    assert_eq!(clip, RectI::new(8, 20, 3, 5));
}

#[test]
fn fractional_non_root_clip_keeps_its_conservative_far_edge() {
    let transform = Transform2D {
        m11: 1.0,
        m22: 1.0,
        tx: 0.25,
        ty: 0.25,
        ..Transform2D::IDENTITY
    };
    let clip = transformed_bounds(transform, RectI::new(578, 656, 240, 48)).unwrap();
    assert_eq!(clip, RectI::new(578, 656, 241, 49));

    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 4,
            height: 4,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "fractional-nested-clip".into(),
        })
        .unwrap();
    let frame = renderer
        .capture_frame(&[
            SceneCommand::clear([0, 0, 0, 255]),
            SceneCommand::PushClip {
                rect: RectI::new(0, 0, 4, 4),
            },
            SceneCommand::PushTransform { transform },
            SceneCommand::PushClip {
                rect: RectI::new(0, 0, 2, 2),
            },
            SceneCommand::rect("fractional.clip", 0, 0, 4, 4, [0, 255, 0, 255]),
            SceneCommand::PopClip,
            SceneCommand::PopTransform,
            SceneCommand::PopClip,
        ])
        .unwrap();
    for y in 0..4 {
        for x in 0..4 {
            let offset = (y * 4 + x) * 4;
            let expected = if x < 3 && y < 3 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(&frame.bytes[offset..offset + 4], expected);
        }
    }
}

#[test]
fn one_pixel_root_viewport_remains_drawable() {
    let transform = Transform2D {
        m11: 1.0 / 1280.0,
        m22: 1.0 / 1280.0,
        ..Transform2D::IDENTITY
    };
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 1,
            height: 1,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "tiny-root-clip".into(),
        })
        .unwrap();
    let frame = renderer
        .capture_frame(&[
            SceneCommand::clear([0, 0, 0, 255]),
            SceneCommand::PushClip {
                rect: RectI::new(0, 0, 1, 1),
            },
            SceneCommand::PushTransform { transform },
            SceneCommand::rect("logical", 0, 0, 1280, 720, [255, 0, 0, 255]),
            SceneCommand::PopTransform,
            SceneCommand::PopClip,
        ])
        .unwrap();
    assert_eq!(frame.bytes.as_slice(), &[255, 0, 0, 255]);
}

#[test]
fn owned_pixel_buffer_moves_the_pixel_allocation_without_copying() {
    let pixels = vec![0x11; 64 * 64 * 4];
    let pointer = pixels.as_ptr();
    let buffer = OwnedPixelBuffer::from_vec(pixels);

    assert_eq!(buffer.as_slice().as_ptr(), pointer);
    assert_eq!(buffer.len(), 64 * 64 * 4);
    assert!(!buffer.is_empty());
}

#[test]
fn texture_frame_preserves_the_buffer_allocation() {
    let pixels = vec![0x22; 8 * 4 * 4];
    let pointer = pixels.as_ptr();
    let frame = TextureFrame::from_vec(8, 4, pixels).unwrap();

    assert_eq!(frame.rgba8.as_slice().as_ptr(), pointer);
}

#[test]
fn mesh_texture_sampling_preserves_channels_and_filter_semantics() {
    let frame = TextureFrame::from_vec(2, 1, vec![100, 50, 25, 128, 200, 100, 50, 255]).unwrap();

    assert_eq!(
        sample_texture(&frame, [0.49, 0.0], TextureFilter2D::Nearest),
        [100, 50, 25, 128]
    );
    assert_eq!(
        sample_texture(&frame, [0.5, 0.0], TextureFilter2D::Linear),
        [150, 75, 38, 192]
    );
}

#[test]
fn retained_validation_tracks_owned_texture_without_pixel_mirror() {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 64,
            height: 64,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "live-metadata-test".into(),
        })
        .unwrap();
    let pixels = vec![0x33; 16 * 8 * 4];
    let pointer = pixels.as_ptr();
    let frame = TextureFrame::from_vec(16, 8, pixels).unwrap();
    assert_eq!(frame.rgba8.as_slice().as_ptr(), pointer);

    renderer
        .submit_frame(&[
            SceneCommand::UploadTexture {
                resource_id: "live.main".into(),
                frame,
            },
            SceneCommand::Sprite {
                id: "draw.main".into(),
                texture_id: "live.main".into(),
                source: None,
                destination: RectI::new(0, 0, 16, 8),
                opacity: 1.0,
                blend: BlendMode::Alpha,
            },
        ])
        .unwrap();

    let update = vec![0x44; 4 * 2 * 4];
    let update_pointer = update.as_ptr();
    let update = OwnedPixelBuffer::from_vec(update);
    renderer
        .submit_frame(&[SceneCommand::UpdateTextureRegion {
            resource_id: "live.main".into(),
            x: 2,
            y: 3,
            width: 4,
            height: 2,
            rgba8: update.clone(),
        }])
        .unwrap();
    assert_eq!(update.as_slice().as_ptr(), update_pointer);
}
