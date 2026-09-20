use super::*;
use crate::{
    mount_musica, parse_sc, profile::mount_musica_with_texture_overrides, MusicaVm,
    ScOpcodeCatalog, MUSICA_PROFILE_FILE,
};
use astra_core::Hash256;
use std::io::{Cursor, Write};

fn ani() -> Vec<u8> {
    let mut bytes = vec![0, 1, 1, 0, 0, 0, 0, 0, b'f', 0];
    for value in [1u16, 1, 32, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([32, 96, 224, 255]);
    bytes
}
fn sqz() -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[32, 96, 224, 255]).unwrap();
    let frame = encoder.finish().unwrap();
    let mut bytes = b"SQZ1".to_vec();
    for value in [
        32u32,
        1,
        1,
        1,
        36,
        frame.len() as u32,
        36,
        frame.len() as u32,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(frame);
    bytes
}

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba(color))
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn quadrant_png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        let color = match (x * 2 < width, y * 2 < height) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [0, 0, 255, 255],
            (false, false) => [255, 255, 0, 255],
        };
        image::Rgba(color)
    });
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn multi_frame_ani() -> Vec<u8> {
    let mut bytes = vec![0, 1, 2, 0, 0, 0, 0, 0];
    for (name, color) in [
        (b"a\0".as_slice(), [32, 96, 224, 255]),
        (b"b\0", [64, 128, 240, 255]),
    ] {
        bytes.extend_from_slice(name);
        for value in [1u16, 1, 32, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&color);
    }
    bytes
}

fn sample_logical(pixels: &[u8], raster: u32, logical: u32, x: u32, y: u32) -> [u8; 4] {
    let raster_x = usize::try_from(u64::from(x) * u64::from(raster) / u64::from(logical)).unwrap();
    let raster_y = usize::try_from(u64::from(y) * u64::from(raster) / u64::from(logical)).unwrap();
    let width = usize::try_from(raster).unwrap();
    pixels[(raster_y * width + raster_x) * 4..(raster_y * width + raster_x + 1) * 4]
        .try_into()
        .unwrap()
}

fn ani_with_origin(offset_x: u16, offset_y: u16) -> Vec<u8> {
    let mut bytes = vec![0, 1, 1, 0, 0, 0, 0, 0, b'f', 0];
    for value in [1u16, 1, 32, offset_x, offset_y] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([32, 96, 224, 255]);
    bytes
}

#[test]
#[ignore = "requires a hardware GPU"]
fn profile_texture_overrides_preserve_native_geometry_and_ani_origin() {
    let root = tempfile::tempdir().unwrap();
    crate::test_fixture::game(root.path(), b".stage * BG.png 2 3\r\n.end\r\n");
    let native_png = png(16, 16, [25, 100, 220, 255]);
    let native_ani = ani_with_origin(3, 4);
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[("BG.png", &native_png), ("BG.ANI", &native_ani)],
    );
    std::fs::create_dir_all(root.path().join("hd")).unwrap();
    let replacement_png = png(32, 32, [220, 40, 80, 255]);
    let replacement_ani = png(24, 20, [80, 220, 100, 255]);
    std::fs::write(root.path().join("hd/static.png"), replacement_png).unwrap();
    std::fs::write(root.path().join("hd/ani.png"), replacement_ani).unwrap();
    let profile_path = root.path().join(MUSICA_PROFILE_FILE);
    let mut profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profile_path).unwrap()).unwrap();
    profile["texture_overrides"] = serde_json::json!({
        "musica:/bg/BG.png": "hd/static.png",
        "musica:/bg/BG.ANI": "hd/ani.png",
    });
    std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();

    let (archive, overrides) =
        mount_musica_with_texture_overrides(root.path(), &profile_path).unwrap();
    let mut scene = Scene::new_scaled(
        Arc::new(archive),
        16,
        16,
        32,
        32,
        overrides,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    let static_asset = scene.texture_asset("musica:/bg/BG.png").unwrap();
    assert_eq!(static_asset.frame.width, 32);
    assert_eq!(static_asset.frame.height, 32);
    assert_eq!(static_asset.logical_extent, Extent2D::new(16, 16));
    assert_eq!(static_asset.logical_origin, [0, 0]);
    let ani_asset = scene.texture_asset("musica:/bg/BG.ANI").unwrap();
    assert_eq!(ani_asset.frame.width, 24);
    assert_eq!(ani_asset.frame.height, 20);
    assert_eq!(ani_asset.logical_extent, Extent2D::new(1, 1));
    assert_eq!(ani_asset.logical_origin, [3, 4]);
}

