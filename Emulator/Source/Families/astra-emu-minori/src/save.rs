use astra_core::Hash256;
use serde::{Deserialize, Serialize};

use crate::MinoriConfigState;

pub(crate) const MINORI_SAVE_SCHEMA: &str = "astra.emu.minori.save_slot.v3";
pub(crate) const MINORI_SAVE_ROOT: &str = "minori/saves";
pub(crate) const MINORI_SAVE_MAX_SLOTS: u32 = 100;
/// The original SaveLoad page uses ten records per page.  Its filename
/// expression is `page * 10 + slot`, with page 1 reserved for Quick Save.
/// Keep this identity separate from the user-visible manual page index so a
/// quick save cannot accidentally overwrite Auto Save slot 0.
pub(crate) const MINORI_SAVE_PAGE_WIDTH: u32 = 10;
pub(crate) const MINORI_QUICK_SAVE_PAGE_INDEX: u32 = 1;
pub(crate) const MINORI_MANUAL_SAVE_FIRST_PAGE_INDEX: u32 = 2;
/// The original quick save rotates through the ten Page1 file numbers
/// (10..19).  The rotation cursor is persisted in the system parameters and
/// the executed file number is `cursor + 10`; a successful quick save
/// advances the cursor modulo the page width.
pub(crate) const MINORI_QUICK_SAVE_SLOT_COUNT: u32 = 10;
pub(crate) const MINORI_SAVE_MAX_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MINORI_SAVE_COMMENT_MAX_BYTES: usize = 256;
pub(crate) const MINORI_SAVE_TIMESTAMP_MAX_BYTES: usize = 16;
pub(crate) const MINORI_SAVE_THUMBNAIL_WIDTH: u32 = 96;
pub(crate) const MINORI_SAVE_THUMBNAIL_HEIGHT: u32 = 54;
pub(crate) const MINORI_SAVE_THUMBNAIL_MAX_BYTES: usize = 1024 * 1024;
pub(crate) const MINORI_CONFIG_SCHEMA: &str = "astra.emu.minori.config.v2";
pub(crate) const MINORI_CONFIG_ROOT: &str = "minori";
pub(crate) const MINORI_CONFIG_PATH: &str = "minori/config-v1.bin";
pub(crate) const MINORI_CONFIG_TEMPORARY_PATH: &str = "minori/config-v1.tmp";
pub(crate) const MINORI_CONFIG_MAX_BYTES: usize = 64 * 1024;

pub(crate) fn slot_path(slot: u32) -> String {
    format!("{MINORI_SAVE_ROOT}/slot-{slot:03}.bin")
}

pub(crate) fn slot_temporary_path(slot: u32) -> String {
    format!("{MINORI_SAVE_ROOT}/slot-{slot:03}.tmp")
}

/// Maps the persisted quick-save rotation cursor to the executed global file
/// number.  The original getter returns `quickSaveFileNumber + 10`, so the
/// ten cursors cover exactly the Page1 Quick Save file numbers 10..19.
pub(crate) fn quick_save_file_number(cursor: u32) -> u32 {
    MINORI_QUICK_SAVE_PAGE_INDEX * MINORI_SAVE_PAGE_WIDTH + cursor
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
    pub timestamp: String,
    pub comment: String,
    pub thumbnail_png: Vec<u8>,
    pub vm_snapshot: Vec<u8>,
}

pub(crate) fn encode(envelope: &MinoriSaveEnvelope) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(envelope)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<MinoriSaveEnvelope, postcard::Error> {
    postcard::from_bytes(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MinoriConfigEnvelope {
    pub schema: String,
    pub case_fingerprint: Hash256,
    pub package_hash: Hash256,
    pub profile_fingerprint: Hash256,
    pub config: MinoriConfigState,
    /// Persisted Quick Save rotation cursor in 0..10.  The original keeps
    /// `quickSaveFileNumber` in the installation-scoped system parameters, so
    /// the rotation survives restarts instead of restarting at Page1 slot 0.
    pub quick_save_cursor: u32,
}

pub(crate) fn encode_config(envelope: &MinoriConfigEnvelope) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(envelope)
}

pub(crate) fn decode_config(bytes: &[u8]) -> Result<MinoriConfigEnvelope, postcard::Error> {
    postcard::from_bytes(bytes)
}
