use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const VN_POLICY_BUNDLE_SOURCE_CACHE_SECTION: &str = "vn.policy_bundle_source_cache";
pub const STANDARD_POLICY_BUNDLE_ID: &str = "astra.policy.standard";
pub const STANDARD_POLICY_ENTRY: &str = "Policies/standard_policy.luau";

const STANDARD_POLICY_SOURCE: &str = include_str!("standard_policy.luau");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyBundleManifest {
    pub schema: String,
    pub bundles: Vec<VnPolicyBundleEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyBundleEntry {
    pub id: String,
    pub entry: String,
    pub capabilities: Vec<String>,
    pub dependencies: Vec<String>,
    pub lock_hash: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub byte_size: u64,
    #[serde(default)]
    pub source_cache_section: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyBundleSourceCache {
    pub schema: String,
    pub bundles: Vec<VnPolicyBundleSourceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyBundleSourceEntry {
    pub id: String,
    pub entry: String,
    pub source_hash: String,
    pub byte_size: u64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyBundleValidationReport {
    pub schema: String,
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl VnPolicyBundleManifest {
    pub fn standard() -> Self {
        let source_hash = standard_policy_source_hash();
        Self {
            schema: "astra.policy_bundle.v1".to_string(),
            bundles: vec![VnPolicyBundleEntry {
                id: STANDARD_POLICY_BUNDLE_ID.to_string(),
                entry: STANDARD_POLICY_ENTRY.to_string(),
                capabilities: vec![
                    "astra.vn.message_ui".to_string(),
                    "astra.vn.choice_ui".to_string(),
                    "astra.vn.system_story".to_string(),
                    "astra.vn.presentation_timeline".to_string(),
                ],
                dependencies: Vec::new(),
                lock_hash: source_hash.clone(),
                source_hash,
                byte_size: STANDARD_POLICY_SOURCE.len() as u64,
                source_cache_section: VN_POLICY_BUNDLE_SOURCE_CACHE_SECTION.to_string(),
            }],
        }
    }

    pub fn validate_standard(&self) -> VnPolicyBundleValidationReport {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.policy_bundle.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_BUNDLE_SCHEMA",
                "VN policy bundle manifest schema is invalid",
            ));
        }

        let Some(standard) = self
            .bundles
            .iter()
            .find(|bundle| bundle.id == STANDARD_POLICY_BUNDLE_ID)
        else {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_BUNDLE_MISSING",
                "standard AstraVN policy bundle is missing",
            ));
            return VnPolicyBundleValidationReport {
                schema: "astra.vn.policy_bundle_validation_report.v1".to_string(),
                passed: false,
                diagnostics,
            };
        };

        if !is_safe_relative_entry(&standard.entry) {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_ENTRY_PATH",
                "standard AstraVN policy bundle entry must be a safe relative path",
            ));
        }
        if standard.entry.trim().is_empty() {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_ENTRY_MISSING",
                "standard AstraVN policy bundle needs an entry script",
            ));
        }
        for capability in [
            "astra.vn.message_ui",
            "astra.vn.choice_ui",
            "astra.vn.system_story",
            "astra.vn.presentation_timeline",
        ] {
            if !standard
                .capabilities
                .iter()
                .any(|declared| declared == capability)
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CAPABILITY_MISSING",
                        "standard AstraVN policy bundle is missing a required capability",
                    )
                    .with_field("capability", capability),
                );
            }
        }
        if !standard.lock_hash.starts_with("sha256:") {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_LOCK_MISSING",
                "standard AstraVN policy bundle needs a sha256 lock hash",
            ));
        }
        if !standard.source_hash.starts_with("sha256:") {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_SOURCE_HASH",
                "standard AstraVN policy bundle needs a sha256 source hash",
            ));
        }
        if standard.source_hash != standard.lock_hash {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_LOCK_MISMATCH",
                "standard AstraVN policy bundle lock hash must match the source hash",
            ));
        }
        if standard.byte_size == 0 {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_SOURCE_SIZE",
                "standard AstraVN policy bundle source byte size must be recorded",
            ));
        }
        if standard.source_cache_section != VN_POLICY_BUNDLE_SOURCE_CACHE_SECTION {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_SOURCE_CACHE",
                "standard AstraVN policy bundle must point at the source cache section",
            ));
        }

        VnPolicyBundleValidationReport {
            schema: "astra.vn.policy_bundle_validation_report.v1".to_string(),
            passed: diagnostics.is_empty(),
            diagnostics,
        }
    }

    pub fn validate_standard_with_cache(
        &self,
        cache: &VnPolicyBundleSourceCache,
    ) -> VnPolicyBundleValidationReport {
        let mut report = self.validate_standard();
        report
            .diagnostics
            .extend(cache.validate_against_manifest(self).diagnostics);
        report.passed = report.diagnostics.is_empty();
        report
    }
}

