use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CompiledCommand, CompiledVnProject, ReadingMode, SystemStoryValidationStatus,
    SystemUiProfilePolicy, SystemUnlockKind,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnSystemUiProfileManifest {
    pub schema: String,
    pub save_schema: String,
    pub unlock_sources: Vec<SystemUnlockSourcePolicy>,
    pub localization: SystemLocalizationCoverage,
    pub profiles: BTreeMap<String, SystemUiProfilePolicy>,
    pub declared_system_action_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemUnlockSourcePolicy {
    pub kind: SystemUnlockKind,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemLocalizationCoverage {
    pub locales: Vec<String>,
    pub text_key_count: usize,
    pub font_fallback_covered: bool,
    pub layout_covered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemUiProfileValidationReport {
    pub schema: String,
    pub status: SystemStoryValidationStatus,
    pub diagnostics: Vec<Diagnostic>,
}

impl VnSystemUiProfileManifest {
    pub fn from_compiled(compiled: &CompiledVnProject, locales: Vec<String>) -> Self {
        let text_key_count = compiled
            .states
            .values()
            .flat_map(|state| &state.scenes)
            .flat_map(|scene| &scene.commands)
            .filter(|command| matches!(command, CompiledCommand::Dialogue { .. }))
            .count();
        Self {
            schema: "astra.vn.system_ui_profile_manifest.v3".to_string(),
            save_schema: "astra.vn.save_slot.v2".to_string(),
            unlock_sources: vec![
                SystemUnlockSourcePolicy {
                    kind: SystemUnlockKind::Gallery,
                    source: "route_flag".to_string(),
                },
                SystemUnlockSourcePolicy {
                    kind: SystemUnlockKind::Replay,
                    source: "scene_read".to_string(),
                },
            ],
            localization: SystemLocalizationCoverage {
                locales,
                text_key_count,
                font_fallback_covered: true,
                layout_covered: true,
            },
            profiles: compiled.system_ui_profiles.clone(),
            declared_system_action_ids: compiled
                .story
                .system_story_manifest
                .actions
                .keys()
                .cloned()
                .collect(),
        }
    }

    pub fn validate(&self) -> SystemUiProfileValidationReport {
        tracing::debug!(
            event = "vn.system_ui.validate.start",
            "AstraVN system UI profile validation started"
        );
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.system_ui_profile_manifest.v3" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_UI_PROFILE_SCHEMA",
                "system UI profile manifest schema is invalid",
            ));
        }
        if self.save_schema != "astra.vn.save_slot.v2" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_SAVE_SCHEMA",
                "system UI profile must bind the current save schema",
            ));
        }
        for kind in [SystemUnlockKind::Gallery, SystemUnlockKind::Replay] {
            if !self
                .unlock_sources
                .iter()
                .any(|source| source.kind == kind && !source.source.trim().is_empty())
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_UNLOCK_SOURCE_POLICY",
                        "system UI profile must declare gallery/replay unlock sources",
                    )
                    .with_field("kind", format!("{kind:?}")),
                );
            }
        }
        if self.localization.locales.is_empty()
            || !self.localization.font_fallback_covered
            || !self.localization.layout_covered
        {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_LOCALIZATION_COVERAGE",
                "system UI profile must declare localization coverage",
            ));
        }
        if self.profiles.is_empty() {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_UI_PROFILE_POLICY_MISSING",
                "system UI profile manifest must declare at least one profile policy",
            ));
        }
        for (profile_id, policy) in &self.profiles {
            if profile_id != &policy.profile_id
                || profile_id.is_empty()
                || policy.save_slot_ids.is_empty()
                || !policy.reading_modes.contains(&ReadingMode::Manual)
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_UI_PROFILE_POLICY_INVALID",
                        "system UI profile policy identity, slots, or reading modes are invalid",
                    )
                    .with_field("profile", profile_id),
                );
                continue;
            }
            let unique = policy.save_slot_ids.iter().collect::<BTreeSet<_>>();
            if unique.len() != policy.save_slot_ids.len()
                || policy
                    .quick_slot_id
                    .as_ref()
                    .is_some_and(|slot| !unique.contains(slot))
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_UI_PROFILE_SLOT_POLICY",
                        "save slots must be unique and quick slot must be declared",
                    )
                    .with_field("profile", profile_id),
                );
            }
            if !policy
                .custom_action_ids
                .is_subset(&self.declared_system_action_ids)
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_ACTION_POLICY",
                        "profile custom actions must be compiled into the system story manifest",
                    )
                    .with_field("profile", profile_id),
                );
            }
        }
        SystemUiProfileValidationReport {
            schema: "astra.vn.system_ui_profile_validation_report.v2".to_string(),
            status: if diagnostics.is_empty() {
                SystemStoryValidationStatus::Pass
            } else {
                SystemStoryValidationStatus::Blocked
            },
            diagnostics,
        }
    }
}
