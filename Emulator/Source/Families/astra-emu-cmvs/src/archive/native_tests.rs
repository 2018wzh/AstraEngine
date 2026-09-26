use super::*;

fn fixture() -> (tempfile::TempDir, CmvsArchive, CmvsPs2aVmState) {
    let (root, mut archive) = super::super::cache_tests::fixture();
    archive.archives[0].path = archive.archives[0].path.canonicalize().unwrap();
    archive
        .bind_game_root(root.path().canonicalize().unwrap())
        .unwrap();
    (root, archive, CmvsPs2aVmState::new(0))
}

#[test]
fn native_boot_config_and_script_calls_share_slot_zero_without_archive_scan() {
    let (root, mut archive, mut vm) = fixture();
    archive
        .entries
        .values_mut()
        .next()
        .unwrap()
        .descriptor
        .relative_path = "start.ps3".into();
    std::fs::create_dir(root.path().join("boot")).unwrap();
    std::fs::write(root.path().join("boot/start.ps3"), b"loose boot").unwrap();
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nSCRIPT_INIT_PATH=boot\\\n",
    )
    .unwrap();
    archive.initialize_native_script_search(&mut vm).unwrap();
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "start.ps3")
        .unwrap();
    assert_eq!(
        archive.read_range(&uri, 0, 10).unwrap().bytes.as_slice(),
        b"loose boot"
    );
    let before = vm.clone();
    assert!(archive.load_native_script("start.ps3", 0, &mut vm).is_err());
    assert_eq!(vm, before); // bad selected loose script cannot fall back to CPZ
    vm.resource_search_paths
        .get_mut(&CmvsResourceSearchBank::Script)
        .unwrap()
        .insert(1, "archive.cpz".into());
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "start.ps3")
            .unwrap(),
        uri
    );
    // Command 16 replaces the same slot; later calls now resolve to CPZ.
    vm.resource_search_paths
        .get_mut(&CmvsResourceSearchBank::Script)
        .unwrap()
        .insert(0, "archive.cpz".into());
    let packed = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "start.ps3")
        .unwrap();
    assert_eq!(
        archive.read_range(&packed, 0, 7).unwrap().bytes.as_slice(),
        b"decoded"
    );
    vm.resource_search_paths.clear();
    std::fs::write(
        root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nSCRIPT_INIT_PATH=absent/\n",
    )
    .unwrap();
    archive.initialize_native_script_search(&mut vm).unwrap();
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "start.ps3")
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_RESOURCE_NOT_FOUND"
    );
}

#[test]
fn missing_native_config_uses_game_root_and_invalid_config_is_atomic() {
    let (root, archive, mut vm) = fixture();
    std::fs::write(root.path().join("start.ps3"), b"root").unwrap();
    archive.initialize_native_script_search(&mut vm).unwrap();
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "start.ps3")
        .unwrap();
    assert_eq!(
        archive.read_range(&uri, 0, 4).unwrap().bytes.as_slice(),
        b"root"
    );
    let before = vm.clone();
    for value in ["../outside/".to_owned(), "a".repeat(256)] {
        std::fs::write(
            root.path().join("cmvs.cfg"),
            format!("[CMVS_SYSTEM_MAIN]\nSCRIPT_INIT_PATH={value}\n"),
        )
        .unwrap();
        assert!(archive.initialize_native_script_search(&mut vm).is_err());
        assert_eq!(vm, before);
    }
}

