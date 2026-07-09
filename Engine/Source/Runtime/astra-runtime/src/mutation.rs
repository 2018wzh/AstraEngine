use astra_core::{Hash256, SchemaId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ComponentId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeMutationRecord {
    pub step: u64,
    pub component_id: ComponentId,
    pub schema: SchemaId,
    pub before_hash: Hash256,
    pub after_hash: Hash256,
    pub source: String,
}
