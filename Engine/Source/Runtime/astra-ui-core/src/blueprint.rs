use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_id, validate_serialized_size, validate_string, UiCapability, UiValidationError,
    UiValue, ValidateUi, MAX_COMPONENT_INSTANCES_PER_VIEW, MAX_NODES_PER_VIEW, MAX_TREE_DEPTH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiSourceRole {
    Story,
    Ui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiBindingRoot {
    Model,
    Item,
    Event,
    State,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiBlueprintFrameModel {
    pub schema: String,
    pub view_id: String,
    pub model: UiValue,
    pub state: UiValue,
    pub localization: BTreeMap<String, String>,
}

impl ValidateUi for UiBlueprintFrameModel {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_blueprint_frame_model.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_FRAME_SCHEMA",
                "blueprint frame model schema must be astra.ui_blueprint_frame_model.v1",
            ));
        }
        validate_id("blueprint_frame.view_id", &self.view_id)?;
        self.model.validate()?;
        self.state.validate()?;
        for (key, value) in &self.localization {
            validate_id("blueprint_frame.localization_key", key)?;
            validate_string("blueprint_frame.localization_value", value)?;
        }
        validate_serialized_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiValueExpr {
    Literal {
        value: UiValue,
    },
    Binding {
        root: UiBindingRoot,
        path: Vec<String>,
    },
    LocalizationKey {
        key: String,
    },
    AssetRef {
        asset_id: String,
    },
    ThemeToken {
        token: String,
    },
}

impl UiValueExpr {
    fn validate(&self) -> Result<(), UiValidationError> {
        match self {
            Self::Literal { value } => value.validate(),
            Self::Binding { path, .. } => {
                if path.is_empty() {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_BINDING_PATH_EMPTY",
                        "binding path must contain at least one segment",
                    ));
                }
                for segment in path {
                    validate_id("binding.path", segment)?;
                }
                Ok(())
            }
            Self::LocalizationKey { key } => validate_id("localization.key", key),
            Self::AssetRef { asset_id } => validate_id("asset.id", asset_id),
            Self::ThemeToken { token } => validate_id("theme.token", token),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiEventBinding {
    pub event: String,
    pub action_id: String,
    pub arguments: BTreeMap<String, UiValueExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiRepeatBinding {
    pub items: UiValueExpr,
    pub item_key_path: Vec<String>,
    pub overscan: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiNodeBlueprint {
    pub source_id: String,
    pub local_id: String,
    pub widget: String,
    pub properties: BTreeMap<String, UiValueExpr>,
    pub events: Vec<UiEventBinding>,
    pub children: Vec<UiNodeBlueprint>,
    pub repeat: Option<UiRepeatBinding>,
    pub component_id: Option<String>,
}

impl UiNodeBlueprint {
    fn validate_tree(
        &self,
        depth: usize,
        node_count: &mut usize,
        component_count: &mut usize,
        identities: &mut BTreeSet<String>,
    ) -> Result<(), UiValidationError> {
        if depth > MAX_TREE_DEPTH {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_DEPTH",
                format!("UI blueprint depth exceeds {MAX_TREE_DEPTH}"),
            ));
        }
        *node_count += 1;
        if *node_count > MAX_NODES_PER_VIEW {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_NODE_LIMIT",
                format!("UI blueprint exceeds {MAX_NODES_PER_VIEW} nodes"),
            ));
        }
        validate_id("node.source_id", &self.source_id)?;
        validate_id("node.local_id", &self.local_id)?;
        validate_id("node.widget", &self.widget)?;
        if !identities.insert(self.local_id.clone()) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_ID_DUPLICATE",
                format!("duplicate local node id {}", self.local_id),
            ));
        }
        for (name, value) in &self.properties {
            validate_id("node.property", name)?;
            value.validate()?;
        }
        let mut events = BTreeSet::new();
        for event in &self.events {
            validate_id("node.event", &event.event)?;
            validate_id("node.action_id", &event.action_id)?;
            if !events.insert(event.event.as_str()) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_EVENT_DUPLICATE",
                    format!(
                        "node {} binds event {} more than once",
                        self.local_id, event.event
                    ),
                ));
            }
            for (name, value) in &event.arguments {
                validate_id("event.argument", name)?;
                value.validate()?;
            }
        }
        if let Some(repeat) = &self.repeat {
            repeat.items.validate()?;
            if repeat.item_key_path.is_empty() {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_REPEAT_KEY_EMPTY",
                    "repeat item_key_path must not be empty",
                ));
            }
            for segment in &repeat.item_key_path {
                validate_id("repeat.item_key", segment)?;
            }
        }
        if let Some(component_id) = &self.component_id {
            validate_id("node.component_id", component_id)?;
            *component_count += 1;
            if *component_count > MAX_COMPONENT_INSTANCES_PER_VIEW {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_COMPONENT_INSTANCE_LIMIT",
                    format!("view exceeds {MAX_COMPONENT_INSTANCES_PER_VIEW} component instances"),
                ));
            }
        }
        for child in &self.children {
            child.validate_tree(depth + 1, node_count, component_count, identities)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiViewBlueprint {
    pub id: String,
    pub source_id: String,
    pub model_schema: String,
    pub theme_id: String,
    pub required_capabilities: Vec<UiCapability>,
    pub root: UiNodeBlueprint,
}

impl ValidateUi for UiViewBlueprint {
    fn validate(&self) -> Result<(), UiValidationError> {
        validate_id("view.id", &self.id)?;
        validate_id("view.source_id", &self.source_id)?;
        validate_id("view.model_schema", &self.model_schema)?;
        validate_id("view.theme_id", &self.theme_id)?;
        let capabilities: BTreeSet<_> = self.required_capabilities.iter().copied().collect();
        if capabilities.len() != self.required_capabilities.len() {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_CAPABILITY_DUPLICATE",
                "view capabilities must be unique",
            ));
        }
        let mut nodes = 0;
        let mut components = 0;
        self.root
            .validate_tree(0, &mut nodes, &mut components, &mut BTreeSet::new())?;
        validate_serialized_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiBlueprintBundle {
    pub schema: String,
    pub views: BTreeMap<String, UiViewBlueprint>,
    pub hash: Hash256,
}

impl UiBlueprintBundle {
    pub fn compute_hash(&self) -> Result<Hash256, UiValidationError> {
        let bytes = postcard::to_allocvec(&(&self.schema, &self.views)).map_err(|error| {
            UiValidationError::invalid("ASTRA_UI_BLUEPRINT_ENCODE", error.to_string())
        })?;
        Ok(Hash256::from_sha256(&bytes))
    }
}

impl ValidateUi for UiBlueprintBundle {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_blueprint_bundle.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_SCHEMA",
                "blueprint bundle schema must be astra.ui_blueprint_bundle.v1",
            ));
        }
        if self.views.is_empty() {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_EMPTY",
                "blueprint bundle must contain at least one view",
            ));
        }
        for (id, view) in &self.views {
            validate_id("bundle.view", id)?;
            if id != &view.id {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_BLUEPRINT_KEY_MISMATCH",
                    format!("view map key {id} does not match embedded id {}", view.id),
                ));
            }
            view.validate()?;
        }
        if self.compute_hash()? != self.hash {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_HASH",
                "blueprint bundle content hash mismatch",
            ));
        }
        validate_serialized_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiViewBinding {
    pub view_id: String,
    pub controller_id: String,
    pub policy_bundle_id: String,
    pub theme_id: String,
}

