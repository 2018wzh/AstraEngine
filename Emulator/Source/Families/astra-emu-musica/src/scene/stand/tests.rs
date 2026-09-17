use super::*;

fn png(entries: &[(&str, &str, u8)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        for &(key, value, encoding) in entries {
            match encoding {
                0 => encoder.add_text_chunk(key.into(), value.into()),
                1 => encoder.add_ztxt_chunk(key.into(), value.into()),
                _ => encoder.add_itxt_chunk(key.into(), value.into()),
            }
            .unwrap();
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255; 8 * 8 * 4])
            .unwrap();
    }
    bytes
}

#[test]
fn native_offsets_support_png_text_encodings_without_using_top_as_screen_y() {
    let offsets = Offsets::read(&png(&[
        ("ol", "3", 0),
        ("ot", "25", 1),
        ("or", "5", 2),
        ("ob", "6", 0),
    ]))
    .unwrap();
    assert_eq!(
        offsets,
        Offsets {
            left: 3,
            top: 25,
            right: 5,
            bottom: 6
        }
    );
    let (source, destination) = offsets.placement(8, 8, 32, 16, 6).unwrap();
    assert_eq!(
        source,
        RectI {
            x: 0,
            y: 0,
            width: 8,
            height: 8
        }
    );
    assert_eq!(
        destination,
        RectI {
            x: 11,
            y: 24,
            width: 8,
            height: 8
        }
    );
    let (source, destination) = offsets.placement(8, 8, 32, 16, 9).unwrap();
    assert_eq!(source.height, 5);
    assert_eq!(
        destination,
        RectI {
            x: 11,
            y: 27,
            width: 8,
            height: 5
        }
    );
    assert_eq!(offsets.placement(8, 8, 32, 16, 2).unwrap().0.height, 8);
}

#[test]
fn malformed_duplicate_and_oversized_geometry_text_is_rejected() {
    for entries in [
        vec![("ol", "not-an-integer", 0)],
        vec![("ol", "1", 0), ("ol", "2", 1)],
        vec![("ob", "2147483648", 2)],
        vec![("ot", "123456789012345678901234567890123456789", 1)],
    ] {
        assert_eq!(
            Offsets::read(&png(&entries)).unwrap_err().code(),
            "ASTRA_EMU_MUSICA_PNG_METADATA"
        );
    }
    assert!(Offsets::read(b"not PNG").is_err());
    assert_eq!(
        Offsets::read(&png(&[("unrelated", "metadata", 0)])).unwrap(),
        Offsets::default()
    );
}

#[test]
fn native_crop_and_coordinate_overflow_fail_without_saturation() {
    let offsets = Offsets {
        bottom: 6,
        ..Offsets::default()
    };
    assert!(offsets.placement(8, 8, 32, 0, 14).is_err());
    assert!(offsets.placement(8, 8, 32, i32::MIN, 6).is_err());
    assert!(offsets.placement(8, 8, u32::MAX, 0, 6).is_err());
    assert!(Offsets {
        left: -20,
        ..offsets
    }
    .placement(8, 8, 32, 0, 6)
    .is_err());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_stand_gpu_uses_metadata_crop_and_restore_without_changing_cached_pixels() {
    use crate::{mount_musica, parse_sc, MusicaVm, ScOpcodeCatalog, MUSICA_PROFILE_FILE};
    use astra_core::Hash256;
    let root = tempfile::tempdir().unwrap();
    let script = b".stage * * 0 0 Stand.png 16,9\r\n.end\r\n";
    crate::test_fixture::game(root.path(), script);
    crate::test_fixture::stand(
        root.path(),
        &png(&[("ol", "3", 0), ("or", "5", 1), ("ob", "6", 2)]),
    );
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_PROFILE_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(script),
            parse_sc(script, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    vm.step(1).unwrap();
    let mut scene = Scene::new(archive.clone(), 32, 32, crate::ScriptEncoding::ShiftJis).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let pixel = |x: usize, y: usize| &scene.pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4];
    assert_eq!(pixel(11, 26), &[0, 0, 0, 255]);
    assert_eq!(pixel(10, 27), &[0, 0, 0, 255]);
    assert_eq!(pixel(11, 27), &[255, 255, 255, 255]);
    assert_eq!(pixel(18, 31), &[255, 255, 255, 255]);
    assert_eq!(pixel(19, 31), &[0, 0, 0, 255]);
    let cropped = scene.pixels.clone();
    let mut restored_vm = make_vm();
    restored_vm
        .restore_native_save(&vm.encode_native_save().unwrap(), 1)
        .unwrap();
    let mut restored = Scene::new(archive, 32, 32, crate::ScriptEncoding::ShiftJis).unwrap();
    restored.render(restored_vm.state(), None, None).unwrap();
    assert_eq!(restored.pixels, cropped);
    let mut state = vm.state().clone();
    state.stage.as_mut().unwrap().stands[0].resource_parameter = 6;
    scene.render(&state, None, None).unwrap();
    assert_eq!(
        &scene.pixels[(24 * 32 + 11) * 4..(24 * 32 + 12) * 4],
        &[255; 4]
    );
    let valid = scene.pixels.clone();
    state.stage.as_mut().unwrap().stands[0].resource_parameter = 14;
    assert_eq!(
        scene.render(&state, None, None).unwrap_err().code(),
        "ASTRA_EMU_MUSICA_STAND_GEOMETRY"
    );
    assert_eq!(scene.pixels, valid);
}
