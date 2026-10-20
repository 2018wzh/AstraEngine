use super::*;
use zip::write::SimpleFileOptions;

const LIBRARY: &str = "astra_emu_fixture.dll";

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn manifest(plugin_id: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut files: Vec<CoreFile> = files
        .iter()
        .map(|(path, bytes)| CoreFile {
            path: (*path).into(),
            sha256: sha256_hex(bytes),
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&CoreManifest {
        schema: CORE_PACKAGE_SCHEMA.into(),
        plugin_id: plugin_id.into(),
        library: LIBRARY.into(),
        version: "0.2.0".into(),
        abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        files,
    })
    .unwrap()
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        writer.start_file(*name, options()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn valid_files() -> Vec<(&'static str, &'static [u8])> {
    vec![
        (LIBRARY, b"new library".as_slice()),
        ("avcodec-62.dll", b"dependency"),
        ("licenses/LICENSE.txt", b"license"),
    ]
}

fn valid_package(path: &Path, plugin_id: &str) {
    let files = valid_files();
    let manifest = manifest(plugin_id, &files);
    let mut entries = files;
    entries.push((CORE_MANIFEST, &manifest));
    write_zip(path, &entries);
}

fn pending_is_empty(cores: &Path) -> bool {
    fs::read_dir(cores.join(PENDING)).map_or(true, |mut entries| entries.next().is_none())
}

#[test]
fn package_with_parent_directory_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let mut files = valid_files();
    files.push(("../evil.dll", b"escape"));
    let manifest = manifest("astra.emu.fixture", &valid_files());
    files.push((CORE_MANIFEST, &manifest));
    write_zip(&archive, &files);
    let error =
        stage_package(&archive, Some("astra.emu.fixture"), &cores.join(PENDING)).unwrap_err();
    assert_eq!(error, "ASTRA_EMU_CORE_PACKAGE_PATH");
    assert!(!root.path().join("evil.dll").exists());
    assert!(pending_is_empty(&cores));
}

#[test]
fn package_with_absolute_symlink_or_duplicate_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let pending = root.path().join("cores").join(PENDING);
    let archive = root.path().join("core.zip");
    write_zip(&archive, &[("/abs.dll", b"x")]);
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "ASTRA_EMU_CORE_PACKAGE_PATH"
    );
    let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    writer
        .add_symlink("link.dll", "../../outside", options())
        .unwrap();
    writer.finish().unwrap();
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "ASTRA_EMU_CORE_PACKAGE_PATH"
    );
    let mut files = valid_files();
    files.push(("AVCODEC-62.DLL", b"other"));
    write_zip(&archive, &files);
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "ASTRA_EMU_CORE_PACKAGE_DUPLICATE"
    );
}

#[test]
fn package_with_hash_mismatch_or_foreign_plugin_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let manifest = manifest("astra.emu.fixture", &valid_files());
    let mut files = valid_files();
    files[0].1 = b"tampered library";
    files.push((CORE_MANIFEST, &manifest));
    write_zip(&archive, &files);
    let error =
        stage_package(&archive, Some("astra.emu.fixture"), &cores.join(PENDING)).unwrap_err();
    assert!(
        error.starts_with("ASTRA_EMU_CORE_PACKAGE_HASH_MISMATCH"),
        "{error}"
    );
    assert!(pending_is_empty(&cores));

    valid_package(&archive, "astra.emu.fixture");
    assert_eq!(
        stage_package(&archive, Some("astra.emu.other"), &cores.join(PENDING)).unwrap_err(),
        "ASTRA_EMU_CORE_PACKAGE_PLUGIN_MISMATCH"
    );
    assert!(pending_is_empty(&cores));
}

