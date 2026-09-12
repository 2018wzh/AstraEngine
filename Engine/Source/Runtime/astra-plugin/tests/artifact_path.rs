use std::path::Path;

use astra_plugin::dylib_path_for_target;

#[test]
fn dylib_path_resolves_relative_cargo_target_dir_from_workspace_root() {
    let root = Path::new("workspace");
    let path = dylib_path_for_target(
        root,
        Some(Path::new("artifacts/batch0")),
        "debug",
        "fixture_provider",
    );

    let extension = if cfg!(target_os = "windows") {
        "fixture_provider.dll"
    } else if cfg!(target_os = "macos") {
        "libfixture_provider.dylib"
    } else {
        "libfixture_provider.so"
    };
    assert_eq!(
        path,
        root.join("artifacts")
            .join("batch0")
            .join("debug")
            .join(extension)
    );
}

#[test]
fn dylib_path_preserves_absolute_cargo_target_dir() {
    let root = Path::new("workspace");
    let target = if cfg!(target_os = "windows") {
        Path::new("C:/isolated/astra-target")
    } else {
        Path::new("/isolated/astra-target")
    };
    let path = dylib_path_for_target(root, Some(target), "release", "fixture_provider");

    assert!(path.starts_with(target));
    assert_eq!(path.parent().unwrap(), target.join("release"));
}

#[test]
fn dylib_path_defaults_to_workspace_target_directory() {
    let root = Path::new("workspace");
    let path = dylib_path_for_target(root, None, "debug", "fixture_provider");

    assert!(path.starts_with(root.join("target").join("debug")));
}
