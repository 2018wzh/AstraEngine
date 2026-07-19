use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_id, validate_string, UiRect, UiValidationError, ValidateUi, MAX_NODES_PER_VIEW,
    MAX_TREE_DEPTH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiSemanticRole {
    Application,
    Window,
    Dialog,
    Group,
    Text,
    Image,
    Button,
    Toggle,
    Slider,
    Select,
    List,
    ListItem,
    Grid,
    GridCell,
    TextInput,
    Link,
    Canvas,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum UiSemanticAction {
    Focus,
    Activate,
    Increment,
    Decrement,
    SetValue,
    ScrollIntoView,
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiSemanticNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: UiSemanticRole,
    pub bounds_points: UiRect,
    pub name: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub enabled: bool,
    pub hidden: bool,
    pub focused: bool,
    pub selected: bool,
    pub checked: Option<bool>,
    pub actions: BTreeSet<UiSemanticAction>,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiSemanticSnapshot {
    pub schema: String,
    pub session_id: String,
    pub generation: u64,
    pub root_id: String,
    pub nodes: Vec<UiSemanticNode>,
    pub hash: Hash256,
}

/// Redacted accessibility evidence. Human-readable commercial strings and
/// bounds deliberately remain in the live snapshot and never enter reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiAccessibilityReportNode {
    pub id: String,
    pub role: UiSemanticRole,
    pub enabled: bool,
    pub action_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiAccessibilityReport {
    pub schema: String,
    pub provider: String,
    pub generation: u64,
    pub semantic_snapshot_hash: Hash256,
    pub nodes: Vec<UiAccessibilityReportNode>,
    pub result: String,
    pub hash: Hash256,
}

impl UiAccessibilityReport {
    pub fn from_snapshot(
        provider: impl Into<String>,
        result: impl Into<String>,
        snapshot: &UiSemanticSnapshot,
    ) -> Result<Self, UiValidationError> {
        snapshot.validate()?;
        let nodes = snapshot
            .nodes
            .iter()
            .map(|node| {
                postcard::to_allocvec(&node.actions)
                    .map(|bytes| UiAccessibilityReportNode {
                        id: node.id.clone(),
                        role: node.role,
                        enabled: node.enabled,
                        action_hash: Hash256::from_sha256(&bytes),
                    })
                    .map_err(|error| {
                        UiValidationError::invalid(
                            "ASTRA_UI_ACCESSIBILITY_REPORT_ENCODE",
                            error.to_string(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut report = Self {
            schema: "astra.ui_accessibility_report.v1".into(),
            provider: provider.into(),
            generation: snapshot.generation,
            semantic_snapshot_hash: snapshot.hash,
            nodes,
            result: result.into(),
            hash: Hash256::from_sha256(&[]),
        };
        report.hash = report.compute_hash()?;
        report.validate()?;
        Ok(report)
    }

    fn compute_hash(&self) -> Result<Hash256, UiValidationError> {
        let mut value = self.clone();
        value.hash = Hash256::from_sha256(&[]);
        postcard::to_allocvec(&value)
            .map(|bytes| Hash256::from_sha256(&bytes))
            .map_err(|error| {
                UiValidationError::invalid(
                    "ASTRA_UI_ACCESSIBILITY_REPORT_ENCODE",
                    error.to_string(),
                )
            })
    }
}

impl ValidateUi for UiAccessibilityReport {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_accessibility_report.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_ACCESSIBILITY_REPORT_SCHEMA",
                "accessibility report schema is invalid",
            ));
        }
        validate_id("accessibility.provider", &self.provider)?;
        validate_id("accessibility.result", &self.result)?;
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES_PER_VIEW {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_ACCESSIBILITY_REPORT_NODE_LIMIT",
                "accessibility report node count is invalid",
            ));
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            validate_id("accessibility.node.id", &node.id)?;
            if !ids.insert(node.id.as_str()) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_ACCESSIBILITY_REPORT_DUPLICATE_ID",
                    "accessibility report contains duplicate node ids",
                ));
            }
        }
        if self.compute_hash()? != self.hash {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_ACCESSIBILITY_REPORT_HASH",
                "accessibility report hash mismatch",
            ));
        }
        crate::validate_serialized_size(self)
    }
}

