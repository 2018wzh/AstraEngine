use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::RuntimeError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SerializedEffectEnvelope {
    pub domain: String,
    pub schema: String,
    pub hash: Hash256,
    pub bytes: Vec<u8>,
}

impl SerializedEffectEnvelope {
    pub fn postcard<T: Serialize>(
        domain: impl Into<String>,
        schema: impl Into<String>,
        value: &T,
    ) -> Result<Self, RuntimeError> {
        let bytes = postcard::to_allocvec(value)
            .map_err(|err| RuntimeError::message(format!("encode runtime effect: {err}")))?;
        Ok(Self {
            domain: domain.into(),
            schema: schema.into(),
            hash: Hash256::from_sha256(&bytes),
            bytes,
        })
    }

    pub fn validate_hash(&self) -> bool {
        Hash256::from_sha256(&self.bytes) == self.hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeEffectRecord {
    pub step: u64,
    pub sequence: u64,
    pub envelope: SerializedEffectEnvelope,
}
