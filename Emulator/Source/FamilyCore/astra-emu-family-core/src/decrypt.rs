use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LegacyCoreError;

pub const LEGACY_DECRYPT_MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;
pub const LEGACY_DECRYPT_MAX_BATCH_ENTRIES: usize = 64;
pub const LEGACY_DECRYPT_CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const LEGACY_DECRYPT_MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyDecryptPhase {
    Index,
    Entry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyOpaqueDescriptor {
    pub schema_id: String,
    pub schema_hash: Hash256,
    pub payload: Vec<u8>,
}

impl LegacyOpaqueDescriptor {
    pub fn validate(&self) -> Result<(), LegacyCoreError> {
        if !safe_symbol(&self.schema_id)
            || self.payload.is_empty()
            || self.payload.len() > LEGACY_DECRYPT_MAX_DESCRIPTOR_BYTES
        {
            return Err(LegacyCoreError::invalid(
                "ASTRA_EMU_DECRYPT_DESCRIPTOR",
                "decrypt descriptor identity or payload is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyDecryptTransport {
    pub chunk_offset: u64,
    pub total_size: u64,
    pub batch_index: u32,
    pub input_bound: u64,
    pub output_bound: u64,
}

impl LegacyDecryptTransport {
    pub fn validate(&self, input_len: usize) -> Result<(), LegacyCoreError> {
        let end = self
            .chunk_offset
            .checked_add(input_len as u64)
            .ok_or_else(|| {
                LegacyCoreError::invalid(
                    "ASTRA_EMU_DECRYPT_RANGE",
                    "decrypt chunk range overflowed",
                )
            })?;
        if input_len == 0
            || input_len > LEGACY_DECRYPT_CHUNK_BYTES
            || self.total_size == 0
            || self.total_size > LEGACY_DECRYPT_MAX_BATCH_BYTES as u64
            || end > self.total_size
            || self.input_bound == 0
            || self.input_bound > LEGACY_DECRYPT_MAX_BATCH_BYTES as u64
            || input_len as u64 > self.input_bound
            || self.output_bound == 0
            || self.output_bound > LEGACY_DECRYPT_MAX_BATCH_BYTES as u64
        {
            return Err(LegacyCoreError::invalid(
                "ASTRA_EMU_DECRYPT_TRANSPORT",
                "decrypt transport is outside the configured bounds",
            ));
        }
        Ok(())
    }
}

pub struct LegacyDecryptRequest<'a> {
    pub phase: LegacyDecryptPhase,
    pub descriptors: &'a [LegacyOpaqueDescriptor],
    pub transport: LegacyDecryptTransport,
    pub bytes: &'a [u8],
}

pub trait LegacyDecryptProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn private_profile_hash(&self) -> Hash256;
    fn descriptor_schema_id(&self) -> &str;
    fn descriptor_schema_hash(&self) -> Hash256;
    fn decrypt(&self, request: LegacyDecryptRequest<'_>) -> Result<Vec<u8>, LegacyCoreError>;
}

pub fn validate_decrypt_request(
    provider: &dyn LegacyDecryptProvider,
    request: &LegacyDecryptRequest<'_>,
) -> Result<(), LegacyCoreError> {
    if request.descriptors.is_empty()
        || request.descriptors.len() > LEGACY_DECRYPT_MAX_BATCH_ENTRIES
    {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_DECRYPT_DESCRIPTOR_COUNT",
            "decrypt descriptor batch is empty or exceeds its entry bound",
        ));
    }
    for descriptor in request.descriptors {
        descriptor.validate()?;
    }
    request.transport.validate(request.bytes.len())?;
    if request.descriptors.iter().any(|descriptor| {
        descriptor.schema_id != provider.descriptor_schema_id()
            || descriptor.schema_hash != provider.descriptor_schema_hash()
    }) {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_DECRYPT_SCHEMA",
            "decrypt descriptor schema does not match the provider",
        ));
    }
    Ok(())
}

pub fn validate_decrypt_output(
    request: &LegacyDecryptRequest<'_>,
    output: &[u8],
) -> Result<(), LegacyCoreError> {
    if output.is_empty() || output.len() as u64 > request.transport.output_bound {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_DECRYPT_OUTPUT",
            "decrypt output is empty or exceeds its declared bound",
        ));
    }
    Ok(())
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureProvider;
    impl LegacyDecryptProvider for FixtureProvider {
        fn provider_id(&self) -> &str {
            "fixture.decrypt.v1"
        }
        fn private_profile_hash(&self) -> Hash256 {
            Hash256::from_sha256(b"private")
        }
        fn descriptor_schema_id(&self) -> &str {
            "fixture.descriptor.v1"
        }
        fn descriptor_schema_hash(&self) -> Hash256 {
            Hash256::from_sha256(self.descriptor_schema_id().as_bytes())
        }
        fn decrypt(&self, request: LegacyDecryptRequest<'_>) -> Result<Vec<u8>, LegacyCoreError> {
            validate_decrypt_request(self, &request)?;
            Ok(request.bytes.to_vec())
        }
    }

    fn descriptor() -> LegacyOpaqueDescriptor {
        LegacyOpaqueDescriptor {
            schema_id: "fixture.descriptor.v1".into(),
            schema_hash: Hash256::from_sha256(b"fixture.descriptor.v1"),
            payload: vec![1],
        }
    }

    #[test]
    fn transport_and_descriptor_bounds_are_enforced() {
        let provider = FixtureProvider;
        let descriptor = descriptor();
        let request = LegacyDecryptRequest {
            phase: LegacyDecryptPhase::Entry,
            descriptors: std::slice::from_ref(&descriptor),
            transport: LegacyDecryptTransport {
                chunk_offset: 0,
                total_size: 4,
                batch_index: 0,
                input_bound: 4,
                output_bound: 4,
            },
            bytes: b"data",
        };
        assert_eq!(provider.decrypt(request).unwrap(), b"data");
        let request = LegacyDecryptRequest {
            phase: LegacyDecryptPhase::Entry,
            descriptors: std::slice::from_ref(&descriptor),
            transport: LegacyDecryptTransport {
                chunk_offset: 0,
                total_size: 4,
                batch_index: 0,
                input_bound: 4,
                output_bound: 4,
            },
            bytes: &[0; LEGACY_DECRYPT_CHUNK_BYTES + 1],
        };
        assert_eq!(
            provider.decrypt(request).unwrap_err().code(),
            "ASTRA_EMU_DECRYPT_TRANSPORT"
        );
    }

    #[test]
    fn schema_and_descriptor_count_mismatch_are_blocking() {
        let provider = FixtureProvider;
        let mut wrong = descriptor();
        wrong.schema_hash = Hash256::from_sha256(b"wrong");
        let request = LegacyDecryptRequest {
            phase: LegacyDecryptPhase::Index,
            descriptors: std::slice::from_ref(&wrong),
            transport: LegacyDecryptTransport {
                chunk_offset: 0,
                total_size: 1,
                batch_index: 0,
                input_bound: 1,
                output_bound: 1,
            },
            bytes: b"x",
        };
        assert_eq!(
            provider.decrypt(request).unwrap_err().code(),
            "ASTRA_EMU_DECRYPT_SCHEMA"
        );
    }
}
