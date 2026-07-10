use std::collections::{BTreeMap, BTreeSet};

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnExtensionManifest {
    pub schema: String,
    pub bindings: Vec<VnExtensionBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnExtensionBinding {
    pub extension_point: String,
    pub provider_id: String,
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnExtensionValidationReport {
    pub schema: String,
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl VnExtensionManifest {
    pub fn standard() -> Self {
        tracing::info!(
            event = "vn.plugin.extensions.create",
            "AstraVN standard extension bindings created"
        );
        Self {
            schema: "astra.vn.extension_manifest.v1".to_string(),
            bindings: vec![
                binding(
                    "astra.vn.policy_bundle_provider",
                    "astra.vn.standard_policy",
                    ["astra.vn.policy_bundle"],
                ),
                binding(
                    "astra.vn.command_provider",
                    "astra.vn.standard_commands",
                    ["astra.vn.command"],
                ),
                binding(
                    "astra.vn.presentation_command_provider",
                    "astra.vn.standard_presentation",
                    ["astra.vn.presentation_command"],
                ),
                binding(
                    "astra.vn.editor_metadata_provider",
                    "astra.vn.editor_metadata",
                    ["astra.vn.editor_metadata"],
                ),
                binding(
                    "astra.vn.release_check_provider",
                    "astra.vn.release_checks",
                    ["astra.vn.release_check"],
                ),
            ],
        }
    }

    pub fn has_binding(&self, extension_point: &str) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.extension_point == extension_point)
    }

    pub fn validate_required(&self) -> VnExtensionValidationReport {
        tracing::debug!(
            event = "vn.plugin.extensions.validate.start",
            binding_count = self.bindings.len(),
            "AstraVN extension validation started"
        );
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.extension_manifest.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_EXTENSION_MANIFEST_SCHEMA",
                "VN extension manifest schema is invalid",
            ));
        }

        let mut by_point = BTreeMap::<&str, Vec<&VnExtensionBinding>>::new();
        for binding in &self.bindings {
            by_point
                .entry(binding.extension_point.as_str())
                .or_default()
                .push(binding);
        }

        for required in required_extension_points() {
            match by_point.get(required.id) {
                None => diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_EXTENSION_BINDING_MISSING",
                        "required VN extension binding is missing",
                    )
                    .with_field("extension_point", required.id),
                ),
                Some(bindings) if bindings.len() > 1 => diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_EXTENSION_BINDING_DUPLICATE",
                        "VN extension point must have exactly one explicit provider binding",
                    )
                    .with_field("extension_point", required.id)
                    .with_field("count", bindings.len()),
                ),
                Some(bindings) => {
                    let binding = bindings[0];
                    let capabilities: BTreeSet<_> = binding
                        .required_capabilities
                        .iter()
                        .map(String::as_str)
                        .collect();
                    if !capabilities.contains(required.capability) {
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_EXTENSION_CAPABILITY_MISSING",
                                "VN extension binding is missing its required capability",
                            )
                            .with_field("extension_point", required.id)
                            .with_field("capability", required.capability),
                        );
                    }
                }
            }
        }

        VnExtensionValidationReport {
            schema: "astra.vn.extension_validation_report.v1".to_string(),
            passed: diagnostics.is_empty(),
            diagnostics,
        }
    }
}

struct RequiredExtensionPoint {
    id: &'static str,
    capability: &'static str,
}

fn required_extension_points() -> [RequiredExtensionPoint; 5] {
    [
        RequiredExtensionPoint {
            id: "astra.vn.policy_bundle_provider",
            capability: "astra.vn.policy_bundle",
        },
        RequiredExtensionPoint {
            id: "astra.vn.command_provider",
            capability: "astra.vn.command",
        },
        RequiredExtensionPoint {
            id: "astra.vn.presentation_command_provider",
            capability: "astra.vn.presentation_command",
        },
        RequiredExtensionPoint {
            id: "astra.vn.editor_metadata_provider",
            capability: "astra.vn.editor_metadata",
        },
        RequiredExtensionPoint {
            id: "astra.vn.release_check_provider",
            capability: "astra.vn.release_check",
        },
    ]
}

fn binding<const N: usize>(
    extension_point: &str,
    provider_id: &str,
    required_capabilities: [&str; N],
) -> VnExtensionBinding {
    VnExtensionBinding {
        extension_point: extension_point.to_string(),
        provider_id: provider_id.to_string(),
        required_capabilities: required_capabilities
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}
