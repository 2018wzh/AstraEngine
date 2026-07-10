use std::collections::BTreeSet;

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyBundleManifest {
    pub schema: String,
    pub bundles: Vec<PolicyBundleEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyBundleEntry {
    pub id: String,
    pub entry: String,
    pub capabilities: Vec<String>,
    pub dependencies: Vec<String>,
    pub lock_hash: String,
    pub source_hash: String,
    pub byte_size: u64,
    pub source_cache_section: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyBundleSourceCache {
    pub schema: String,
    pub bundles: Vec<PolicyBundleSourceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyBundleSourceEntry {
    pub id: String,
    pub entry: String,
    pub source_hash: String,
    pub byte_size: u64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyBundleValidationReport {
    pub schema: String,
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl PolicyBundleSourceCache {
    pub fn validate(&self, manifest: &PolicyBundleManifest) -> PolicyBundleValidationReport {
        let mut diagnostics = Vec::new();
        if manifest.schema != "astra.policy_bundle.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_POLICY_BUNDLE_SCHEMA",
                "policy bundle manifest schema is unsupported",
            ));
        }
        if self.schema != "astra.policy_bundle_source_cache.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_POLICY_CACHE_SCHEMA",
                "policy bundle source cache schema is unsupported",
            ));
        }
        let mut ids = BTreeSet::new();
        for bundle in &manifest.bundles {
            if !ids.insert(bundle.id.as_str()) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_POLICY_BUNDLE_DUPLICATE",
                        "policy bundle id is declared more than once",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            if !is_safe_relative_entry(&bundle.entry) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_POLICY_ENTRY_PATH",
                        "policy entry must be a safe relative path",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            let Some(cached) = self.bundles.iter().find(|entry| entry.id == bundle.id) else {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_POLICY_CACHE_MISSING",
                        "policy source cache is missing a declared bundle",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
                continue;
            };
            if cached.entry != bundle.entry || !is_safe_relative_entry(&cached.entry) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_POLICY_ENTRY_PATH",
                        "policy source cache entry is unsafe or differs from its manifest",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            let hash = Hash256::from_sha256(cached.source.as_bytes()).to_string();
            if cached.source_hash != hash || bundle.source_hash != hash || bundle.lock_hash != hash
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_POLICY_CACHE_HASH",
                        "policy source bytes do not match manifest and lock hashes",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
            let byte_size = cached.source.len() as u64;
            if byte_size == 0 || cached.byte_size != byte_size || bundle.byte_size != byte_size {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_POLICY_CACHE_SIZE",
                        "policy source byte size is empty or inconsistent",
                    )
                    .with_field("bundle_id", &bundle.id),
                );
            }
        }
        PolicyBundleValidationReport {
            schema: "astra.policy_bundle_validation_report.v1".to_string(),
            passed: diagnostics.is_empty(),
            diagnostics,
        }
    }
}

pub fn is_safe_relative_entry(value: &str) -> bool {
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
