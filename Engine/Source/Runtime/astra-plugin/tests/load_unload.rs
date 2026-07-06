use std::{path::Path, process::Command};

use astra_plugin::{dylib_path, EngineModuleSlot, PluginGate, PluginLoader, PluginRegistrar};
use semver::Version;

#[test]
fn load_unload_loads_fixture_cdylib_and_releases_callbacks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let status = Command::new("cargo")
        .args(["build", "-p", "headless-presentation-provider"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());

    let mut registrar = PluginRegistrar::default();
    let loader = PluginLoader::new(PluginGate {
        engine_version: Version::parse("0.1.0").unwrap(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "stage1-core".to_string(),
        required_capabilities: vec!["presentation.headless".to_string()],
        required_permissions: vec!["runtime.presentation".to_string()],
    });
    let plugin = loader
        .load(
            dylib_path(root, "headless_presentation_provider"),
            &mut registrar,
        )
        .unwrap();
    assert_eq!(
        plugin.descriptor().id,
        "astra.fixture.headless_presentation"
    );
    assert_eq!(registrar.extensions.providers().len(), 1);
    assert_eq!(
        registrar
            .selected_provider(&EngineModuleSlot("presentation".to_string()))
            .unwrap()
            .provider_id,
        "astra.fixture.headless_presentation"
    );
    let report = plugin.unload_from(&mut registrar).unwrap();
    assert_eq!(report.status, "unloaded");
    assert!(report.callbacks_released);
    assert!(registrar.extensions.providers().is_empty());
    assert!(registrar.services.get("presentation").is_none());
}
