use std::collections::BTreeMap;

use astra_core::Hash128;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::PolicyError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyValue {
    Nil,
    Bool(bool),
    Integer(i64),
    String(String),
    Object(BTreeMap<String, PolicyValue>),
}

impl PolicyValue {
    pub fn validate_depth(&self, max_depth: usize) -> Result<(), PolicyError> {
        self.validate_at_depth(0, max_depth)
    }

    pub fn stable_hash(&self) -> Hash128 {
        let bytes = postcard::to_allocvec(self).expect("PolicyValue serialization is infallible");
        Hash128::from_blake3(&bytes)
    }

    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> Result<(), PolicyError> {
        if depth > max_depth {
            return Err(PolicyError::diagnostic(
                "ASTRA_POLICY_VALUE_DEPTH",
                "policy value nesting exceeds the configured depth",
            ));
        }
        if let Self::Object(values) = self {
            for value in values.values() {
                value.validate_at_depth(depth + 1, max_depth)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyCommandRecord {
    pub api: String,
    pub name: String,
    pub payload: PolicyValue,
    pub replay_event: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyQueryRecord {
    pub api: String,
    pub target: String,
    pub args: BTreeMap<String, PolicyValue>,
    pub result_hash: Hash128,
    pub replay_event: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyTraceRecord {
    pub api: String,
    pub kind: String,
    pub fields: PolicyValue,
    pub replay_event: String,
}
