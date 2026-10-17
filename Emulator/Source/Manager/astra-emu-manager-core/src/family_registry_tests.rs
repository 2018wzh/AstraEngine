use super::*;
use crate::{model::ProbeRequest, test_native::{self, Fixture}};
use std::path::Path;

#[test]
fn static_queries_use_same_multi_probe_policy() {
    let mut registry = FamilyProviderRegistry::new();
    let mut first = Fixture::new("a", "fvp", "fvp.hcb");
    first.confidence = 8000;
    registry.register_provider(test_native::install(0, first)).unwrap();
    registry.register_provider(test_native::install(1, Fixture::new("b", "fvp", "fvp.hcb"))).unwrap();
    let selection = registry.probe(&ProbeRequest { game_path: "game".into() }, None, None).unwrap();
    assert!(selection.requires_user_choice());
    assert_eq!(selection.candidates()[0].report.plugin_id, "b");
}

#[test]
fn duplicate_static_id_disables_both_queries() {
    let mut registry = FamilyProviderRegistry::new();
    registry.register_provider(test_native::install(0, Fixture::new("duplicate", "fvp", "fvp.hcb"))).unwrap();
    assert_eq!(registry.register_provider(test_native::install(1, Fixture::new("duplicate", "fvp", "fvp.hcb"))).unwrap_err(), FamilyLoadError::DuplicatePlugin);
    assert!(registry.descriptors().next().is_none());
}

#[test]
fn diagnostic_initialization_failure_prevents_describe_probe_and_open() {
    let mut fixture = Fixture::new("bad-diagnostics", "fvp", "fvp.hcb");
    fixture.init_error = true;
    let calls = fixture.calls.clone();
    assert!(matches!(test_native::try_install(2, fixture), Err(FamilyLoadError::Diagnostics)));
    let calls = calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "initialize");
    assert_eq!(calls[0].thread, std::thread::current().id());
    assert!(calls[0].key.is_empty());
}

fn library_path(directory: &Path, name: &str) -> std::path::PathBuf {
    directory.join(format!("{name}.{}", std::env::consts::DLL_EXTENSION))
}

#[test]
fn core_directory_filters_dependency_libraries_by_family_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let family = library_path(directory.path(), "astra_emu_broken");
    let dependency = library_path(directory.path(), "avcodec-62");
    std::fs::write(&family, b"not a library").unwrap();
    std::fs::write(&dependency, b"not a library").unwrap();
    std::fs::write(directory.path().join("astra_emu_notes.txt"), b"ignored").unwrap();

    let mut registry = FamilyProviderRegistry::new();
    let report = registry.load_directory(directory.path()).unwrap();
    assert_eq!(report.loaded, 0);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].path, family);
    assert!(!is_family_library_path(&dependency));
}

#[test]
fn repeated_core_directory_is_discovered_once() {
    let directory = tempfile::tempdir().unwrap();
    let family = library_path(directory.path(), "astra_emu_broken");
    std::fs::write(&family, b"not a library").unwrap();
    let mut registry = FamilyProviderRegistry::new();
    let report = registry
        .load_directories(&[directory.path().into(), directory.path().into()])
        .unwrap();
    assert_eq!(report.errors.len(), 1);
}

#[test]
#[ignore = "requires explicitly built Family plugin binaries"]
fn core_directory_rejects_duplicate_plugin_ids_as_a_group() {
    let first = std::env::var_os("ASTRA_EMU_TEST_PLUGIN_A")
        .expect("ASTRA_EMU_TEST_PLUGIN_A must identify a plugin");
    let second = std::env::var_os("ASTRA_EMU_TEST_PLUGIN_B")
        .expect("ASTRA_EMU_TEST_PLUGIN_B must identify a plugin");
    let directory = tempfile::tempdir().unwrap();
    let bundled = directory.path().join("bundled");
    let user = directory.path().join("user");
    std::fs::create_dir_all(&bundled).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    let first_path = library_path(&bundled, "astra_emu_first");
    let second_path = library_path(&user, "astra_emu_second");
    std::fs::copy(first, &first_path).unwrap();
    std::fs::copy(second, &second_path).unwrap();

    let mut registry = FamilyProviderRegistry::new();
    let report = registry.load_directories(&[bundled, user]).unwrap();
    assert_eq!(report.loaded, 0);
    assert_eq!(report.errors.len(), 2);
    assert!(report
        .errors
        .iter()
        .all(|entry| entry.error == FamilyLoadError::DuplicatePlugin));
    assert!(registry.descriptors().next().is_none());
}
