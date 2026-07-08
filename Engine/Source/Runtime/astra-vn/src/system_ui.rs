use std::collections::BTreeMap;

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CompiledCommand, CompiledStory, SystemPageKind, SystemUnlockKind, VnError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryManifest {
    pub schema: String,
    pub entries: BTreeMap<SystemPageKind, SystemStoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryEntry {
    pub page: SystemPageKind,
    pub story_id: String,
    pub state_id: String,
    pub source_id: String,
    pub policy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryValidationReport {
    pub schema: String,
    pub status: SystemStoryValidationStatus,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnSystemUiProfileManifest {
    pub schema: String,
    pub save_migration: SystemSaveMigrationPolicy,
    pub unlock_sources: Vec<SystemUnlockSourcePolicy>,
    pub localization: SystemLocalizationCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemSaveMigrationPolicy {
    pub minimum_supported_schema: String,
    pub current_schema: String,
    pub migrator_id: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemStoryValidationStatus {
    Pass,
    Blocked,
}

impl SystemStoryManifest {
    pub fn empty() -> Self {
        Self {
            schema: "astra.vn.system_story_manifest.v1".to_string(),
            entries: BTreeMap::new(),
        }
    }

    pub fn from_compiled(compiled: &CompiledStory) -> Result<Self, VnError> {
        let mut entries = BTreeMap::new();
        for state in compiled.states.values() {
            for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
                let CompiledCommand::SystemPage { id, page, policy } = command else {
                    continue;
                };
                if *page == SystemPageKind::Unknown {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_SYSTEM_PAGE_UNKNOWN",
                        format!("system page {id} has unknown kind"),
                    ));
                }
                entries.entry(*page).or_insert_with(|| SystemStoryEntry {
                    page: *page,
                    story_id: state.story_id.clone(),
                    state_id: state.id.clone(),
                    source_id: id.clone(),
                    policy: policy.clone(),
                });
            }
        }
        Ok(Self {
            entries,
            ..Self::empty()
        })
    }

    pub fn commercial_required_pages() -> Vec<SystemPageKind> {
        vec![
            SystemPageKind::Title,
            SystemPageKind::Save,
            SystemPageKind::Load,
            SystemPageKind::Config,
            SystemPageKind::Gallery,
            SystemPageKind::Replay,
            SystemPageKind::VoiceReplay,
            SystemPageKind::RouteChart,
            SystemPageKind::Backlog,
            SystemPageKind::LocalizationPreview,
        ]
    }

    pub fn validate_required(&self, required: &[SystemPageKind]) -> SystemStoryValidationReport {
        let mut diagnostics = Vec::new();
        for page in required {
            let Some(entry) = self.entries.get(page) else {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_ENTRY_MISSING",
                        format!("required system page {page:?} is missing"),
                    )
                    .with_field("page", format!("{page:?}")),
                );
                continue;
            };
            if entry
                .policy
                .as_deref()
                .is_none_or(|policy| policy.trim().is_empty())
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_POLICY_MISSING",
                        format!("required system page {page:?} is missing policy binding"),
                    )
                    .with_field("page", format!("{page:?}"))
                    .with_field("state_id", &entry.state_id),
                );
            }
        }
        SystemStoryValidationReport {
            schema: "astra.vn.system_story_validation_report.v1".to_string(),
            status: if diagnostics.is_empty() {
                SystemStoryValidationStatus::Pass
            } else {
                SystemStoryValidationStatus::Blocked
            },
            diagnostics,
        }
    }
}

impl VnSystemUiProfileManifest {
    pub fn from_compiled(compiled: &CompiledStory, locales: Vec<String>) -> Self {
        let text_key_count = compiled
            .states
            .values()
            .flat_map(|state| &state.scenes)
            .flat_map(|scene| &scene.commands)
            .filter(|command| matches!(command, CompiledCommand::Dialogue { .. }))
            .count();
        Self {
            schema: "astra.vn.system_ui_profile_manifest.v1".to_string(),
            save_migration: SystemSaveMigrationPolicy {
                minimum_supported_schema: "astra.vn.save_slot.v1".to_string(),
                current_schema: "astra.vn.save_slot.v1".to_string(),
                migrator_id: "astra.vn.save_slot.identity_migrator.v1".to_string(),
            },
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
        }
    }

    pub fn validate(&self) -> SystemUiProfileValidationReport {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.system_ui_profile_manifest.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_UI_PROFILE_SCHEMA",
                "system UI profile manifest schema is invalid",
            ));
        }
        if self
            .save_migration
            .minimum_supported_schema
            .trim()
            .is_empty()
            || self.save_migration.current_schema.trim().is_empty()
            || self.save_migration.migrator_id.trim().is_empty()
        {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_MIGRATION",
                "system UI profile must declare save migration coverage",
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
            || self.localization.text_key_count == 0
            || !self.localization.font_fallback_covered
            || !self.localization.layout_covered
        {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_LOCALIZATION_COVERAGE",
                "system UI profile must declare localization coverage",
            ));
        }
        SystemUiProfileValidationReport {
            schema: "astra.vn.system_ui_profile_validation_report.v1".to_string(),
            status: if diagnostics.is_empty() {
                SystemStoryValidationStatus::Pass
            } else {
                SystemStoryValidationStatus::Blocked
            },
            diagnostics,
        }
    }
}
