use super::*;
use crate::{mount_musica, parse_sc, MusicaVm, ScOpcodeCatalog, MUSICA_PROFILE_FILE};
use astra_core::Hash256;
use std::io::Write;

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
        let mut scene = Scene::new(archive.clone(), 16, 16).unwrap();
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
        let mut restored = Scene::new(archive, 16, 16).unwrap();
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
