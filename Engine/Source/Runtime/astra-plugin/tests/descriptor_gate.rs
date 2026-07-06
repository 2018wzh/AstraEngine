use astra_core::Hash256;
use astra_plugin::{PluginDescriptor, PluginError, PluginGate};
use semver::Version;

const DESCRIPTOR: &str = r#"
id: com.example.renderer
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-stable
feature_fingerprint: stage1-core
abi_style: abi_stable_rust
capabilities:
  - presentation.headless
permissions:
  - runtime.presentation
packaged: true
"#;

#[test]
fn descriptor_gate_rejects_mismatch_and_missing_permission() {
    let descriptor = PluginDescriptor::from_yaml(DESCRIPTOR).unwrap();
    let gate = PluginGate {
        engine_version: Version::parse("0.1.0").unwrap(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "stage1-core".to_string(),
        required_capabilities: vec!["presentation.headless".to_string()],
        required_permissions: vec!["runtime.presentation".to_string()],
    };
    descriptor.validate(&gate).unwrap();

    let blocked = PluginGate {
        engine_version: Version::parse("0.2.0").unwrap(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "stage1-core".to_string(),
        required_capabilities: vec!["presentation.headless".to_string()],
        required_permissions: vec!["gpu.surface".to_string()],
    };
    let err = descriptor.validate(&blocked).unwrap_err();
    match err {
        PluginError::GateBlocked(diagnostics) => {
            assert!(diagnostics
                .iter()
                .any(|d| d.code == "ASTRA_PLUGIN_ENGINE_VERSION"));
            assert!(diagnostics
                .iter()
                .any(|d| d.code == "ASTRA_PLUGIN_PERMISSION_MISSING"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn descriptor_gate_rejects_binary_hash_mismatch() {
    let mut descriptor = PluginDescriptor::from_yaml(DESCRIPTOR).unwrap();
    descriptor.binary_hash = Some(Hash256::from_sha256(b"expected"));
    let err = descriptor
        .validate_binary_hash(Hash256::from_sha256(b"actual"))
        .unwrap_err();
    match err {
        PluginError::GateBlocked(diagnostics) => {
            assert_eq!(diagnostics[0].code, "ASTRA_PLUGIN_BINARY_HASH");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn descriptor_gate_requires_abi_stable_rust() {
    let mut descriptor = PluginDescriptor::from_yaml(DESCRIPTOR).unwrap();
    descriptor.abi_style = "raw_c".to_string();
    let gate = PluginGate {
        engine_version: Version::parse("0.1.0").unwrap(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "stage1-core".to_string(),
        required_capabilities: vec!["presentation.headless".to_string()],
        required_permissions: vec!["runtime.presentation".to_string()],
    };
    let err = descriptor.validate(&gate).unwrap_err();
    match err {
        PluginError::GateBlocked(diagnostics) => {
            assert_eq!(diagnostics[0].code, "ASTRA_PLUGIN_ABI_STYLE");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
