use super::*;
use crate::{mount_minori, parse_sc, MinoriVm, ScOpcodeCatalog, MINORI_PROFILE_FILE};
use astra_core::Hash256;

#[test]
#[ignore = "requires a hardware GPU"]
fn native_stage_gpu_restore_preserves_position_and_rejects_unimplemented_sequences() {
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
        .push(None);
    assert_eq!(
        restored_scene
            .render(&unsupported, None, None)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MINORI_STAGE_SEQUENCE"
    );
    assert_eq!(restored_scene.pixels, expected);
}
