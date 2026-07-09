use std::collections::BTreeSet;

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CompiledStory;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditorVisualMetadata {
    pub schema: String,
    pub graph_nodes: Vec<GraphNodeMetadata>,
    pub timeline_tracks: Vec<TimelineTrackMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphNodeMetadata {
    pub id: String,
    pub command_id: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineTrackMetadata {
    pub id: String,
    pub command_ids: Vec<String>,
    pub lane: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EditorMetadataValidationReport {
    pub schema: String,
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EditorMetadataPatchManifest {
    pub schema: String,
    pub command_ids: Vec<String>,
}

impl EditorVisualMetadata {
    pub fn validate_against(&self, compiled: &CompiledStory) -> EditorMetadataValidationReport {
        let mut diagnostics = Vec::new();
        for command_id in self.command_ids() {
            if !compiled.source_map.contains_key(&command_id) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_EDITOR_METADATA_SOURCE_MISSING",
                        format!("editor metadata references missing command {command_id}"),
                    )
                    .with_field("command_id", command_id),
                );
            }
        }
        EditorMetadataValidationReport {
            schema: "astra.vn.editor_metadata_validation_report.v1".to_string(),
            passed: diagnostics.is_empty(),
            diagnostics,
        }
    }

    pub fn to_patch_manifest(&self) -> EditorMetadataPatchManifest {
        EditorMetadataPatchManifest {
            schema: "astra.vn.editor_metadata_patch_manifest.v1".to_string(),
            command_ids: self.command_ids().into_iter().collect(),
        }
    }

    fn command_ids(&self) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        for node in &self.graph_nodes {
            ids.insert(node.command_id.clone());
        }
        for track in &self.timeline_tracks {
            ids.extend(track.command_ids.iter().cloned());
        }
        ids
    }
}
