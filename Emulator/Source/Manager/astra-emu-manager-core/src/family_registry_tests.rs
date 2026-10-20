use super::*;
use crate::{
    model::ProbeRequest,
    test_native::{self, Fixture},
    SessionMenuEntry,
};
use std::path::Path;

#[test]
fn static_queries_use_same_multi_probe_policy() {
    let mut registry = FamilyProviderRegistry::new();
    let mut first = Fixture::new("a", "fvp", "fvp.hcb");
    first.confidence = 8000;
    registry
        .register_provider(test_native::install(0, first))
        .unwrap();
    registry
        .register_provider(test_native::install(1, Fixture::new("b", "fvp", "fvp.hcb")))
        .unwrap();
    let selection = registry
        .probe(
            &ProbeRequest {
                game_path: "game".into(),
            },
            None,
            None,
        )
        .unwrap();
    assert!(selection.requires_user_choice());
    assert_eq!(selection.candidates()[0].report.plugin_id, "b");
}

#[test]
fn duplicate_static_id_disables_both_queries() {
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(
            0,
            Fixture::new("duplicate", "fvp", "fvp.hcb"),
        ))
        .unwrap();
    assert_eq!(
        registry
            .register_provider(test_native::install(
                1,
                Fixture::new("duplicate", "fvp", "fvp.hcb")
            ))
            .unwrap_err(),
        FamilyLoadError::DuplicatePlugin
    );
    assert!(registry.descriptors().next().is_none());
}

#[test]
fn diagnostic_initialization_failure_prevents_describe_probe_and_open() {
    let mut fixture = Fixture::new("bad-diagnostics", "fvp", "fvp.hcb");
    fixture.init_error = true;
    let calls = fixture.calls.clone();
    assert!(matches!(
        test_native::try_install(2, fixture),
        Err(FamilyLoadError::Diagnostics)
    ));
    let calls = calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "initialize");
    assert_eq!(calls[0].thread, std::thread::current().id());
    assert!(calls[0].key.is_empty());
}

fn library_path(directory: &Path, name: &str) -> std::path::PathBuf {
    directory.join(format!("{name}.{}", std::env::consts::DLL_EXTENSION))
}

fn core_directory(root: &Path, stem: &str) -> std::path::PathBuf {
    let directory = root.join(stem);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn core_subdirectory_filters_dependency_libraries_by_family_prefix() {
    let root = tempfile::tempdir().unwrap();
    let directory = core_directory(root.path(), "astra_emu_broken");
    let family = library_path(&directory, "astra_emu_broken");
    let dependency = library_path(&directory, "avcodec-62");
    std::fs::write(&family, b"not a library").unwrap();
    std::fs::write(&dependency, b"not a library").unwrap();
    std::fs::write(directory.join("astra_emu_notes.txt"), b"ignored").unwrap();

    let mut registry = FamilyProviderRegistry::new();
    let report = registry.load_directory(root.path()).unwrap();
    assert_eq!(report.loaded, 0);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].path, family);
    assert!(!is_family_library_path(&dependency));
}

#[test]
fn top_level_libraries_and_pending_area_are_not_discovered() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(library_path(root.path(), "astra_emu_legacy"), b"x").unwrap();
    let pending = core_directory(&root.path().join(".pending"), "astra_emu_next");
    std::fs::write(library_path(&pending, "astra_emu_next"), b"x").unwrap();
    let discovery = discover_family_libraries(root.path()).unwrap();
    assert!(discovery.libraries.is_empty());
    assert!(discovery.errors.is_empty());
}

#[test]
fn subdirectory_with_two_family_libraries_is_a_layout_error() {
    let root = tempfile::tempdir().unwrap();
    let directory = core_directory(root.path(), "astra_emu_pair");
    std::fs::write(library_path(&directory, "astra_emu_pair"), b"x").unwrap();
    std::fs::write(library_path(&directory, "astra_emu_other"), b"x").unwrap();
    let empty = core_directory(root.path(), "astra_emu_empty");
    let discovery = discover_family_libraries(root.path()).unwrap();
    assert!(discovery.libraries.is_empty());
    let failed: Vec<_> = discovery.errors.iter().map(|error| &error.path).collect();
    assert_eq!(failed, [&empty, &directory]);
    assert!(discovery
        .errors
        .iter()
        .all(|error| error.error.diagnostic_code() == "ASTRA_EMU_CORE_LAYOUT"));
}

#[test]
fn user_core_overrides_bundled_core_with_same_stem() {
    let user = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let user_core = core_directory(user.path(), "astra_emu_x");
    std::fs::write(library_path(&user_core, "astra_emu_x"), b"user").unwrap();
    for stem in ["astra_emu_x", "astra_emu_y"] {
        let directory = core_directory(bundled.path(), stem);
        std::fs::write(library_path(&directory, stem), b"bundled").unwrap();
    }
    let discovery =
        discover_core_directories(&[user.path().into(), bundled.path().into()]).unwrap();
    assert_eq!(
        discovery.libraries,
        [
            library_path(&user_core, "astra_emu_x"),
            library_path(&bundled.path().join("astra_emu_y"), "astra_emu_y"),
        ]
    );
}

#[test]
fn repeated_core_directory_is_discovered_once() {
    let root = tempfile::tempdir().unwrap();
    let directory = core_directory(root.path(), "astra_emu_broken");
    std::fs::write(
        library_path(&directory, "astra_emu_broken"),
        b"not a library",
    )
    .unwrap();
    let mut registry = FamilyProviderRegistry::new();
    let report = registry
        .load_directories(&[root.path().into(), root.path().into()])
        .unwrap();
    assert_eq!(report.errors.len(), 1);
}

#[test]
fn session_menu_is_copied_from_core_extension() {
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(0, Fixture::new("a", "fvp", "fvp.hcb")))
        .unwrap();
    assert_eq!(
        registry.session_menu("a"),
        [SessionMenuEntry {
            control: 0x1001,
            icon: 0,
            label: "测试动作".into(),
        }]
    );
}

#[test]
fn v3_descriptor_names_and_choice_labels_are_decoded() {
    let mut registry = FamilyProviderRegistry::new();
    let mut fixture = Fixture::new("a", "fvp", "fvp.hcb");
    fixture.config = true;
    registry
        .register_provider(test_native::install(0, fixture))
        .unwrap();
    let descriptor = registry.descriptor("a").unwrap();
    assert_eq!(
        (
            descriptor.display_name.as_str(),
            descriptor.family_name.as_str()
        ),
        ("a", "fvp")
    );
    assert_eq!(descriptor.render, None);
    let fields = registry.configuration("a").unwrap();
    assert_eq!(fields[0].choice_labels, ["直接进入脚本", "标题菜单启动"]);
    assert_eq!(fields[0].flags, crate::model::ConfigFlags::default());
    assert_eq!(fields[0].step, None);
}
