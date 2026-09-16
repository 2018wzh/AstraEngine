use astra_emu_sdk::{read_game_profile, resolve_game_file};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u32,
}

#[test]
fn private_profile_and_resources_stay_inside_the_game_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(root.path().join("profile.json"), br#"{"version":1}"#).unwrap();
    let (profile, _, canonical) =
        read_game_profile::<Profile>(root.path(), Path::new("profile.json"), 64).unwrap();
    assert_eq!(profile.version, 1);
    assert_eq!(canonical, root.path().canonicalize().unwrap());
    assert_eq!(
        read_game_profile::<Profile>(root.path(), outside.path(), 64)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_PROFILE_PATH"
    );
    for invalid in [
        "../outside",
        "/absolute",
        "C:/outside",
        "a\\b",
        "a//b",
        "./profile.json",
    ] {
        assert!(resolve_game_file(root.path(), invalid).is_err());
    }
    assert_eq!(
        resolve_game_file(root.path(), "profile.json").unwrap(),
        canonical.join("profile.json")
    );
}

#[test]
fn malformed_or_oversized_profiles_are_preserved_and_not_echoed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profile.json");
    let bytes = br#"{"unexpected":"private-input"}"#;
    std::fs::write(&path, bytes).unwrap();
    let error = read_game_profile::<Profile>(root.path(), &path, 64).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_PROFILE_FORMAT");
    assert!(!error.to_string().contains("private-input"));
    assert_eq!(
        read_game_profile::<Profile>(root.path(), &path, 4)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_PROFILE_BOUND"
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn relative_game_root_resolves_profile_once_without_changing_working_directory() {
    let cwd = std::env::current_dir().unwrap();
    let root = tempfile::tempdir_in(&cwd).unwrap();
    let relative_root = root.path().strip_prefix(&cwd).unwrap();
    std::fs::create_dir(root.path().join("config")).unwrap();
    std::fs::write(root.path().join("config/profile.json"), br#"{"version":1}"#).unwrap();
    let (profile, _, canonical) =
        read_game_profile::<Profile>(relative_root, Path::new("config/profile.json"), 64).unwrap();
    assert_eq!(profile.version, 1);
    assert_eq!(canonical, root.path().canonicalize().unwrap());
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}
