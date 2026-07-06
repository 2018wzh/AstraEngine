use std::collections::BTreeMap;

use astra_core::StableId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum BlackboardValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<BlackboardValue>),
    Map(BTreeMap<String, BlackboardValue>),
    StableId(StableId),
}

impl From<&str> for BlackboardValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for BlackboardValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for BlackboardValue {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

impl From<bool> for BlackboardValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Blackboard {
    values: BTreeMap<String, BlackboardValue>,
}

impl Blackboard {
    pub fn set(&mut self, key: impl Into<String>, value: BlackboardValue) {
        self.values.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&BlackboardValue> {
        self.values.get(key)
    }

    pub fn values(&self) -> &BTreeMap<String, BlackboardValue> {
        &self.values
    }
}
