use super::*;

fn setup() -> (tempfile::TempDir, CmvsArchive, CmvsPs2aVmState) {
    let (root, mut archive) = super::super::cache_tests::fixture();
    archive.bind_game_root(root.path().to_path_buf()).unwrap();
    std::fs::create_dir(root.path().join("save")).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    vm.interpreter_string_buffers.insert(
        5404,
        vec![
            CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
            CmvsStringSegment::OwnedString("save/".into()),
        ],
    );
    (root, archive, vm)
}

fn header(title: &str) -> Vec<u8> {
    let mut bytes = vec![0; 512];
    bytes[..4].copy_from_slice(b"CSV9");
    bytes[16..16 + title.len()].copy_from_slice(title.as_bytes());
    bytes
}

#[test]
fn catalog_preserves_files_and_native_menu_selection_with_ties() {
    let (root, archive, mut vm) = setup();
    let old = header("2026/09/20 01:02:02");
    let new = header("2026/09/21 01:02:02");
    for (slot, bytes) in [(0, &old), (2, &new), (3, &new)] {
        std::fs::write(root.path().join(format!("save/save{slot:03}.dat")), bytes).unwrap();
    }
    std::fs::write(root.path().join("save/save004.dat"), b"CSV1unknown").unwrap();
    archive.refresh_native_save_catalog(&mut vm, 5).unwrap();
    let catalog = vm.native_save_catalog.as_ref().unwrap();
    assert_eq!(catalog.latest_slot, Some(2));
    assert!(catalog.load_enabled(0) && catalog.load_enabled(2));
    assert!(!catalog.load_enabled(1) && !catalog.load_enabled(4));
    assert!(catalog.save_enabled(1) && !catalog.save_enabled(5));
    assert!(catalog.selected(2) && !catalog.selected(3));
    assert_eq!(
        std::fs::read(root.path().join("save/save000.dat")).unwrap(),
        old
    );
    crate::validate_cmvs390_vm_state(&vm).unwrap();
    let encoded = postcard::to_allocvec(&vm).unwrap();
    assert_eq!(
        postcard::from_bytes::<CmvsPs2aVmState>(&encoded).unwrap(),
        vm
    );
}

#[test]
fn truncated_valid_header_fails_without_replacing_existing_catalog() {
    let (root, archive, mut vm) = setup();
    archive.refresh_native_save_catalog(&mut vm, 3).unwrap();
    let before = vm.clone();
    std::fs::write(root.path().join("save/save001.dat"), b"CSV2short").unwrap();
    assert!(archive.refresh_native_save_catalog(&mut vm, 4).is_err());
    assert_eq!(vm, before);
    let mut unterminated = vec![b'a'; 512];
    unterminated[..4].copy_from_slice(b"CSV2");
    assert!(parse_header(&unterminated).is_err());
    assert!(parse_header(&header("short")).is_err());
}

#[test]
fn bounded_scan_and_empty_catalog_do_not_require_or_create_saves() {
    let (root, archive, mut vm) = setup();
    archive.refresh_native_save_catalog(&mut vm, 256).unwrap();
    let catalog = vm.native_save_catalog.as_ref().unwrap();
    assert_eq!(catalog.slots.len(), 256);
    assert_eq!(catalog.latest_slot, None);
    assert_eq!(
        std::fs::read_dir(root.path().join("save")).unwrap().count(),
        0
    );
    let previous = vm.clone();
    assert!(archive.refresh_native_save_catalog(&mut vm, 257).is_err());
    assert_eq!(vm, previous);
    vm.interpreter_string_buffers.clear();
    archive.refresh_native_save_catalog(&mut vm, 0).unwrap();
    assert!(vm.native_save_catalog.unwrap().slots.is_empty());
}

#[test]
fn native_date_ranking_uses_title_and_two_second_precision() {
    assert_eq!(native_timestamp(b"2026/09/21 01:02:02"), 0x3535_0841);
    assert_eq!(
        native_timestamp(b"2026/09/21 01:02:02"),
        native_timestamp(b"2026/09/21 01:02:03")
    );
    assert!(native_timestamp(b"2026/09/21 01:02:04") > native_timestamp(b"2026/09/21 01:02:02"));
    for version in b'2'..=b'9' {
        let mut bytes = header("2026/09/21 01:02:02");
        bytes[3] = version;
        assert!(parse_header(&bytes).unwrap().is_some());
    }
}

#[test]
fn unsafe_save_roots_preserve_catalog_and_zero_timestamp_has_no_latest_slot() {
    let (root, archive, mut vm) = setup();
    std::fs::write(
        root.path().join("save/save000.dat"),
        header("0000/00/00 00:00:00"),
    )
    .unwrap();
    archive.refresh_native_save_catalog(&mut vm, 1).unwrap();
    assert!(vm.native_save_catalog.as_ref().unwrap().load_enabled(0));
    assert_eq!(vm.native_save_catalog.as_ref().unwrap().latest_slot, None);
    vm.interpreter_words.insert(10756, 1);
    let previous = vm.clone();
    assert!(archive.refresh_native_save_catalog(&mut vm, 1).is_err());
    assert_eq!(vm, previous);
    vm.interpreter_words.remove(&10756);
    vm.interpreter_string_buffers.get_mut(&5404).unwrap()[1] =
        CmvsStringSegment::OwnedString("../".into());
    let previous = vm.clone();
    assert!(archive.refresh_native_save_catalog(&mut vm, 1).is_err());
    assert_eq!(vm, previous);
}
