use std::collections::BTreeSet;

use astra_core::{Diagnostic, Hash128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CompiledCommand, CompiledStory, PresentationCommand, StageCommand, TimelineCommand, VnAudioBus,
    VnAudioSync,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnAdvancedPresentationManifest {
    pub schema: String,
    pub profile: String,
    pub story_hash: Hash128,
    pub evidence: BTreeSet<String>,
    pub timeline_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnAdvancedPresentationValidationReport {
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub required_count: usize,
    pub evidence_count: usize,
}

impl VnAdvancedPresentationManifest {
    pub fn profile_requires_advanced(profile: &str) -> bool {
        matches!(profile, "advanced-vn" | "vn.advanced_presentation")
            || profile.contains("advanced")
    }

    pub fn from_compiled(compiled: &CompiledStory, profile: impl Into<String>) -> Self {
        let mut evidence = BTreeSet::new();
        let mut layer_ids = BTreeSet::new();
        let mut timeline_ids = BTreeSet::new();
        let mut has_timeline_join = false;
        let mut has_timeline_cancel = false;

        for command in compiled
            .states
            .values()
            .flat_map(|state| &state.scenes)
            .flat_map(|scene| &scene.commands)
        {
            let CompiledCommand::Presentation {
                command: PresentationCommand::Stage(stage),
                ..
            } = command
            else {
                continue;
            };

            match stage {
                StageCommand::DeclareLayer { id, .. } => {
                    layer_ids.insert(id.clone());
                }
                StageCommand::Camera { .. } => {
                    evidence.insert("camera.task".to_string());
                }
                StageCommand::Movie { fallback, .. } => {
                    evidence.insert("video.layer".to_string());
                    if fallback.is_some() {
                        evidence.insert("presentation.fallback".to_string());
                    }
                }
                StageCommand::Audio(cue) => {
                    if cue.bus == VnAudioBus::Voice && cue.sync != VnAudioSync::None {
                        evidence.insert("voice.sync".to_string());
                    }
                }
                StageCommand::Timeline(TimelineCommand::Start(spec)) => {
                    timeline_ids.insert(spec.id.clone());
                    if spec.join == crate::VnTimelineJoinPolicy::Block {
                        has_timeline_join = true;
                    }
                    if spec.fallback.is_some() {
                        evidence.insert("presentation.fallback".to_string());
                    }
                    if spec.budget_us > 0 {
                        evidence.insert("renderer.effect_budget".to_string());
                    }
                }
                StageCommand::Timeline(TimelineCommand::Cancel { id, .. }) => {
                    timeline_ids.insert(id.clone());
                    has_timeline_cancel = true;
                }
                StageCommand::Effect {
                    fallback,
                    filter,
                    budget_us,
                    ..
                } => {
                    if !fallback.is_empty() {
                        evidence.insert("presentation.fallback".to_string());
                    }
                    if !filter.is_empty() && *budget_us > 0 {
                        evidence.insert("renderer.effect_budget".to_string());
                    }
                }
                _ => {}
            }
        }

        if layer_ids.len() >= 4 {
            evidence.insert("stage.multi_layer".to_string());
        }
        if has_timeline_join && has_timeline_cancel {
            evidence.insert("timeline.join_cancel".to_string());
        }

        Self {
            schema: "astra.vn.advanced_presentation_manifest.v1".to_string(),
            profile: profile.into(),
            story_hash: compiled.story_hash,
            evidence,
            timeline_ids: timeline_ids.into_iter().collect(),
        }
    }

    pub fn required_evidence() -> BTreeSet<String> {
        [
            "stage.multi_layer",
            "camera.task",
            "video.layer",
            "timeline.join_cancel",
            "presentation.fallback",
            "voice.sync",
            "renderer.effect_budget",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    pub fn validate_required(&self) -> VnAdvancedPresentationValidationReport {
        let required = Self::required_evidence();
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.advanced_presentation_manifest.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_ADVANCED_PRESENTATION_SCHEMA",
                "advanced presentation manifest schema is invalid",
            ));
        }
        for id in &required {
            if !self.evidence.contains(id) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_ADVANCED_PRESENTATION_EVIDENCE",
                        "advanced presentation evidence is missing",
                    )
                    .with_field("evidence", id),
                );
            }
        }

        VnAdvancedPresentationValidationReport {
            passed: diagnostics.is_empty(),
            diagnostics,
            required_count: required.len(),
            evidence_count: self.evidence.len(),
        }
    }

    pub fn has_evidence(&self, id: &str) -> bool {
        self.evidence.contains(id)
    }
}
