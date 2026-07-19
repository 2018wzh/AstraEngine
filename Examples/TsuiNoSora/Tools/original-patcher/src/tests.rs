use std::{fs, path::Path};

use tempfile::tempdir;

use crate::{filesystem::collect_file_digests, manifest::PATCH_MANIFEST_NAME};

#[test]
fn manifest_is_excluded_from_verified_game_files() {
    let temp = tempdir().expect("tempdir");
    fs::write(temp.path().join("game.bin"), b"game").expect("game");
    fs::write(temp.path().join(PATCH_MANIFEST_NAME), b"manifest").expect("manifest");
    let records = collect_file_digests(temp.path(), Some(PATCH_MANIFEST_NAME)).expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].relative_path, "game.bin");
    assert!(!Path::new(&records[0].relative_path).is_absolute());
}
