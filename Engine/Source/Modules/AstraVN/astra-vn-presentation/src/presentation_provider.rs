use std::collections::{BTreeMap, BTreeSet};

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::VnWaitKind;

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

impl VnPresentationProviderManifest {
    pub fn standard() -> Self {
        let preset_ids = vec![
            "flash_soft".to_string(),
            "hero_enter".to_string(),
            "slow_push".to_string(),
            "soft_fade".to_string(),
        ];
        Self {
            schema: VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA.to_string(),
            provider_id: "astra.vn.standard_presentation".to_string(),
            renderer_provider: "astra.renderer2d.wgpu".to_string(),
            filter_provider: "astra.media.filter_graph".to_string(),
            shader_profile: "astra.shader_profile.vn_stage2d.v1".to_string(),
            supported_filters: vec![
                "astra.filter.bloom".to_string(),
                "astra.filter.color_matrix".to_string(),
                "astra.filter.crt_soft".to_string(),
                "astra.filter.fade".to_string(),
            ],
            fallback_policies: vec![
                VnEffectFallbackPolicy {
                    id: "filter_missing".to_string(),
                    mode: VnFallbackMode::UseFlatFade,
                    blocks_release: false,
                },
                VnEffectFallbackPolicy {
                    id: "movie_decode_missing".to_string(),
                    mode: VnFallbackMode::UseFallbackFrame,
                    blocks_release: true,
                },
                VnEffectFallbackPolicy {
                    id: "voice_decode_missing".to_string(),
                    mode: VnFallbackMode::MuteWithDiagnostic,
                    blocks_release: true,
                },
            ],
            wait_capabilities: vec![
                VnWaitKind::Fence,
                VnWaitKind::TimelineComplete,
                VnWaitKind::MovieEnd,
                VnWaitKind::VoiceEnd,
            ],
            profiles: [
                (VN_ENGINE_TEST_PROFILE_ID, 4, 2, 4_000),
                ("classic", 16, 8, 4_000),
                ("modern", 32, 16, 8_000),
                ("advanced-vn", 64, 32, 16_000),
            ]
            .into_iter()
            .map(
                |(id, max_layers, max_timelines, max_effect_budget_us)| VnPresentationProfile {
                    id: id.to_string(),
                    max_layers,
                    max_timelines,
                    max_effect_budget_us,
                    allowed_presets: preset_ids.clone(),
                    allowed_filters: vec![
                        "astra.filter.bloom".to_string(),
                        "astra.filter.color_matrix".to_string(),
                        "astra.filter.crt_soft".to_string(),
                        "astra.filter.fade".to_string(),
                    ],
                    fallback_policy_ids: vec![
                        "filter_missing".to_string(),
                        "movie_decode_missing".to_string(),
                        "voice_decode_missing".to_string(),
                    ],
                },
            )
            .collect(),
            presets: vec![
                VnPresentationPreset {
                    id: "soft_fade".to_string(),
                    command_kinds: vec!["background".to_string(), "hide".to_string()],
                    duration_ms: 300,
                    easing: VnPresentationEasing::EaseInOut,
                    filter: Some("astra.filter.fade".to_string()),
                    fallback_policy_id: Some("filter_missing".to_string()),
                    budget_us: 4_000,
                },
                VnPresentationPreset {
                    id: "hero_enter".to_string(),
                    command_kinds: vec!["show".to_string()],
                    duration_ms: 300,
                    easing: VnPresentationEasing::EaseOut,
                    filter: None,
                    fallback_policy_id: None,
                    budget_us: 2_000,
                },
                VnPresentationPreset {
                    id: "slow_push".to_string(),
                    command_kinds: vec!["camera".to_string(), "move".to_string()],
                    duration_ms: 480,
                    easing: VnPresentationEasing::EaseInOut,
                    filter: None,
                    fallback_policy_id: None,
                    budget_us: 2_000,
                },
                VnPresentationPreset {
                    id: "flash_soft".to_string(),
                    command_kinds: vec!["transition".to_string()],
                    duration_ms: 220,
                    easing: VnPresentationEasing::EaseOut,
                    filter: Some("astra.filter.bloom".to_string()),
                    fallback_policy_id: Some("filter_missing".to_string()),
                    budget_us: 4_000,
                },
            ],
        }
    }

