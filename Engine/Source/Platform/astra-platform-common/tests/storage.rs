use std::fs;

use astra_platform::PlatformErrorCode;
use astra_platform_common::{AtomicSaveStore, FilePackageSource};

#[test]
fn save_transaction_commits_reopens_and_aborts_without_partial_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = AtomicSaveStore::new(temp.path(), "com.example.game").unwrap();

    let mut transaction = store.begin("slot-1").unwrap();
    transaction.write(&[1, 2, 3, 4]).unwrap();
    let hash = transaction.commit().unwrap();
    assert!(hash.starts_with("sha256:"));
    assert_eq!(store.read("slot-1").unwrap(), [1, 2, 3, 4]);

    let mut replacement = store.begin("slot-1").unwrap();
    replacement.write(&[5, 6]).unwrap();
    replacement.commit().unwrap();
    assert_eq!(store.read("slot-1").unwrap(), [5, 6]);
    store.delete("slot-1").unwrap();
    assert_eq!(
        store.read("slot-1").unwrap_err().code,
        PlatformErrorCode::Io
    );

    let mut dotted = store.begin("slot.01").unwrap();
    dotted.write(&[7]).unwrap();
    dotted.commit().unwrap();
    assert_eq!(store.read("slot.01").unwrap(), [7]);
    assert_eq!(store.list().unwrap(), ["slot.01"]);

    let mut aborted = store.begin("slot-2").unwrap();
    aborted.write(&[9, 9]).unwrap();
    aborted.abort().unwrap();
    assert_eq!(
        store.read("slot-2").unwrap_err().code,
        PlatformErrorCode::Io
    );
    assert!(fs::read_dir(store.root()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("tmp")));
}

#[test]
fn save_catalog_is_sorted_and_rejects_unsafe_owned_entries() {
    let temp = tempfile::tempdir().unwrap();
    let store = AtomicSaveStore::new(temp.path(), "com.example.game").unwrap();
    for slot in ["slot.10", "slot.02", "slot.quick"] {
        let mut transaction = store.begin(slot).unwrap();
        transaction.write(slot.as_bytes()).unwrap();
        transaction.commit().unwrap();
    }
    assert_eq!(store.list().unwrap(), ["slot.02", "slot.10", "slot.quick"]);

    fs::write(store.root().join("unsafe name.astrasave"), [1]).unwrap();
    assert_eq!(
        store.list().unwrap_err().code,
        PlatformErrorCode::PermissionDenied
    );
}

#[test]
fn package_source_verifies_whole_file_hash_before_range_reads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("game.astrapkg");
    fs::write(&path, [0, 1, 2, 3, 4, 5]).unwrap();
    let expected = astra_core::Hash256::from_sha256(&[0, 1, 2, 3, 4, 5]).to_string();

    let mut source = FilePackageSource::open(&path, &expected).unwrap();
    assert_eq!(source.len(), 6);
    assert_eq!(source.read_range(2, 3).unwrap(), [2, 3, 4]);
    assert_eq!(
        FilePackageSource::open(&path, "sha256:bad")
            .unwrap_err()
            .code,
        PlatformErrorCode::IntegrityMismatch
    );
}