impl UiViewBinding {
    pub fn validate(&self) -> Result<(), UiValidationError> {
        validate_id("binding.view_id", &self.view_id)?;
        validate_id("binding.controller_id", &self.controller_id)?;
        validate_id("binding.policy_bundle_id", &self.policy_bundle_id)?;
        validate_id("binding.theme_id", &self.theme_id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiBindingManifest {
    pub schema: String,
    pub command_bindings: BTreeMap<String, UiViewBinding>,
    pub system_page_bindings: BTreeMap<String, UiViewBinding>,
    pub surface_bindings: BTreeMap<String, UiViewBinding>,
    pub profile_bindings: BTreeMap<String, UiViewBinding>,
    pub hash: Hash256,
}

impl UiBindingManifest {
    pub fn compute_hash(&self) -> Result<Hash256, UiValidationError> {
        let bytes = postcard::to_allocvec(&(
            &self.schema,
            &self.command_bindings,
            &self.system_page_bindings,
            &self.surface_bindings,
            &self.profile_bindings,
        ))
        .map_err(|error| {
            UiValidationError::invalid("ASTRA_UI_BINDING_ENCODE", error.to_string())
        })?;
        Ok(Hash256::from_sha256(&bytes))
    }
}

impl ValidateUi for UiBindingManifest {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_binding_manifest.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BINDING_SCHEMA",
                "binding manifest schema must be astra.ui_binding_manifest.v1",
            ));
        }
        for bindings in [
            &self.command_bindings,
            &self.system_page_bindings,
            &self.surface_bindings,
            &self.profile_bindings,
        ] {
            for (key, binding) in bindings {
                validate_id("binding.key", key)?;
                binding.validate()?;
            }
        }
        if self.compute_hash()? != self.hash {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BINDING_HASH",
                "binding manifest content hash mismatch",
            ));
        }
        validate_serialized_size(self)
    }
}

pub fn validate_ui_source_text(text: &str) -> Result<(), UiValidationError> {
    validate_string("ui.source", text)
}
