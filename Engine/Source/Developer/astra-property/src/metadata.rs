use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TypeMetadata {
    pub type_name: &'static str,
    pub fields: Vec<PropertyField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PropertyField {
    pub name: &'static str,
    pub rust_type: &'static str,
    pub schema: &'static str,
    pub inspector: InspectorField,
    pub save: SaveField,
}

impl PropertyField {
    pub fn new(name: &'static str, rust_type: &'static str) -> Self {
        Self {
            name,
            rust_type,
            schema: rust_type,
            inspector: InspectorField {
                label: name,
                read_only: false,
            },
            save: SaveField { included: true },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InspectorField {
    pub label: &'static str,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveField {
    pub included: bool,
}

pub trait PropertyDescribe {
    fn property_metadata() -> TypeMetadata;
}
