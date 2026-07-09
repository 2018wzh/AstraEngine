use std::collections::BTreeSet;

use astra_core::{Diagnostic, Hash128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CompiledCommand, CompiledStory, PresentationCommand, SystemPageKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnCommercialBaselineManifest {
    pub schema: String,
    pub story_hash: Hash128,
    pub required_features: BTreeSet<String>,
    pub features_present: BTreeSet<String>,
}

impl VnCommercialBaselineManifest {
    pub fn from_compiled(compiled: &CompiledStory) -> Self {
        let mut features_present = BTreeSet::new();
        for command in compiled
            .states
            .values()
            .flat_map(|state| &state.scenes)
            .flat_map(|scene| &scene.commands)
        {
            match command {
                CompiledCommand::Dialogue { voice, .. } => {
                    features_present.insert("dialogue".to_string());
                    if voice.is_some() {
                        features_present.insert("voice_replay".to_string());
                    }
                }
                CompiledCommand::Choice { options, .. } => {
                    features_present.insert("choice".to_string());
                    if !options.is_empty() {
                        features_present.insert("route".to_string());
                    }
                }
                CompiledCommand::Jump { .. } | CompiledCommand::Call { .. } => {
                    features_present.insert("route".to_string());
                }
                CompiledCommand::SystemPage { page, .. } if *page != SystemPageKind::Unknown => {
                    features_present.insert("system_ui".to_string());
                }
                CompiledCommand::Wait { .. } => {
                    features_present.insert("explicit_wait".to_string());
                }
                CompiledCommand::Presentation {
                    command:
                        PresentationCommand::Stage {
                            command,
                            attributes,
                        },
                    ..
                } => match command.as_str() {
                    "movie" => {
                        if attributes
                            .get("end")
                            .is_some_and(|value| value.eq_ignore_ascii_case("wait"))
                            && attributes.contains_key("fallback")
                        {
                            features_present.insert("movie_wait".to_string());
                            features_present.insert("explicit_wait".to_string());
                        }
                    }
                    "voice" => {
                        features_present.insert("voice_command".to_string());
                        if attributes
                            .get("sync")
                            .is_some_and(|value| matches!(value.as_str(), "text" | "fence"))
                        {
                            features_present.insert("explicit_wait".to_string());
                        }
                    }
                    "bgm" => {
                        features_present.insert("bgm".to_string());
                    }
                    "se" => {
                        features_present.insert("se".to_string());
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        Self {
            schema: "astra.vn.commercial_baseline_manifest.v1".to_string(),
            story_hash: compiled.story_hash,
            required_features: required_features(),
            features_present,
        }
    }

    pub fn validate_required(&self) -> VnCommercialBaselineValidationReport {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.commercial_baseline_manifest.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_COMMERCIAL_BASELINE_SCHEMA",
                "commercial baseline manifest schema is invalid",
            ));
        }
        for feature in &self.required_features {
            if !self.features_present.contains(feature) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_COMMERCIAL_BASELINE_FEATURE",
                        "commercial baseline feature is missing",
                    )
                    .with_field("feature", feature),
                );
            }
        }

        VnCommercialBaselineValidationReport {
            passed: diagnostics.is_empty(),
            diagnostics,
            required_count: self.required_features.len(),
            feature_count: self.features_present.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnCommercialBaselineValidationReport {
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub required_count: usize,
    pub feature_count: usize,
}

fn required_features() -> BTreeSet<String> {
    [
        "dialogue",
        "choice",
        "route",
        "voice_replay",
        "movie_wait",
        "bgm",
        "se",
        "explicit_wait",
        "system_ui",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
