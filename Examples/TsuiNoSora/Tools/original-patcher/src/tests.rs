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

#[test]
fn native_save_changes_do_not_relax_distribution_verification() {
    let temp = tempdir().expect("tempdir");
    fs::write(temp.path().join("game.bin"), b"game").expect("game");
    let original = collect_file_digests(temp.path(), None).expect("original");
    let save = temp.path().join("savefile.tns");
    fs::write(&save, b"first native save").expect("save");
    let with_save = collect_file_digests(temp.path(), None).expect("with save");
    crate::verify_distribution_files(&with_save, &original).expect("new save is allowed");
    fs::write(&save, b"changed native save").expect("change save");
    let changed = collect_file_digests(temp.path(), None).expect("changed");
    crate::verify_distribution_files(&changed, &with_save).expect("save is mutable");
    assert_eq!(
        fs::read(&save).expect("save retained"),
        b"changed native save"
    );
    crate::verify_distribution_files(&original, &with_save).expect("save is optional");
    fs::write(temp.path().join("unexpected.dll"), b"extra").expect("extra");
    let extra = collect_file_digests(temp.path(), None).expect("extra files");
    assert!(crate::verify_distribution_files(&extra, &original).is_err());
    fs::remove_file(temp.path().join("unexpected.dll")).expect("remove test extra");
    fs::write(temp.path().join("game.bin"), b"changed game").expect("changed game");
    let changed_game = collect_file_digests(temp.path(), None).expect("changed game files");
    assert!(crate::verify_distribution_files(&changed_game, &original).is_err());
}