#[test]
fn texture_override_profile_rejects_unsupported_archive_sources() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage * BG.png 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let native_png = png(16, 16, [25, 100, 220, 255]);
    let sqz = sqz();
    let multi_ani = multi_frame_ani();
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[
            ("BG.png", &native_png),
            ("BG.SQZ", &sqz),
            ("BG.ANI", &multi_ani),
        ],
    );
    std::fs::create_dir_all(root.path().join("hd")).unwrap();
    std::fs::write(
        root.path().join("hd/replacement.png"),
        png(16, 16, [1, 2, 3, 255]),
    )
    .unwrap();
    let profile_path = root.path().join(MUSICA_PROFILE_FILE);
    let mut profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profile_path).unwrap()).unwrap();
    let set_override = |profile: &mut serde_json::Value, source: &str| {
        profile["texture_overrides"] = serde_json::json!({
            source: "hd/replacement.png",
        });
        std::fs::write(&profile_path, serde_json::to_vec(profile).unwrap()).unwrap();
    };
    let validate_profile = || {
        let (archive, overrides) =
            match mount_musica_with_texture_overrides(root.path(), &profile_path) {
                Ok(value) => value,
                Err(error) => panic!("profile mount failed: {}", error.code()),
            };
        match validate_texture_overrides(&archive, &overrides) {
            Ok(()) => panic!("unsupported texture override was accepted"),
            Err(error) => error,
        }
    };

    set_override(&mut profile, "musica:/bg/missing.png");
    let missing_source = validate_profile();
    assert_eq!(
        missing_source.code(),
        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE"
    );
    set_override(&mut profile, "musica:/bg/BG.SQZ");
    let sqz_source = validate_profile();
    assert_eq!(
        sqz_source.code(),
        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT"
    );
    set_override(&mut profile, "musica:/bg/BG.ANI");
    let multi_frame_source = validate_profile();
    assert_eq!(
        multi_frame_source.code(),
        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_ANI_FRAMES"
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn profile_texture_overrides_render_gpu_at_all_scales_with_crop_and_offset() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.png 0 0 * 0 0\r\n.scrollxf 16 16 12 12 0 0 2 1 100 0\r\n.shakescreen V 2 30\r\n.transition 0 * 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let native_png = quadrant_png(16, 16);
    let native_ani = ani_with_origin(3, 4);
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[("BG.png", &native_png), ("BG.ANI", &native_ani)],
    );
    std::fs::create_dir_all(root.path().join("hd")).unwrap();
    std::fs::write(root.path().join("hd/static.png"), quadrant_png(32, 32)).unwrap();
    std::fs::write(
        root.path().join("hd/ani.png"),
        png(24, 20, [80, 220, 100, 255]),
    )
    .unwrap();
    let profile_path = root.path().join(MUSICA_PROFILE_FILE);
    let mut profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profile_path).unwrap()).unwrap();
    profile["texture_overrides"] = serde_json::json!({
        "musica:/bg/BG.png": "hd/static.png",
        "musica:/bg/BG.ANI": "hd/ani.png",
    });
    std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();

    let (archive, overrides) =
        mount_musica_with_texture_overrides(root.path(), &profile_path).unwrap();
    let archive = Arc::new(archive);
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };

    for raster in [16u32, 24, 32, 48] {
        let mut vm = make_vm();
        vm.step(1).unwrap();
        let mut scene = Scene::new_scaled(
            archive.clone(),
            16,
            16,
            raster,
            raster,
            overrides.clone(),
            crate::ScriptEncoding::ShiftJis,
        )
        .unwrap();
        assert_ne!(scene.renderer.identity().device_type, "cpu");

        let static_asset = scene.texture_asset("musica:/bg/BG.png").unwrap();
        assert_eq!(static_asset.frame.width, 32);
        assert_eq!(static_asset.frame.height, 32);
        assert_eq!(static_asset.logical_extent, Extent2D::new(16, 16));
        assert_eq!(static_asset.logical_origin, [0, 0]);
        let first_resident = scene.textures.resident_bytes();
        assert_eq!(first_resident, 16 * 16 * 4 + 32 * 32 * 4);
        assert_eq!(
            scene.texture_asset("musica:/bg/BG.png").unwrap(),
            static_asset
        );
        assert_eq!(scene.textures.resident_bytes(), first_resident);

        scene.render(vm.state(), None, None).unwrap();
        for (x, y, color) in [
            (3, 3, [255, 0, 0, 255]),
            (12, 3, [0, 255, 0, 255]),
            (3, 12, [0, 0, 255, 255]),
            (12, 12, [255, 255, 0, 255]),
        ] {
            assert_eq!(sample_logical(&scene.pixels, raster, 16, x, y), color);
        }

        vm.step(2).unwrap();
        vm.advance_scroll_xf_clock(50_000_000).unwrap();
        let mut cropped_state = vm.state().clone();
        cropped_state.screen_shake = None;
        scene.render(&cropped_state, None, None).unwrap();
        assert_ne!(
            sample_logical(&scene.pixels, raster, 16, 1, 1),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&scene.pixels, raster, 16, 14, 1),
            [0, 0, 0, 255]
        );

        vm.step(3).unwrap();
        vm.advance_screen_shake_clock(30_000_000).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        assert_eq!(
            sample_logical(&scene.pixels, raster, 16, 14, 1),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&scene.pixels, raster, 16, 1, 15),
            [0, 0, 0, 255]
        );

        let mut ani_vm = make_vm();
        ani_vm.step(1).unwrap();
        let mut ani_state = ani_vm.state().clone();
        ani_state.stage.as_mut().unwrap().resource_sequence[0] = Some("musica:/bg/BG.ANI".into());
        ani_state.scroll_xf = None;
        ani_state.screen_shake = None;
        let ani_asset = scene.texture_asset("musica:/bg/BG.ANI").unwrap();
        assert_eq!(ani_asset.frame.width, 24);
        assert_eq!(ani_asset.frame.height, 20);
        assert_eq!(ani_asset.logical_extent, Extent2D::new(1, 1));
        assert_eq!(ani_asset.logical_origin, [3, 4]);
        scene.render(&ani_state, None, None).unwrap();
        assert_eq!(
            sample_logical(&scene.pixels, raster, 16, 3, 4),
            [80, 220, 100, 255]
        );
        assert_eq!(
            sample_logical(&scene.pixels, raster, 16, 2, 4),
            [0, 0, 0, 255]
        );
        assert_eq!(
            scene.textures.resident_bytes(),
            16 * 16 * 4 + 32 * 32 * 4 + 4 + 24 * 20 * 4
        );

        drop(scene);
        let mut reopened = Scene::new_scaled(
            archive.clone(),
            16,
            16,
            raster,
            raster,
            overrides.clone(),
            crate::ScriptEncoding::ShiftJis,
        )
        .unwrap();
        assert_ne!(reopened.renderer.identity().device_type, "cpu");
        reopened.render(vm.state(), None, None).unwrap();
        assert_eq!(
            sample_logical(&reopened.pixels, raster, 16, 3, 3),
            [255, 0, 0, 255]
        );
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_container_images_use_shared_gpu_cache_and_restore() {
    for (name, payload) in [("BG.ANI", ani()), ("BG.SQZ", sqz())] {
        let root = tempfile::tempdir().unwrap();
        let source = format!(".stage * {name} 2 3\r\n.end\r\n");
        crate::test_fixture::game(root.path(), source.as_bytes());
        crate::test_fixture::assets(
            root.path(),
            "bg",
            &[(name, &payload), ("bad.ani", b"invalid")],
        );
        let archive =
            Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source.as_bytes()),
            parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap();
        vm.step(1).unwrap();
        let saved = vm.encode_native_save().unwrap();
        let mut scene =
            Scene::new(archive.clone(), 16, 16, crate::ScriptEncoding::ShiftJis).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        assert_eq!(
            &scene.pixels[(3 * 16 + 2) * 4..(3 * 16 + 3) * 4],
            &[224, 96, 32, 255]
        );
        assert_eq!(scene.textures.resident_bytes(), 4);
        let expected = scene.pixels.clone();
        scene.render(vm.state(), None, None).unwrap();
        assert_eq!(scene.textures.resident_bytes(), 4);
        assert_eq!(scene.pixels, expected);
        let mut invalid = vm.state().clone();
        invalid
            .stage
            .as_mut()
            .unwrap()
            .background
            .as_mut()
            .unwrap()
            .resource_uri = "musica:/bg/bad.ani".into();
        assert_eq!(
            scene.render(&invalid, None, None).unwrap_err().code(),
            "ASTRA_EMU_MUSICA_ANI_HEADER"
        );
        assert_eq!(scene.pixels, expected);
        assert_eq!(scene.textures.resident_bytes(), 4);
        vm.restore_native_save(&saved, 1).unwrap();
        let mut restored = Scene::new(archive, 16, 16, crate::ScriptEncoding::ShiftJis).unwrap();
        restored.render(vm.state(), None, None).unwrap();
        assert_eq!(restored.pixels, expected);
    }
}

#[test]
fn native_container_allocation_respects_scene_budget() {
    for dimensions in [(0, 1), (8193, 1), (8192, 8192)] {
        assert_eq!(
            validate_dimensions(dimensions.0, dimensions.1)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MUSICA_IMAGE_BOUND"
        );
    }
    validate_dimensions(8192, 4096).unwrap();
}
