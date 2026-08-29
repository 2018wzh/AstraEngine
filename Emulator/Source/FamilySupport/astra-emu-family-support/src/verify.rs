use std::io::Read;

use astra_core::Hash256;
use astra_emu_family_core::{LegacyCoreError, LegacyMountedVfs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VFS_VERIFY_REPORT_SCHEMA: &str = "astra.emu.vfs.verify.v2";
const VERIFY_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const REREAD_BYTES: u64 = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsVerifyReport {
    pub schema: String,
    pub family_id: String,
    pub source_count: u64,
    pub entry_count: u64,
    pub range_count: u64,
    pub byte_count: u64,
    pub aggregate_hash: Hash256,
}

pub fn verify_vfs(vfs: &dyn LegacyMountedVfs) -> Result<LegacyVfsVerifyReport, LegacyCoreError> {
    let manifest = vfs.manifest();
    manifest.validate(10_000_000)?;
    vfs.validate_sources()?;
    let mut aggregate = Sha256::new();
    let mut range_count = 0u64;
    let mut byte_count = 0u64;

    for (entry_ordinal, entry) in manifest.entries.iter().enumerate() {
        let mut offset = 0u64;
        let mut entry_hash = Sha256::new();
        let mut first = Vec::new();
        let mut tail = Vec::new();
        let mut stream = vfs.open_stream(&entry.uri).inspect_err(|error| {
            tracing::error!(
                event = "astra_emu_vfs_verify_entry_failed",
                diagnostic_code = error.code(),
                source_id = %entry.source_id,
                entry_ordinal,
                source_offset = entry.source_offset,
                stored_size = entry.stored_size,
                decoded_size = entry.decoded_size,
                phase = "open_stream"
            );
        })?;
        while offset < entry.decoded_size {
            let length = (entry.decoded_size - offset).min(VERIFY_CHUNK_BYTES);
            let expected = usize::try_from(length).map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_VERIFY_RANGE",
                    "verify range does not fit memory bounds",
                )
            })?;
            let mut bytes = vec![0u8; expected];
            stream.read_exact(&mut bytes).map_err(|_| {
                tracing::error!(
                    event = "astra_emu_vfs_verify_entry_failed",
                    diagnostic_code = "ASTRA_EMU_VFS_VERIFY_STREAM",
                    source_id = %entry.source_id,
                    entry_ordinal,
                    source_offset = entry.source_offset,
                    stored_size = entry.stored_size,
                    decoded_size = entry.decoded_size,
                    decoded_offset = offset,
                    phase = "stream_read"
                );
                invalid(
                    "ASTRA_EMU_VFS_VERIFY_STREAM",
                    "VFS decoded stream failed before its declared size",
                )
            })?;
            if offset == 0 {
                first.extend_from_slice(&bytes[..bytes.len().min(REREAD_BYTES as usize)]);
            }
            tail.extend_from_slice(&bytes);
            if tail.len() > REREAD_BYTES as usize {
                tail.drain(..tail.len() - REREAD_BYTES as usize);
            }
            entry_hash.update(&bytes);
            offset = offset.checked_add(length).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_VERIFY_OVERFLOW",
                    "verification offset overflowed",
                )
            })?;
            range_count += 1;
            byte_count = byte_count.checked_add(length).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_VERIFY_OVERFLOW",
                    "verification byte count overflowed",
                )
            })?;
        }
        let mut eof_probe = [0u8; 1];
        match stream.read(&mut eof_probe) {
            Ok(0) => {}
            Ok(_) => {
                return Err(invalid(
                    "ASTRA_EMU_VFS_VERIFY_STREAM_SIZE",
                    "VFS decoded stream exceeds its declared size",
                ));
            }
            Err(_) => {
                tracing::error!(
                    event = "astra_emu_vfs_verify_entry_failed",
                    diagnostic_code = "ASTRA_EMU_VFS_VERIFY_STREAM",
                    source_id = %entry.source_id,
                    entry_ordinal,
                    source_offset = entry.source_offset,
                    stored_size = entry.stored_size,
                    decoded_size = entry.decoded_size,
                    decoded_offset = entry.decoded_size,
                    phase = "stream_eof"
                );
                return Err(invalid(
                    "ASTRA_EMU_VFS_VERIFY_STREAM",
                    "VFS decoded stream failed while validating EOF",
                ));
            }
        }
        let digest: [u8; 32] = entry_hash.finalize().into();
        let content_hash = Hash256::from_bytes(digest);
        if entry
            .content_hash
            .is_some_and(|expected| expected != content_hash)
        {
            return Err(invalid(
                "ASTRA_EMU_VFS_VERIFY_CONTENT_HASH",
                "decoded entry hash does not match the manifest",
            ));
        }
        let first_read = reread(vfs, &entry.uri, 0, &first)?;
        let tail_offset = entry.decoded_size.saturating_sub(tail.len() as u64);
        let tail_read = reread(vfs, &entry.uri, tail_offset, &tail)?;
        for read in [first_read, tail_read].into_iter().flatten() {
            range_count = range_count.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_VERIFY_OVERFLOW",
                    "verification range count overflowed",
                )
            })?;
            byte_count = byte_count.checked_add(read.byte_count).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_VERIFY_OVERFLOW",
                    "verification byte count overflowed",
                )
            })?;
        }
        aggregate.update(entry.entry_id.as_bytes());
        aggregate.update(entry.decoded_size.to_le_bytes());
        aggregate.update(content_hash.as_bytes());
        let entries_verified = entry_ordinal + 1;
        if entries_verified.is_multiple_of(256) || entries_verified == manifest.entries.len() {
            tracing::info!(
                event = "astra_emu_vfs_verify_progress",
                entries_verified,
                entry_count = manifest.entries.len(),
                range_count,
                byte_count
            );
        }
    }
    vfs.validate_sources()?;
    let aggregate_hash = Hash256::from_bytes(aggregate.finalize().into());
    Ok(LegacyVfsVerifyReport {
        schema: VFS_VERIFY_REPORT_SCHEMA.into(),
        family_id: manifest.family_id.clone(),
        source_count: manifest.sources.len() as u64,
        entry_count: manifest.entries.len() as u64,
        range_count,
        byte_count,
        aggregate_hash,
    })
}

fn reread(
    vfs: &dyn LegacyMountedVfs,
    uri: &str,
    offset: u64,
    expected: &[u8],
) -> Result<Option<RereadEvidence>, LegacyCoreError> {
    if expected.is_empty() {
        return Ok(None);
    }
    let read = vfs.read_range(uri, offset, expected.len() as u64)?;
    if read.offset != offset || read.bytes.as_slice() != expected {
        return Err(invalid(
            "ASTRA_EMU_VFS_VERIFY_REREAD",
            "random verification reread differs from the streamed bytes",
        ));
    }
    Ok(Some(RereadEvidence {
        byte_count: expected.len() as u64,
    }))
}

struct RereadEvidence {
    byte_count: u64,
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use crate::test_support::MemoryVfs;

    use super::verify_vfs;

    #[test]
    fn full_stream_and_random_rereads_are_counted() {
        let vfs = MemoryVfs::new(&[("test:/scr/a.sc", b"abcdef", "script")]);
        let report = verify_vfs(&vfs).unwrap();
        assert_eq!(report.entry_count, 1);
        assert_eq!(report.range_count, 3);
        assert_eq!(report.byte_count, 18);
    }
}
