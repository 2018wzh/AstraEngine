use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CompiledCommand, CompiledStory, SystemPageKind, SystemStoryValidationStatus, SystemUnlockKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemUiSurface {
    Title,
    Message,
    Choice,
    Save,
    Load,
    Config,
    Backlog,
    Gallery,
    Replay,
    VoiceReplay,
    RouteChart,
    LocalizationPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemUiRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SystemUiRect {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x < (self.x + self.width) as f64
            && y < (self.y + self.height) as f64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SystemUiAction {
    Advance,
    ChooseIndex { index: usize },
    Open { surface: SystemUiSurface },
    Activate { control_id: String },
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemUiControl {
    pub id: String,
    pub bounds: SystemUiRect,
    pub input_priority: i32,
    pub action: SystemUiAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemUiModel {
    pub schema: String,
    pub surface: SystemUiSurface,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub controls: Vec<SystemUiControl>,
}

impl SystemUiModel {
    pub fn message(viewport_width: u32, viewport_height: u32) -> Self {
        Self::new(
            SystemUiSurface::Message,
            viewport_width,
            viewport_height,
            vec![SystemUiControl {
                id: "message.advance".into(),
                bounds: SystemUiRect {
                    x: 0,
                    y: 0,
                    width: viewport_width,
                    height: viewport_height,
                },
                input_priority: 0,
                action: SystemUiAction::Advance,
            }],
        )
    }

    pub fn choice(viewport_width: u32, viewport_height: u32, option_count: usize) -> Self {
        let row_height = 56_u32;
        let width = viewport_width.saturating_sub(80).min(960);
        let x = viewport_width.saturating_sub(width) / 2;
        let total_height = row_height.saturating_mul(option_count as u32);
        let y = viewport_height.saturating_sub(total_height) / 2;
        let controls = (0..option_count)
            .map(|index| SystemUiControl {
                id: format!("choice.{index}"),
                bounds: SystemUiRect {
                    x,
                    y: y + row_height * index as u32,
                    width,
                    height: row_height,
                },
                input_priority: 100,
                action: SystemUiAction::ChooseIndex { index },
            })
            .collect();
        Self::new(
            SystemUiSurface::Choice,
            viewport_width,
            viewport_height,
            controls,
        )
    }

    pub fn system(page: SystemPageKind, viewport_width: u32, viewport_height: u32) -> Option<Self> {
        let surface = match page {
            SystemPageKind::Title => SystemUiSurface::Title,
            SystemPageKind::Save => SystemUiSurface::Save,
            SystemPageKind::Load => SystemUiSurface::Load,
            SystemPageKind::Config => SystemUiSurface::Config,
            SystemPageKind::Gallery => SystemUiSurface::Gallery,
            SystemPageKind::Replay => SystemUiSurface::Replay,
            SystemPageKind::VoiceReplay => SystemUiSurface::VoiceReplay,
            SystemPageKind::RouteChart => SystemUiSurface::RouteChart,
            SystemPageKind::Backlog => SystemUiSurface::Backlog,
            SystemPageKind::LocalizationPreview => SystemUiSurface::LocalizationPreview,
            SystemPageKind::Unknown => return None,
        };
        let controls = vec![SystemUiControl {
            id: format!("{}.back", format!("{surface:?}").to_lowercase()),
            bounds: SystemUiRect {
                x: 24,
                y: 24,
                width: 160,
                height: 52,
            },
            input_priority: 1000,
            action: SystemUiAction::Back,
        }];
        Some(Self::new(
            surface,
            viewport_width,
            viewport_height,
            controls,
        ))
    }

    pub fn hit_test(&self, x: f64, y: f64) -> Option<&SystemUiAction> {
        self.controls
            .iter()
            .filter(|control| control.bounds.contains(x, y))
            .max_by_key(|control| control.input_priority)
            .map(|control| &control.action)
    }

    fn new(
        surface: SystemUiSurface,
        viewport_width: u32,
        viewport_height: u32,
        controls: Vec<SystemUiControl>,
    ) -> Self {
        Self {
            schema: "astra.vn.system_ui_model.v1".into(),
            surface,
            viewport_width,
            viewport_height,
            controls,
        }
    }
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
        tracing::debug!(
            event = "vn.system_ui.validate.start",
            "AstraVN system UI profile validation started"
        );
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