    pub fn validate_standard(&self) -> VnPresentationProviderValidationReport {
        let mut diagnostics = Vec::new();
        if self.schema != VN_PRESENTATION_PROVIDER_MANIFEST_SCHEMA {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_PRESENTATION_PROVIDER_SCHEMA",
                "presentation provider manifest schema is invalid or requires re-cook",
            ));
        }
        if self.provider_id != "astra.vn.standard_presentation" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_PRESENTATION_PROVIDER_ID",
                "presentation provider manifest must use astra.vn.standard_presentation",
            ));
        }
        for (field, value) in [
            ("renderer_provider", &self.renderer_provider),
            ("filter_provider", &self.filter_provider),
            ("shader_profile", &self.shader_profile),
        ] {
            if value.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PROVIDER_FIELD",
                        "presentation provider manifest has an empty provider field",
                    )
                    .with_field("field", field),
                );
            }
        }
        validate_unique_ids(
            &self.supported_filters,
            "ASTRA_VN_PRESENTATION_FILTER_ID",
            "supported filter",
            &mut diagnostics,
        );
        for required in [
            VnWaitKind::Fence,
            VnWaitKind::TimelineComplete,
            VnWaitKind::MovieEnd,
            VnWaitKind::VoiceEnd,
        ] {
            if !self.wait_capabilities.contains(&required) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_WAIT_CAPABILITY",
                        "presentation provider is missing a required wait capability",
                    )
                    .with_field("wait_kind", format!("{required:?}")),
                );
            }
        }

        let fallback_ids = collect_unique(
            self.fallback_policies
                .iter()
                .map(|policy| policy.id.as_str()),
            "ASTRA_VN_PRESENTATION_FALLBACK_DUPLICATE",
            "fallback policy",
            &mut diagnostics,
        );
        for required in [
            "filter_missing",
            "movie_decode_missing",
            "voice_decode_missing",
        ] {
            if !fallback_ids.contains(required) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_FALLBACK_POLICY",
                        "presentation provider is missing a required fallback policy",
                    )
                    .with_field("policy", required),
                );
            }
        }

        let presets = self.validate_presets(&fallback_ids, &mut diagnostics);
        let profiles = self.validate_profiles(&fallback_ids, &presets, &mut diagnostics);
        for required in [
            VN_ENGINE_TEST_PROFILE_ID,
            "classic",
            "modern",
            "advanced-vn",
        ] {
            if !profiles.contains_key(required) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PROFILE_REQUIRED",
                        "presentation provider is missing a required product profile",
                    )
                    .with_field("profile", required),
                );
            }
        }

        VnPresentationProviderValidationReport {
            passed: diagnostics.is_empty(),
            diagnostics,
            filter_count: self.supported_filters.len(),
            fallback_count: self.fallback_policies.len(),
            wait_capability_count: self.wait_capabilities.len(),
            profile_count: self.profiles.len(),
            preset_count: self.presets.len(),
        }
    }

    pub fn profile(&self, profile: &str) -> Result<&VnPresentationProfile, Diagnostic> {
        let report = self.validate_standard();
        if let Some(diagnostic) = report.diagnostics.into_iter().next() {
            return Err(diagnostic);
        }
        self.profiles
            .iter()
            .find(|candidate| candidate.id == profile)
            .ok_or_else(|| {
                Diagnostic::blocking(
                    "ASTRA_VN_PRESENTATION_PROFILE_UNDECLARED",
                    "requested presentation profile is not declared by the package",
                )
                .with_field("profile", profile)
            })
    }

    pub fn resolve_preset(
        &self,
        profile: &str,
        command_kind: &str,
        preset_id: &str,
    ) -> Result<&VnPresentationPreset, Diagnostic> {
        let profile = self.profile(profile)?;
        let preset = self
            .presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .ok_or_else(|| {
                Diagnostic::blocking(
                    "ASTRA_VN_PRESENTATION_PRESET_UNDECLARED",
                    "presentation preset is not declared by the package",
                )
                .with_field("preset", preset_id)
            })?;
        if !profile
            .allowed_presets
            .iter()
            .any(|allowed| allowed == preset_id)
        {
            return Err(Diagnostic::blocking(
                "ASTRA_VN_PRESENTATION_PRESET_PROFILE",
                "presentation preset is not allowed by the selected profile",
            )
            .with_field("profile", &profile.id)
            .with_field("preset", preset_id));
        }
        if !preset
            .command_kinds
            .iter()
            .any(|allowed| allowed == command_kind)
        {
            return Err(Diagnostic::blocking(
                "ASTRA_VN_PRESENTATION_PRESET_COMMAND",
                "presentation preset cannot execute this command kind",
            )
            .with_field("preset", preset_id)
            .with_field("command", command_kind));
        }
        Ok(preset)
    }

    fn validate_presets<'a>(
        &'a self,
        fallback_ids: &BTreeSet<&str>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> BTreeMap<&'a str, &'a VnPresentationPreset> {
        let mut presets = BTreeMap::new();
        for preset in &self.presets {
            if !is_safe_id(&preset.id) || presets.insert(preset.id.as_str(), preset).is_some() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PRESET_ID",
                        "presentation preset id is invalid or duplicated",
                    )
                    .with_field("preset", &preset.id),
                );
            }
            if preset.command_kinds.is_empty() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PRESET_COMMAND",
                        "presentation preset must declare at least one command kind",
                    )
                    .with_field("preset", &preset.id),
                );
            }
            validate_unique_ids(
                &preset.command_kinds,
                "ASTRA_VN_PRESENTATION_PRESET_COMMAND",
                "presentation preset command kind",
                diagnostics,
            );
            if preset.duration_ms == 0 || preset.budget_us == 0 {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PRESET_BUDGET",
                        "presentation preset duration and budget must be non-zero",
                    )
                    .with_field("preset", &preset.id),
                );
            }
            if let Some(filter) = &preset.filter {
                if !self.supported_filters.iter().any(|item| item == filter) {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_PRESENTATION_PRESET_FILTER",
                            "presentation preset references an unsupported filter",
                        )
                        .with_field("preset", &preset.id)
                        .with_field("filter", filter),
                    );
                }
            }
            if let Some(fallback) = &preset.fallback_policy_id {
                if !fallback_ids.contains(fallback.as_str()) {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_PRESENTATION_PRESET_FALLBACK",
                            "presentation preset references an unknown fallback policy",
                        )
                        .with_field("preset", &preset.id)
                        .with_field("fallback", fallback),
                    );
                }
            }
        }
        presets
    }

    fn validate_profiles<'a>(
        &'a self,
        fallback_ids: &BTreeSet<&str>,
        presets: &BTreeMap<&str, &VnPresentationPreset>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> BTreeMap<&'a str, &'a VnPresentationProfile> {
        let mut profiles = BTreeMap::new();
        for profile in &self.profiles {
            if !is_safe_id(&profile.id) || profiles.insert(profile.id.as_str(), profile).is_some() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PROFILE_ID",
                        "presentation profile id is invalid or duplicated",
                    )
                    .with_field("profile", &profile.id),
                );
            }
            if profile.max_layers == 0
                || profile.max_timelines == 0
                || profile.max_effect_budget_us == 0
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_PROFILE_BUDGET",
                        "presentation profile budgets must be non-zero",
                    )
                    .with_field("profile", &profile.id),
                );
            }
            for (values, code, kind) in [
                (
                    &profile.allowed_presets,
                    "ASTRA_VN_PRESENTATION_PROFILE_PRESET",
                    "presentation profile preset",
                ),
                (
                    &profile.allowed_filters,
                    "ASTRA_VN_PRESENTATION_PROFILE_FILTER",
                    "presentation profile filter",
                ),
                (
                    &profile.fallback_policy_ids,
                    "ASTRA_VN_PRESENTATION_PROFILE_FALLBACK",
                    "presentation profile fallback",
                ),
            ] {
                if values.is_empty() {
                    diagnostics.push(
                        Diagnostic::blocking(code, format!("{kind} list must not be empty"))
                            .with_field("profile", &profile.id),
                    );
                }
                validate_unique_ids(values, code, kind, diagnostics);
            }
            for preset in &profile.allowed_presets {
                match presets.get(preset.as_str()) {
                    Some(preset) if preset.budget_us <= profile.max_effect_budget_us => {}
                    Some(_) => diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_PRESENTATION_PROFILE_PRESET_BUDGET",
                            "presentation preset exceeds the selected profile budget",
                        )
                        .with_field("profile", &profile.id)
                        .with_field("preset", preset),
                    ),
                    None => diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_PRESENTATION_PROFILE_PRESET",
                            "presentation profile references an unknown preset",
                        )
                        .with_field("profile", &profile.id)
                        .with_field("preset", preset),
                    ),
                }
            }
            for filter in &profile.allowed_filters {
                if !self.supported_filters.iter().any(|item| item == filter) {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_PRESENTATION_PROFILE_FILTER",
                            "presentation profile references an unsupported filter",
                        )
                        .with_field("profile", &profile.id)
                        .with_field("filter", filter),
                    );
                }
            }
            for fallback in &profile.fallback_policy_ids {
                if !fallback_ids.contains(fallback.as_str()) {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_PRESENTATION_PROFILE_FALLBACK",
                            "presentation profile references an unknown fallback policy",
                        )
                        .with_field("profile", &profile.id)
                        .with_field("fallback", fallback),
                    );
                }
            }
        }
        profiles
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnPresentationEasing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
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
