use std::{path::Path, process::Command};

use astra_plugin::{dylib_path, PluginGate, PluginLoader, PluginRegistrar};
use astra_vn_plugin::{VnExtensionBinding, VnExtensionManifest};
use semver::Version;

#[astra_headless_test::test]
fn standard_vn_extension_manifest_declares_required_bindings() {
    let manifest = VnExtensionManifest::standard();
    let report = manifest.validate_required();

    assert!(report.passed, "{report:?}");
    assert!(manifest.has_binding("astra.vn.policy_bundle_provider"));
    assert!(manifest.has_binding("astra.vn.command_provider"));
    assert!(manifest.has_binding("astra.vn.presentation_command_provider"));
    assert!(manifest.has_binding("astra.vn.editor_metadata_provider"));
    assert!(manifest.has_binding("astra.vn.release_check_provider"));
}

#[astra_headless_test::test]
fn missing_vn_extension_binding_blocks_validation() {
    let mut manifest = VnExtensionManifest::standard();
    manifest
        .bindings
        .retain(|binding| binding.extension_point != "astra.vn.presentation_command_provider");

    let report = manifest.validate_required();

    assert!(!report.passed);
    assert_eq!(
        report.diagnostics[0].code,
        "ASTRA_VN_EXTENSION_BINDING_MISSING"
    );
}

#[astra_headless_test::test]
fn duplicate_vn_extension_binding_blocks_validation() {
    let mut manifest = VnExtensionManifest::standard();
    manifest.bindings.push(VnExtensionBinding {
        extension_point: "astra.vn.command_provider".to_string(),
        provider_id: "astra.vn.standard.alt".to_string(),
        required_capabilities: vec!["astra.vn.command".to_string()],
    });

    let report = manifest.validate_required();

    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_EXTENSION_BINDING_DUPLICATE"));
}

#[astra_headless_test::test]
fn vn_extension_manifest_accepts_real_cdylib_provider_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(5)
        .unwrap();
    let status = Command::new("cargo")
        .args(["build", "-p", "vn-extension-provider"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());

    let mut registrar = PluginRegistrar::default();
    let loader = PluginLoader::new(PluginGate {
        engine_version: Version::parse("0.1.0").unwrap(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "stage3-vn".to_string(),
        abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        required_capabilities: vec![
            "astra.vn.policy_bundle".to_string(),
            "astra.vn.command".to_string(),
            "astra.vn.presentation_command".to_string(),
            "astra.vn.editor_metadata".to_string(),
            "astra.vn.release_check".to_string(),
        ],
        required_permissions: vec!["runtime.vn".to_string()],
    });
    let plugin = loader
        .load(dylib_path(root, "vn_extension_provider"), &mut registrar)
        .unwrap();

    assert_eq!(
        plugin.descriptor().id,
        "astra.fixture.vn_extension_provider"
    );
    let snapshot = registrar.extension_registry_snapshot().unwrap();
    let manifest = VnExtensionManifest {
        schema: "astra.vn.extension_manifest.v1".to_string(),
        bindings: snapshot
            .providers
            .into_iter()
            .map(|provider| VnExtensionBinding {
                extension_point: provider.slot,
                provider_id: provider.provider_id,
                required_capabilities: vec![provider.capability],
            })
            .collect(),
    };

    let report = manifest.validate_required();
    assert!(report.passed, "{report:?}");

    let unload = plugin.unload_from(&mut registrar).unwrap();
    assert_eq!(unload.status, "unloaded");
    assert!(registrar.extensions.providers().is_empty());
}