#[test]
fn valid_package_is_staged_and_apply_pending_replaces_the_core_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let installed = cores.join("astra_emu_fixture");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(LIBRARY), b"old library").unwrap();
    fs::write(installed.join("stale-dependency.dll"), b"old").unwrap();
    let archive = root.path().join("core.zip");
    valid_package(&archive, "astra.emu.fixture");

    let (stem, staged) =
        stage_package(&archive, Some("astra.emu.fixture"), &cores.join(PENDING)).unwrap();
    assert_eq!(stem, "astra_emu_fixture");
    assert_eq!(staged.version, "0.2.0");
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([(stem.clone(), PendingKind::Install)])
    );
    assert_eq!(
        fs::read(cores.join(PENDING).join(&stem).join(LIBRARY)).unwrap(),
        b"new library"
    );

    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(LIBRARY)).unwrap(), b"new library");
    assert_eq!(
        fs::read(installed.join("licenses/LICENSE.txt")).unwrap(),
        b"license"
    );
    assert!(!installed.join("stale-dependency.dll").exists());
    assert_eq!(
        read_manifest(&installed).unwrap().plugin_id,
        "astra.emu.fixture"
    );
    assert!(pending_is_empty(&cores));
}

#[test]
fn removal_marker_deletes_the_core_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    fs::create_dir_all(cores.join("astra_emu_y")).unwrap();
    fs::write(cores.join("astra_emu_y").join("astra_emu_y.dll"), b"y").unwrap();
    mark_removal(&cores, "astra_emu_y").unwrap();
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([("astra_emu_y".to_owned(), PendingKind::Remove)])
    );
    assert!(apply_pending(&cores).is_empty());
    assert!(!cores.join("astra_emu_y").exists());
    assert!(pending_is_empty(&cores));
}

#[test]
fn legacy_top_level_library_is_migrated_into_its_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    fs::create_dir_all(&cores).unwrap();
    fs::write(cores.join("astra_emu_x.dll"), b"legacy").unwrap();
    fs::write(cores.join("avcodec-62.dll"), b"dependency").unwrap();
    assert!(apply_pending(&cores).is_empty());
    assert!(!cores.join("astra_emu_x.dll").exists());
    assert_eq!(
        fs::read(cores.join("astra_emu_x").join("astra_emu_x.dll")).unwrap(),
        b"legacy"
    );
    let manifest = read_manifest(&cores.join("astra_emu_x")).unwrap();
    assert_eq!(manifest.library, "astra_emu_x.dll");
    assert_eq!(manifest.files[0].sha256, sha256_hex(b"legacy"));
    assert!(cores.join("avcodec-62.dll").exists());
}

#[test]
fn single_library_install_generates_a_manifest() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let library = root.path().join(LIBRARY);
    fs::write(&library, b"bare").unwrap();
    let stem = stage_library(&library, &cores.join(PENDING)).unwrap();
    let staged = cores.join(PENDING).join(&stem);
    assert_eq!(fs::read(staged.join(LIBRARY)).unwrap(), b"bare");
    let manifest = read_manifest(&staged).unwrap();
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].sha256, sha256_hex(b"bare"));
    assert_eq!(
        stage_library(&root.path().join("avcodec-62.dll"), &cores.join(PENDING)).unwrap_err(),
        "ASTRA_EMU_CORE_INSTALL_NAME"
    );
}

#[cfg(windows)]
#[test]
fn failed_install_restores_the_previous_directory() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let installed = cores.join("astra_emu_fixture");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(LIBRARY), b"installed").unwrap();
    let staged = cores.join(PENDING).join("astra_emu_fixture");
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join(LIBRARY), b"replacement").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(staged.join(LIBRARY))
        .unwrap();
    assert_eq!(apply_pending(&cores).len(), 1);
    assert_eq!(fs::read(installed.join(LIBRARY)).unwrap(), b"installed");
    drop(locked);
    assert_eq!(fs::read(staged.join(LIBRARY)).unwrap(), b"replacement");
    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(LIBRARY)).unwrap(), b"replacement");
    assert!(pending_is_empty(&cores));
}
