use astra_core::SchemaId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ComponentId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeMutationRecord {
    pub step: u64,
    pub component_id: ComponentId,
    pub schema: SchemaId,
    pub before_revision: u64,
    pub after_revision: u64,
    pub source: String,
}
