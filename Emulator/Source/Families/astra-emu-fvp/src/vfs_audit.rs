use astra_byte_source::{ByteRange, ByteSourceError, DEFAULT_MAX_RANGE_BYTES};
use astra_core::Hash256;
use astra_emu_family_core::LegacyCoreError;
use sha2::{Digest, Sha256};

use crate::FvpArchive;

type EntryAudit = (Hash256, Vec<u8>);
type ArchiveAudit = (Hash256, Vec<EntryAudit>);

pub(crate) fn audit_archive(archive: &FvpArchive) -> Result<ArchiveAudit, LegacyCoreError> {
    let source = archive.source().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_FVP_SOURCE_MISSING",
            "mounted FVP archive source is unavailable",
        )
    })?;
    let stat = archive.stat();
    let mut ordered = archive
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let end = entry.offset.checked_add(entry.size).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FVP_ENTRY_OVERFLOW",
                    "FVP entry range overflowed during audit",
                )
            })?;
            if entry.size == 0 || end > stat.len {
                return Err(invalid(
                    "ASTRA_EMU_FVP_ENTRY_BOUNDS",
                    "FVP entry is empty or outside its archive",
                ));
            }
            Ok((entry.offset, end, index))
        })
        .collect::<Result<Vec<_>, LegacyCoreError>>()?;
    ordered.sort_unstable_by_key(|(start, end, index)| (*start, *end, *index));
    for pair in ordered.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err(invalid(
                "ASTRA_EMU_FVP_ENTRY_OVERLAP",
                "FVP archive contains overlapping entry ranges",
            ));
        }
    }

    let mut source_digest = Sha256::new();
    let mut entry_digests = (0..archive.entries().len())
        .map(|_| Sha256::new())
        .collect::<Vec<_>>();
    let mut entry_magic = (0..archive.entries().len())
        .map(|_| Vec::with_capacity(16))
        .collect::<Vec<_>>();
    let mut observed = vec![0_u64; archive.entries().len()];
    let mut ordered_index = 0_usize;
    let mut offset = 0_u64;
    while offset < stat.len {
        let length = (stat.len - offset).min(DEFAULT_MAX_RANGE_BYTES);
        let result = source
            .read_range(
                stat.revision,
                ByteRange {
                    offset,
                    len: length,
                },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .map_err(byte_source_error)?;
        source_digest.update(&result.bytes);
        let chunk_end = offset.checked_add(length).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_READ_OVERFLOW",
                "FVP archive audit cursor overflowed",
            )
        })?;
        while ordered_index < ordered.len() && ordered[ordered_index].1 <= offset {
            ordered_index += 1;
        }
        let mut candidate = ordered_index;
        while candidate < ordered.len() && ordered[candidate].0 < chunk_end {
            let (entry_start, entry_end, entry_index) = ordered[candidate];
            let overlap_start = entry_start.max(offset);
            let overlap_end = entry_end.min(chunk_end);
            let local_start = usize::try_from(overlap_start - offset).map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_READ_BOUNDS",
                    "FVP audit range does not fit memory",
                )
            })?;
            let local_end = usize::try_from(overlap_end - offset).map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_READ_BOUNDS",
                    "FVP audit range does not fit memory",
                )
            })?;
            let bytes = &result.bytes[local_start..local_end];
            entry_digests[entry_index].update(bytes);
            observed[entry_index] = observed[entry_index]
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_FVP_ENTRY_OVERFLOW",
                        "FVP audited entry length overflowed",
                    )
                })?;
            if entry_magic[entry_index].len() < 16 {
                let needed = 16 - entry_magic[entry_index].len();
                entry_magic[entry_index].extend_from_slice(&bytes[..bytes.len().min(needed)]);
            }
            if entry_end <= chunk_end {
                candidate += 1;
            } else {
                break;
            }
        }
        ordered_index = candidate;
        offset = chunk_end;
    }
    if archive
        .entries()
        .iter()
        .zip(&observed)
        .any(|(entry, observed)| entry.size != *observed)
    {
        return Err(invalid(
            "ASTRA_EMU_FVP_ENTRY_SHORT_READ",
            "FVP archive audit did not observe an entire entry",
        ));
    }
    let audits = entry_digests
        .into_iter()
        .zip(entry_magic)
        .map(|(digest, magic)| (Hash256::from_bytes(digest.finalize().into()), magic))
        .collect();
    Ok((Hash256::from_bytes(source_digest.finalize().into()), audits))
}

fn byte_source_error(error: ByteSourceError) -> LegacyCoreError {
    let code = match error {
        ByteSourceError::RangeLimit => "ASTRA_EMU_VFS_READ_LIMIT",
        ByteSourceError::RangeOverflow => "ASTRA_EMU_VFS_READ_OVERFLOW",
        ByteSourceError::RangeBounds => "ASTRA_EMU_VFS_READ_BOUNDS",
        ByteSourceError::RevisionMismatch => "ASTRA_EMU_VFS_SOURCE_CHANGED",
        ByteSourceError::ShortRead => "ASTRA_EMU_VFS_SHORT_READ",
        ByteSourceError::RepeatMismatch => "ASTRA_EMU_VFS_REPEAT_READ_CHANGED",
        ByteSourceError::Poisoned => "ASTRA_EMU_VFS_SOURCE_POISONED",
        ByteSourceError::Io(_) => "ASTRA_EMU_VFS_SOURCE_IO",
    };
    invalid(code, "FVP bounded archive audit failed")
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}
