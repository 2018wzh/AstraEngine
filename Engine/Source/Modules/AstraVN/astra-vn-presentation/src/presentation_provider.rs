use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::VnWaitKind;

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
}

impl VnPresentationProviderManifest {
    pub fn standard() -> Self {
        Self {
            schema: "astra.vn.presentation_provider_manifest.v1".to_string(),
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
        }
    }

    pub fn validate_standard(&self) -> VnPresentationProviderValidationReport {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.presentation_provider_manifest.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_PRESENTATION_PROVIDER_SCHEMA",
                "presentation provider manifest schema is invalid",
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
        for required in [
            "filter_missing",
            "movie_decode_missing",
            "voice_decode_missing",
        ] {
            if !self
                .fallback_policies
                .iter()
                .any(|policy| policy.id == required)
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_PRESENTATION_FALLBACK_POLICY",
                        "presentation provider is missing a required fallback policy",
                    )
                    .with_field("policy", required),
                );
            }
        }

        VnPresentationProviderValidationReport {
            passed: diagnostics.is_empty(),
            diagnostics,
            filter_count: self.supported_filters.len(),
            fallback_count: self.fallback_policies.len(),
            wait_capability_count: self.wait_capabilities.len(),
        }
    }
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
}
