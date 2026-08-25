use astra_core::Hash256;
use serde::{Deserialize, Serialize};

pub(crate) const MINORI_SAVE_SCHEMA: &str = "astra.emu.minori.save_slot.v1";
pub(crate) const MINORI_SAVE_ROOT: &str = "minori/saves";
pub(crate) const MINORI_SAVE_MAX_SLOTS: u32 = 100;
pub(crate) const MINORI_SAVE_MAX_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn slot_path(slot: u32) -> String {
    format!("{MINORI_SAVE_ROOT}/slot-{slot:03}.bin")
}

pub(crate) fn slot_temporary_path(slot: u32) -> String {
    format!("{MINORI_SAVE_ROOT}/slot-{slot:03}.tmp")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MinoriSaveEnvelope {
    pub schema: String,
    pub case_fingerprint: Hash256,
    pub package_hash: Hash256,
    pub profile_fingerprint: Hash256,
    pub script_uri: String,
    pub script_hash: Hash256,
    pub vm_snapshot: Vec<u8>,
}

pub(crate) fn encode(envelope: &MinoriSaveEnvelope) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(envelope)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<MinoriSaveEnvelope, postcard::Error> {
    postcard::from_bytes(bytes)
}
