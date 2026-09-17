use super::*;
use crate::{mount_minori, parse_sc, MinoriVm, ScOpcodeCatalog, MINORI_PROFILE_FILE};
use astra_core::Hash256;

#[test]
#[ignore = "requires a hardware GPU"]
fn native_stage_gpu_restore_preserves_position_and_rejects_invalid_sequences() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.png -4 2 * 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_minori(root.path(), std::path::Path::new(MINORI_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    vm.step(1).unwrap();
    let mut scene = Scene::new(archive.clone(), 32, 32).unwrap();
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
    let mut restored_scene = Scene::new(archive, 32, 32).unwrap();
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
        "ASTRA_EMU_MINORI_STAGE_SEQUENCE"
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
        Arc::new(mount_minori(root.path(), std::path::Path::new(MINORI_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap(),
            1,
        )
        .unwrap()
    };
    let make_scene = || {
        let mut scene = Scene::new(archive.clone(), 16, 16).unwrap();
        // Seed the shared decoded cache; the archive-backed background and stand
        // still exercise native resource loading and geometry.
        scene
            .textures
            .decode(
                "minori:/bg/Light_SC.PNG".into(),
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
    state.stage.as_mut().unwrap().resource_sequence[0] = Some("minori:/bg/Light_ov.png".into());
    assert_eq!(
        scene.render(&state, None, None).unwrap_err().code(),
        "ASTRA_EMU_MINORI_STAGE_BLEND"
    );
    assert_eq!(scene.pixels, valid);
}
