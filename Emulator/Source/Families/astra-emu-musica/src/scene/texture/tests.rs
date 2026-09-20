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