impl UiSemanticSnapshot {
    pub fn compute_hash(&self) -> Result<Hash256, UiValidationError> {
        #[derive(Serialize)]
        struct Hashable<'a> {
            schema: &'a str,
            session_id: &'a str,
            generation: u64,
            root_id: &'a str,
            nodes: &'a [UiSemanticNode],
        }
        let bytes = postcard::to_allocvec(&Hashable {
            schema: &self.schema,
            session_id: &self.session_id,
            generation: self.generation,
            root_id: &self.root_id,
            nodes: &self.nodes,
        })
        .map_err(|error| {
            UiValidationError::invalid("ASTRA_UI_SEMANTIC_ENCODE", error.to_string())
        })?;
        Ok(Hash256::from_sha256(&bytes))
    }
}

impl ValidateUi for UiSemanticSnapshot {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_semantic_snapshot.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SEMANTIC_SCHEMA",
                "semantic schema must be astra.ui_semantic_snapshot.v1",
            ));
        }
        validate_id("semantic.session_id", &self.session_id)?;
        validate_id("semantic.root_id", &self.root_id)?;
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES_PER_VIEW {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SEMANTIC_NODE_LIMIT",
                format!("semantic tree must contain 1..={MAX_NODES_PER_VIEW} nodes"),
            ));
        }
        let mut ids = BTreeSet::new();
        let mut parents = BTreeMap::new();
        let mut focused = 0usize;
        for node in &self.nodes {
            validate_id("semantic.node.id", &node.id)?;
            if !ids.insert(node.id.as_str()) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_SEMANTIC_DUPLICATE_ID",
                    format!("duplicate semantic node {}", node.id),
                ));
            }
            if !node.bounds_points.is_finite_and_ordered() {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_SEMANTIC_BOUNDS",
                    format!(
                        "semantic node {} has invalid bounds min=({}, {}) max=({}, {})",
                        node.id,
                        node.bounds_points.min.x,
                        node.bounds_points.min.y,
                        node.bounds_points.max.x,
                        node.bounds_points.max.y
                    ),
                ));
            }
            for value in [&node.name, &node.description, &node.value]
                .into_iter()
                .flatten()
            {
                validate_string("semantic.text", value)?;
            }
            for (key, value) in &node.properties {
                validate_id("semantic.property", key)?;
                validate_string("semantic.property.value", value)?;
            }
            if node.focused {
                focused += 1;
            }
            parents.insert(node.id.as_str(), node.parent_id.as_deref());
        }
        if !ids.contains(self.root_id.as_str()) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SEMANTIC_ROOT_MISSING",
                "semantic root id does not exist",
            ));
        }
        if parents
            .get(self.root_id.as_str())
            .copied()
            .flatten()
            .is_some()
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SEMANTIC_ROOT_PARENT",
                "semantic root must not have a parent",
            ));
        }
        if focused > 1 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SEMANTIC_MULTIPLE_FOCUS",
                "a semantic snapshot may contain at most one focused node",
            ));
        }
        for (id, parent) in &parents {
            if let Some(parent) = parent {
                if !ids.contains(parent) {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_SEMANTIC_PARENT_MISSING",
                        format!("semantic node {id} references missing parent {parent}"),
                    ));
                }
            }
            let mut cursor = Some(*id);
            let mut visited = BTreeSet::new();
            let mut depth = 0usize;
            while let Some(current) = cursor {
                if !visited.insert(current) {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_SEMANTIC_CYCLE",
                        format!("semantic node {id} participates in a parent cycle"),
                    ));
                }
                depth += 1;
                if depth > MAX_TREE_DEPTH {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_SEMANTIC_DEPTH",
                        format!("semantic node {id} exceeds depth {MAX_TREE_DEPTH}"),
                    ));
                }
                cursor = parents.get(current).copied().flatten();
            }
        }
        let expected = self.compute_hash()?;
        if expected != self.hash {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SEMANTIC_HASH",
                "semantic snapshot hash mismatch",
            ));
        }
        crate::validate_serialized_size(self)
    }
}
