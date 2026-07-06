use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginDescriptor {
    pub id: String,
    #[schemars(with = "String")]
    pub version: Version,
    #[schemars(with = "String")]
    pub engine_version: Version,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_hash: Option<Hash256>,
    pub abi_style: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub packaged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginGate {
    pub engine_version: Version,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub required_capabilities: Vec<String>,
    pub required_permissions: Vec<String>,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("descriptor parse failed: {0}")]
    DescriptorParse(String),
    #[error("plugin gate blocked")]
    GateBlocked(Vec<Diagnostic>),
    #[error("plugin load failed: {0}")]
    Load(String),
}

impl PluginDescriptor {
    pub fn from_yaml(input: &str) -> Result<Self, PluginError> {
        serde_yaml::from_str(input).map_err(|err| PluginError::DescriptorParse(err.to_string()))
    }

    pub fn validate(&self, gate: &PluginGate) -> Result<(), PluginError> {
        let mut diagnostics = Vec::new();
        if self.engine_version != gate.engine_version {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_PLUGIN_ENGINE_VERSION",
                "plugin engine version does not match",
            ));
        }
        if self.rustc_fingerprint != gate.rustc_fingerprint {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_PLUGIN_RUSTC_FINGERPRINT",
                "plugin rustc fingerprint does not match",
            ));
        }
        if self.feature_fingerprint != gate.feature_fingerprint {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_PLUGIN_FEATURE_FINGERPRINT",
                "plugin feature fingerprint does not match",
            ));
        }
        if self.abi_style != "abi_stable_rust" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_PLUGIN_ABI_STYLE",
                "plugin abi_style must be abi_stable_rust",
            ));
        }
        for capability in &gate.required_capabilities {
            if !self.capabilities.contains(capability) {
                diagnostics.push(Diagnostic::blocking(
                    "ASTRA_PLUGIN_CAPABILITY_MISSING",
                    format!("missing capability {capability}"),
                ));
            }
        }
        for permission in &gate.required_permissions {
            if !self.permissions.contains(permission) {
                diagnostics.push(Diagnostic::blocking(
                    "ASTRA_PLUGIN_PERMISSION_MISSING",
                    format!("missing permission {permission}"),
                ));
            }
        }
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(PluginError::GateBlocked(diagnostics))
        }
    }

    pub fn validate_binary_hash(&self, actual: Hash256) -> Result<(), PluginError> {
        if self.binary_hash.is_some_and(|expected| expected != actual) {
            Err(PluginError::GateBlocked(vec![Diagnostic::blocking(
                "ASTRA_PLUGIN_BINARY_HASH",
                "plugin binary hash does not match descriptor",
            )]))
        } else {
            Ok(())
        }
    }
}
