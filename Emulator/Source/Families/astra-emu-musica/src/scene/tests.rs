use super::*;
use crate::{mount_musica, parse_sc, MusicaVm, ScOpcodeCatalog, MUSICA_PROFILE_FILE};
use astra_core::Hash256;

#[test]
#[ignore = "requires a hardware GPU"]
fn native_stage_gpu_restore_preserves_position_and_rejects_invalid_sequences() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.png -4 2 * 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    vm.step(1).unwrap();
    let mut scene = Scene::new(archive.clone(), 32, 32, crate::ScriptEncoding::ShiftJis).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let pixel = |x: usize, y: usize| &scene.pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4];
    assert_eq!(pixel(0, 0), &[0, 0, 0, 255]);
    assert_ne!(pixel(0, 2), &[0, 0, 0, 255]);
    assert_eq!(pixel(12, 2), &[0, 0, 0, 255]);
    let expected = scene.pixels.clone();
    let mut restored_vm = make_vm();
    restored_vm
        .restore_native_save(&vm.encode_native_save().unwrap(), 1)
        .unwrap();
    let mut restored_scene = Scene::new(archive, 32, 32, crate::ScriptEncoding::ShiftJis).unwrap();
    restored_scene
        .render(restored_vm.state(), None, None)
        .unwrap();
    assert_eq!(restored_scene.pixels, expected);
    let mut unsupported = restored_vm.state().clone();
    unsupported
        .stage
        .as_mut()
        .unwrap()
        .resource_sequence
        .extend([None, None]);
    assert_eq!(
        restored_scene
            .render(&unsupported, None, None)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_STAGE_SEQUENCE"
    );
    assert_eq!(restored_scene.pixels, expected);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn dual_stage_screen_prefix_orders_layers_and_restores_without_partial_frames() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage Light_SC.PNG:BG.png 4 2 BG.png 0 0 Stand.png 8,0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let png = |width, height, color| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    crate::test_fixture::stand(root.path(), &png(8, 8, [64, 128, 192, 255]));
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let make_scene = || {
        let mut scene =
            Scene::new(archive.clone(), 16, 16, crate::ScriptEncoding::ShiftJis).unwrap();
        // Seed the shared decoded cache; the archive-backed background and stand
        // still exercise native resource loading and geometry.
        scene
            .textures
            .decode(
                "musica:/bg/Light_SC.PNG".into(),
                &png(16, 16, [128, 0, 0, 128]),
            )
            .unwrap();
        scene
    };
    let mut vm = make_vm();
    vm.step(1).unwrap();
    let mut scene = make_scene();
    scene.render(vm.state(), None, None).unwrap();
    let pixel = |scene: &Scene, x: usize, y: usize| {
        <[u8; 4]>::try_from(&scene.pixels[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]).unwrap()
    };
    assert_eq!(pixel(&scene, 0, 0), [83, 100, 220, 255]);
    assert_eq!(pixel(&scene, 5, 10), [83, 100, 220, 255]);
    let mut restored_vm = make_vm();
    restored_vm
        .restore_native_save(&vm.encode_native_save().unwrap(), 1)
        .unwrap();
    let mut restored = make_scene();
    restored.render(restored_vm.state(), None, None).unwrap();
    assert_eq!(restored.pixels, scene.pixels);
    let mut state = vm.state().clone();
    state.stage.as_mut().unwrap().resource_sequence[1] = None;
    scene.render(&state, None, None).unwrap();
    assert_eq!(pixel(&scene, 5, 10), [112, 128, 192, 255]);
    let valid = scene.pixels.clone();
    state.stage.as_mut().unwrap().resource_sequence[0] = Some("musica:/bg/Light_ov.png".into());
    assert_eq!(
        scene.render(&state, None, None).unwrap_err().code(),
        "ASTRA_EMU_MUSICA_STAGE_BLEND"
    );
    assert_eq!(scene.pixels, valid);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn panel_modes_draw_at_native_positions_and_clear_after_restore() {
    let root = tempfile::tempdir().unwrap();
    let source = b".panel 1 * custom.png\r\n.panel 3\r\n.panel 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut scene = Scene::new(archive, 16, 80, crate::ScriptEncoding::ShiftJis).unwrap();
    for (uri, color) in [
        ("musica:/sys/custom.png", [255, 0, 0, 255]),
        ("musica:/sys/fullPanel.png", [0, 255, 0, 255]),
    ] {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 80, image::Rgba(color))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        scene
            .textures
            .decode(uri.into(), &png.into_inner())
            .unwrap();
    }
    let mut vm = make_vm();
    for tick in 1..=3 {
        vm.step(tick).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        let expected = scene.pixels.clone();
        let top = &expected[..4];
        let bottom = &expected[79 * 16 * 4..79 * 16 * 4 + 4];
        match tick {
            1 => {
                assert_eq!(top, [0, 0, 0, 255]);
                assert_eq!(bottom, [255, 0, 0, 255]);
            }
            2 => {
                assert_eq!(top, [0, 255, 0, 255]);
                assert_eq!(bottom, top);
            }
            3 => {
                assert_eq!(top, [0, 0, 0, 255]);
                assert_eq!(bottom, top);
            }
            _ => unreachable!(),
        }
        let mut restored = make_vm();
        restored
            .restore_native_save(&vm.encode_native_save().unwrap(), 1)
            .unwrap();
        scene.render(restored.state(), None, None).unwrap();
        assert_eq!(scene.pixels, expected);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn screen_shake_gpu_preserves_composed_clipping_and_restores_mid_interval() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.png 0 0 * 0 0\r\n.shakescreen V 4 30\r\n.transition 0 * 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    let mut scene = Scene::new(archive, 16, 16, crate::ScriptEncoding::ShiftJis).unwrap();
    vm.step(1).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let original = scene.pixels.clone();
    vm.step(2).unwrap();
    vm.advance_screen_shake_clock(30_000_000).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let shifted = scene.pixels.clone();
    for y in 0..16 {
        for x in 0..16 {
            let index = (y * 16 + x) * 4;
            let expected = if y < 12 {
                &original[((y + 4) * 16 + x) * 4..((y + 4) * 16 + x) * 4 + 4]
            } else {
                &[0, 0, 0, 255]
            };
            assert_eq!(&shifted[index..index + 4], expected);
        }
    }
    vm.advance_screen_shake_clock(29_000_000).unwrap();
    let mut restored = make_vm();
    restored
        .restore_native_save(&vm.encode_native_save().unwrap(), 3)
        .unwrap();
    vm.advance_screen_shake_clock(1_000_000).unwrap();
    restored.advance_screen_shake_clock(1_000_000).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let expected = scene.pixels.clone();
    scene.render(restored.state(), None, None).unwrap();
    assert_eq!(scene.pixels, expected);
    vm.step(3).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    assert_eq!(scene.pixels, original);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn scrollxf_gpu_crops_translates_and_accepts_empty_extent() {
    let root = tempfile::tempdir().unwrap();
    let source=b".stage * BG.png 0 0\r\n.scrollxf 16 16 8 8 0 0 8 8 100 0\r\n.scrollxf 0 0 0 0 0 0 0 0 100 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    let mut scene = Scene::new(archive, 16, 16, crate::ScriptEncoding::ShiftJis).unwrap();
    vm.step(1).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let original = scene.pixels.clone();
    vm.step(2).unwrap();
    vm.advance_scroll_xf_clock(50_000_000).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let cropped = scene.pixels.clone();
    for y in 0..16 {
        for x in 0..16 {
            let index = (y * 16 + x) * 4;
            let expected = if x < 12 && y < 12 {
                &original[((y + 4) * 16 + x + 4) * 4..((y + 4) * 16 + x + 4) * 4 + 4]
            } else {
                &[0, 0, 0, 255]
            };
            assert_eq!(&cropped[index..index + 4], expected);
        }
    }
    let mut restored = make_vm();
    restored
        .restore_native_save(&vm.encode_native_save().unwrap(), 3)
        .unwrap();
    scene.render(restored.state(), None, None).unwrap();
    assert_eq!(scene.pixels, cropped);
    vm.step(3).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    assert!(scene
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel == &[0, 0, 0, 255]));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_gpu_scaled_ani_offset_scroll_clip_and_shake_preserve_logical_geometry() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.ani 0 0 * 0 0\r\n.scrollxf 16 16 12 12 0 0 2 1 100 0\r\n.shakescreen V 2 30\r\n.transition 0 * 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);

    // The test-only ANI contains four opaque quadrants and a non-zero native
    // origin. It is deliberately small and synthetic; the Scene loader and
    // archive path remain the same as a native family session.
    let mut ani = vec![0x00, 0x01, 0x01, 0x00, 0, 0, 0, 0];
    ani.extend_from_slice(b"scaled\0");
    ani.extend_from_slice(&16u16.to_le_bytes());
    ani.extend_from_slice(&16u16.to_le_bytes());
    ani.extend_from_slice(&32u16.to_le_bytes());
    ani.extend_from_slice(&2i16.to_le_bytes());
    ani.extend_from_slice(&1i16.to_le_bytes());
    for y in 0..16u16 {
        for x in 0..16u16 {
            let rgba = match (x < 8, y < 8) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255, 255, 0, 255],
            };
            // ANI stores BGRA while the Scene renderer consumes RGBA.
            ani.extend_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
        }
    }
    crate::test_fixture::asset(root.path(), "bg", "BG.ani", &ani);

    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let scales = [(16u32, 1.0f32), (24, 1.5), (32, 2.0), (48, 3.0)];
    let mut baseline_by_scale = Vec::new();
    let mut transformed_by_scale = Vec::new();

    for (raster_size, scale) in scales {
        let mut vm = make_vm();
        vm.step(1).unwrap();
        let mut scene = Scene::new_scaled(
            archive.clone(),
            16,
            16,
            raster_size,
            raster_size,
            std::collections::BTreeMap::new(),
            crate::ScriptEncoding::ShiftJis,
        )
        .unwrap();
        assert_ne!(scene.renderer.identity().device_type, "cpu");
        scene.render(vm.state(), None, None).unwrap();
        let baseline = scene.pixels.clone();

        // The ANI origin is [2, 1], so the first red quadrant begins at that
        // logical point while the preceding logical border remains clear.
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 2, 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 1, 1),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 12, 1),
            [0, 255, 0, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 2, 9),
            [0, 0, 255, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 12, 9),
            [255, 255, 0, 255]
        );

        vm.step(2).unwrap();
        vm.advance_scroll_xf_clock(50_000_000).unwrap();
        let mut cropped_state = vm.state().clone();
        cropped_state.screen_shake = None;
        scene.render(&cropped_state, None, None).unwrap();
        let cropped = scene.pixels.clone();
        // The half-way scroll moves the source by one logical pixel and clips
        // to a 14x14 visible extent. This checks source crop and clip through
        // the actual Musica Scene::render path.
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 2, 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 13, 1),
            [0, 255, 0, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 14, 1),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 2, 13),
            [0, 0, 255, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 2, 14),
            [0, 0, 0, 255]
        );

        vm.step(3).unwrap();
        vm.advance_screen_shake_clock(30_000_000).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        let transformed = scene.pixels.clone();
        // Vertical shake is -2 logical pixels on its first update. The
        // compositor's outer clip must keep the shifted frame inside the
        // logical viewport while preserving the inner scroll clip.
        assert_eq!(
            sample_logical(&transformed, raster_size, scale, 2, 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&transformed, raster_size, scale, 2, 13),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&transformed, raster_size, scale, 14, 1),
            [0, 0, 0, 255]
        );

        baseline_by_scale.push((raster_size, baseline));
        transformed_by_scale.push((raster_size, transformed));
    }

    // The same logical scene must agree at every supported output density;
    // compare representative interior logical pixels rather than raster
    // bytes so the assertion covers the coordinate contract.
    let baseline_reference = &baseline_by_scale[0].1;
    let transformed_reference = &transformed_by_scale[0].1;
    for ((raster_size, baseline), (_, transformed)) in
        baseline_by_scale.iter().zip(&transformed_by_scale)
    {
        let scale = *raster_size as f32 / 16.0;
        for (x, y) in [(2, 1), (12, 1), (2, 9), (12, 9), (2, 13)] {
            assert_eq!(
                sample_logical(baseline, *raster_size, scale, x, y),
                sample_logical(baseline_reference, 16, 1.0, x, y)
            );
            assert_eq!(
                sample_logical(transformed, *raster_size, scale, x, y),
                sample_logical(transformed_reference, 16, 1.0, x, y)
            );
        }
    }
}

fn sample_logical(pixels: &[u8], raster_size: u32, scale: f32, x: u32, y: u32) -> [u8; 4] {
    let raster_x = (((x as f32) + 0.5) * scale).floor() as u32;
    let raster_y = (((y as f32) + 0.5) * scale).floor() as u32;
    let offset = ((raster_y * raster_size + raster_x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}
