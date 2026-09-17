use astra_core::{is_safe_symbol as safe_symbol, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CoreError, MusicaPazDecryptProvider};

pub const PAZ_DECRYPT_MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;
pub const PAZ_DECRYPT_MAX_BATCH_ENTRIES: usize = 64;
pub const PAZ_DECRYPT_CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const PAZ_DECRYPT_MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PazDecryptPhase {
    Index,
    Entry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PazDecryptDescriptor {
    pub schema_id: String,
    pub schema_hash: Hash256,
    pub payload: Vec<u8>,
}

impl PazDecryptDescriptor {
    pub fn validate(&self) -> Result<(), CoreError> {
        if !safe_symbol(&self.schema_id)
            || self.payload.is_empty()
            || self.payload.len() > PAZ_DECRYPT_MAX_DESCRIPTOR_BYTES
        {
            return Err(CoreError::invalid(
                "ASTRA_EMU_DECRYPT_DESCRIPTOR",
                "decrypt descriptor identity or payload is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PazDecryptTransport {
    pub chunk_offset: u64,
    pub total_size: u64,
    pub batch_index: u32,
    pub input_bound: u64,
    pub output_bound: u64,
}

impl PazDecryptTransport {
    pub fn validate(&self, input_len: usize) -> Result<(), CoreError> {
        let end = self
            .chunk_offset
            .checked_add(input_len as u64)
            .ok_or_else(|| {
                CoreError::invalid("ASTRA_EMU_DECRYPT_RANGE", "decrypt chunk range overflowed")
            })?;
        if input_len == 0
            || input_len > PAZ_DECRYPT_CHUNK_BYTES
            || self.total_size == 0
            || self.total_size > PAZ_DECRYPT_MAX_BATCH_BYTES as u64
            || end > self.total_size
            || self.input_bound == 0
            || self.input_bound > PAZ_DECRYPT_MAX_BATCH_BYTES as u64
            || input_len as u64 > self.input_bound
            || self.output_bound == 0
            || self.output_bound > PAZ_DECRYPT_MAX_BATCH_BYTES as u64
        {
            return Err(CoreError::invalid(
                "ASTRA_EMU_DECRYPT_TRANSPORT",
                "decrypt transport is outside the configured bounds",
            ));
        }
        Ok(())
    }
}

pub struct PazDecryptRequest<'a> {
    pub phase: PazDecryptPhase,
    pub descriptors: &'a [PazDecryptDescriptor],
    pub transport: PazDecryptTransport,
    pub bytes: &'a [u8],
}

pub fn validate_decrypt_request(
    provider: &MusicaPazDecryptProvider,
    request: &PazDecryptRequest<'_>,
) -> Result<(), CoreError> {
    if request.descriptors.is_empty() || request.descriptors.len() > PAZ_DECRYPT_MAX_BATCH_ENTRIES {
        return Err(CoreError::invalid(
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
        return Err(CoreError::invalid(
            "ASTRA_EMU_DECRYPT_SCHEMA",
            "decrypt descriptor schema does not match the provider",
        ));
    }
    Ok(())
}

pub fn validate_decrypt_output(
    request: &PazDecryptRequest<'_>,
    output: &[u8],
) -> Result<(), CoreError> {
    if output.is_empty() || output.len() as u64 > request.transport.output_bound {
        return Err(CoreError::invalid(
            "ASTRA_EMU_DECRYPT_OUTPUT",
            "decrypt output is empty or exceeds its declared bound",
        ));
    }
    Ok(())
}
