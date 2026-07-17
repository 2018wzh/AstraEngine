use std::collections::BTreeMap;

use astra_emu_family_api::{LegacyProviderError, LegacyVfsReader};
use astra_emu_manager_core::{
    CancellationToken, GrantedSourceEntry, GrantedSourceReader, SourceScanError,
};

/// Compile-time adapter for platforms whose document-provider integration is
/// outside the current evidence level. It never manufactures a filesystem
/// fallback: every operation fails with a stable platform diagnostic.
#[derive(Default)]
pub struct UnsupportedVfsRegistry;

impl UnsupportedVfsRegistry {
    pub fn bind(&self, _mount_set_id: &str, _platform_token: &str) -> Result<(), String> {
        Err("PLATFORM_NOT_IMPLEMENTED: source grant provider is unavailable".into())
    }

    pub fn install_overlays(
        &self,
        _mount_set_id: &str,
        _overlays: BTreeMap<String, Vec<u8>>,
    ) -> Result<(), String> {
        Err("PLATFORM_NOT_IMPLEMENTED: source grant provider is unavailable".into())
    }

    pub fn unbind(&self, _mount_set_id: &str) {}
}

impl LegacyVfsReader for UnsupportedVfsRegistry {
    fn stat_file(
        &self,
        _mount_set_id: &str,
        _uri: &str,
    ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
        Err(LegacyProviderError::invalid(
            "PLATFORM_NOT_IMPLEMENTED",
            "source grant provider is unavailable",
        ))
    }

    fn read_file_range(
        &self,
        _mount_set_id: &str,
        _uri: &str,
        _expected_revision: astra_byte_source::SourceRevision,
        _range: astra_byte_source::ByteRange,
        _max_bytes: u64,
    ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError> {
        Err(LegacyProviderError::invalid(
            "PLATFORM_NOT_IMPLEMENTED",
            "source grant provider is unavailable",
        ))
    }
}

pub struct UnsupportedGrantedSource;

impl UnsupportedGrantedSource {
    pub fn new(_platform_token: &str) -> Result<Self, SourceScanError> {
        Err(SourceScanError::Enumeration)
    }
}

impl GrantedSourceReader for UnsupportedGrantedSource {
    fn enumerate(
        &self,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<GrantedSourceEntry>, SourceScanError> {
        Err(SourceScanError::Enumeration)
    }

    fn read_file(&self, _relative_path: &str, _max_bytes: u64) -> Result<Vec<u8>, SourceScanError> {
        Err(SourceScanError::Read)
    }
}
