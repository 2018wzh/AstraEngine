use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterLayer {
    FinalFrame,
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilterBinding {
    pub preset_id: String,
    pub layer: FilterLayer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilterGraph {
    pub schema: String,
    pub bindings: Vec<FilterBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterValidation {
    pub active: Vec<FilterBinding>,
    pub diagnostics: Vec<String>,
}

impl FilterGraph {
    pub fn validate(&self, layer_metadata: Option<&BTreeSet<String>>) -> FilterValidation {
        let mut active = Vec::new();
        let mut diagnostics = Vec::new();
        let mut keys = BTreeSet::new();
        for binding in &self.bindings {
            let key = format!("{}:{:?}", binding.preset_id, binding.layer);
            if !keys.insert(key) {
                diagnostics.push("ASTRA_EMU_FILTER_DUPLICATE_BINDING".to_owned());
                continue;
            }
            match &binding.layer {
                FilterLayer::FinalFrame => active.push(binding.clone()),
                FilterLayer::Named(layer) => match layer_metadata {
                    Some(layers) if layers.contains(layer) => active.push(binding.clone()),
                    Some(_) => diagnostics.push(format!("ASTRA_EMU_FILTER_LAYER_MISSING:{layer}")),
                    None => diagnostics.push(format!(
                        "ASTRA_EMU_FILTER_LAYER_METADATA_UNAVAILABLE:{layer}"
                    )),
                },
            }
        }
        FilterValidation {
            active,
            diagnostics,
        }
    }
}