impl VnPolicyBundleSourceCache {
    pub fn standard() -> Self {
        Self {
            schema: "astra.vn.policy_bundle_source_cache.v1".to_string(),
            bundles: vec![VnPolicyBundleSourceEntry {
                id: STANDARD_POLICY_BUNDLE_ID.to_string(),
                entry: STANDARD_POLICY_ENTRY.to_string(),
                source_hash: standard_policy_source_hash(),
                byte_size: STANDARD_POLICY_SOURCE.len() as u64,
                source: STANDARD_POLICY_SOURCE.to_string(),
            }],
        }
    }

    pub fn standard_source() -> &'static str {
        STANDARD_POLICY_SOURCE
    }

    pub fn validate_against_manifest(
        &self,
        manifest: &VnPolicyBundleManifest,
    ) -> VnPolicyBundleValidationReport {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.policy_bundle_source_cache.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_POLICY_CACHE_SCHEMA",
                "VN policy bundle source cache schema is invalid",
            ));
        }

        for bundle in &manifest.bundles {
            let Some(cached) = self.bundles.iter().find(|entry| entry.id == bundle.id) else {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CACHE_MISSING",
                        "VN policy bundle source cache is missing a bundle source",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
                continue;
            };

            if cached.entry != bundle.entry {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CACHE_ENTRY",
                        "VN policy bundle source cache entry does not match the manifest",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            let recomputed = Hash256::from_sha256(cached.source.as_bytes()).to_string();
            if cached.source_hash != recomputed || bundle.source_hash != recomputed {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CACHE_HASH",
                        "VN policy bundle source cache hash does not match the source bytes",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            let byte_size = cached.source.len() as u64;
            if cached.byte_size != byte_size || bundle.byte_size != byte_size {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CACHE_SIZE",
                        "VN policy bundle source cache byte size does not match the source bytes",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            if cached.source.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CACHE_EMPTY",
                        "VN policy bundle source cache contains an empty source",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            if !is_safe_relative_entry(&cached.entry) || contains_absolute_path_like(&cached.source)
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_POLICY_CACHE_PATH_LEAK",
                        "VN policy bundle source cache contains path-like local data",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
        }

        VnPolicyBundleValidationReport {
            schema: "astra.vn.policy_bundle_validation_report.v1".to_string(),
            passed: diagnostics.is_empty(),
            diagnostics,
        }
    }
}

fn standard_policy_source_hash() -> String {
    Hash256::from_sha256(STANDARD_POLICY_SOURCE.as_bytes()).to_string()
}

fn is_safe_relative_entry(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.as_bytes().windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && matches!(window[2], b'/' | b'\\')
        })
    {
        return false;
    }
    !value
        .split(['/', '\\'])
        .any(|component| component.is_empty() || component == "." || component == "..")
}

fn contains_absolute_path_like(value: &str) -> bool {
    value.contains("\\\\")
        || value.as_bytes().windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && matches!(window[2], b'/' | b'\\')
        })
}