#[test]
fn source_domains_and_slot_priority_control_real_file_reads() {
    let (root, archive, mut vm) = fixture();
    std::fs::create_dir(root.path().join("override")).unwrap();
    std::fs::write(root.path().join("override/test.bin"), b"native bytes").unwrap();
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, "missing/".into()), (1, "archive.cpz".into())]),
    );
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Texture,
        BTreeMap::from([(0, "override\\".into()), (1, "archive.cpz".into())]),
    );
    let script = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
        .unwrap();
    let image = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "test.bin")
        .unwrap();
    assert_eq!(
        archive.read_range(&script, 0, 7).unwrap().bytes.as_slice(),
        b"decoded"
    );
    assert_eq!(
        archive.read_range(&image, 0, 6).unwrap().bytes.as_slice(),
        b"native"
    );
    assert_eq!(archive.stat(&image).unwrap().size, 12);
    assert!(archive.read_range(&image, 10, 3).is_err());
    let restored = postcard::from_bytes(&postcard::to_allocvec(&vm).unwrap()).unwrap();
    assert_eq!(
        archive
            .resolve_native_resource(&restored, CmvsResourceSearchBank::Texture, "test.bin")
            .unwrap(),
        image
    );
    vm.resource_search_paths
        .get_mut(&CmvsResourceSearchBank::Texture)
        .unwrap()
        .insert(0, "archive.cpz".into());
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "test.bin")
            .unwrap(),
        script
    );
}

#[test]
fn native_reader_bounds_and_missing_banks_do_not_scan_unrelated_archives() {
    let (_root, archive, mut vm) = fixture();
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_RESOURCE_UNBOUND"
    );
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(4, "archive.cpz".into())]),
    );
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_RESOURCE_NOT_FOUND"
    );
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Texture,
        BTreeMap::from([(9, "archive.cpz".into())]),
    );
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "TEST.BIN")
            .unwrap(),
        "cmvs:/test.bin"
    );
}

#[test]
fn archive_directory_search_follows_native_index_order() {
    let (_root, mut archive, mut vm) = fixture();
    let mut earlier = archive.entries["cmvs:/test.bin"].clone();
    earlier.uri = "cmvs:/z/test.bin".into();
    earlier.descriptor.relative_path = "z/test.bin".into();
    earlier.ordinal = 0;
    archive.entries.get_mut("cmvs:/test.bin").unwrap().ordinal = 1;
    archive.entries.insert(earlier.uri.clone(), earlier);
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, "archive.cpz".into())]),
    );
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
            .unwrap(),
        "cmvs:/z/test.bin"
    );
}

#[test]
fn same_native_name_in_two_archives_uses_configured_slot_order() {
    let (root, mut archive, mut vm) = fixture();
    let patch_path = root.path().join("patch.cpz");
    std::fs::write(&patch_path, b"patch source").unwrap();
    let mut patch = archive.archives[0].clone();
    patch.path = patch_path.canonicalize().unwrap();
    patch.role = "patch".into();
    archive.archives.push(patch);
    let mut entry = archive.entries["cmvs:/test.bin"].clone();
    entry.archive = 1;
    entry.uri = "cmvs:/archive/patch/test.bin".into();
    archive.entries.insert(entry.uri.clone(), entry);
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, "patch.cpz".into()), (1, "archive.cpz".into())]),
    );
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
            .unwrap(),
        "cmvs:/archive/patch/test.bin"
    );
    vm.resource_search_paths
        .get_mut(&CmvsResourceSearchBank::Script)
        .unwrap()
        .insert(0, "archive.cpz".into());
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
            .unwrap(),
        "cmvs:/test.bin"
    );
}

#[test]
fn unknown_archive_and_unsafe_paths_are_explicit_errors() {
    let (root, archive, mut vm) = fixture();
    std::fs::write(root.path().join("unconfigured.cpz"), b"unknown").unwrap();
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, "unconfigured.cpz".into())]),
    );
    assert_eq!(
        archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Script, "test.bin")
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_RESOURCE_ARCHIVE"
    );
    for path in [
        "../x",
        "dir/../x",
        "/x",
        "\\server\\file",
        "C:\\x",
        "a//x",
        "x\0",
    ] {
        assert!(validate_search_path(path).is_err());
    }
    assert!(validate_search_path(&"a".repeat(2048)).is_err());
    assert!(archive.stat("cmvs:/native-files/../outside").is_err());
    assert!(validate_search_path("").is_ok());
}
