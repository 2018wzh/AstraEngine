use std::collections::BTreeSet;

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{VnPresentationEasing, VnWaitKind};

mod manifest;
pub const VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA: &str =
    "astra.vn.presentation_provider_manifest.v2";
/// Bounded profile reserved for Engine tests. Shipping manifests must not select it.
pub const VN_ENGINE_TEST_PROFILE_ID: &str = "minimal";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationProviderManifest {
    pub schema: String,
    pub provider_id: String,
    pub renderer_provider: String,
    pub filter_provider: String,
    pub shader_profile: String,
    pub supported_filters: Vec<String>,
    pub fallback_policies: Vec<VnEffectFallbackPolicy>,
    pub wait_capabilities: Vec<VnWaitKind>,
    pub profiles: Vec<VnPresentationProfile>,
    pub presets: Vec<VnPresentationPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationProfile {
    pub id: String,
    pub max_layers: u32,
    pub max_timelines: u32,
    pub max_effect_budget_us: u32,
    pub allowed_presets: Vec<String>,
    pub allowed_filters: Vec<String>,
    pub fallback_policy_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationPreset {
    pub id: String,
    pub command_kinds: Vec<String>,
    pub duration_ms: u32,
    pub easing: VnPresentationEasing,
    pub filter: Option<String>,
    pub fallback_policy_id: Option<String>,
    pub budget_us: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnEffectFallbackPolicy {
    pub id: String,
    pub mode: VnFallbackMode,
    pub blocks_release: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnFallbackMode {
    UseFlatFade,
    UseFallbackFrame,
    MuteWithDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationProviderValidationReport {
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub filter_count: usize,
    pub fallback_count: usize,
    pub wait_capability_count: usize,
    pub profile_count: usize,
    pub preset_count: usize,
}

fn collect_unique<'a>(
    values: impl Iterator<Item = &'a str>,
    code: &str,
    kind: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<&'a str> {
    let mut unique = BTreeSet::new();
    for value in values {
        if !is_safe_id(value) || !unique.insert(value) {
            diagnostics.push(
                Diagnostic::blocking(code, format!("{kind} id is invalid or duplicated"))
                    .with_field("id", value),
            );
        }
    }
    unique
}

fn validate_unique_ids(
    values: &[String],
    code: &str,
    kind: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let _ = collect_unique(values.iter().map(String::as_str), code, kind, diagnostics);
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
