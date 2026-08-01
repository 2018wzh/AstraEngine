use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use astra_core::Hash256;
use astra_emu_minori::REQUIRED_ARCHIVE_ROLES;
use serde::Serialize;

const MAX_SCANNED_FILES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveInventoryReport {
    pub schema: &'static str,
    pub logical_archive_count: u64,
    pub physical_file_count: u64,
    pub physical_byte_count: u64,
    pub required_role_set_match: bool,
    pub inventory_hash: Hash256,
    pub archives: Vec<ArchiveInventoryRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveInventoryRole {
    pub role: String,
    pub physical_file_count: u64,
    pub physical_byte_count: u64,
    pub highest_part: Option<char>,
    pub required: bool,
}

#[derive(Debug)]
struct Part {
    index: u8,
    byte_size: u64,
}

pub fn scan_archive_inventory(
    game_root: &Path,
) -> Result<ArchiveInventoryReport, Box<dyn std::error::Error>> {
    let root = game_root
        .canonicalize()
        .map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_ROOT")?;
    if !root.is_dir() {
        return Err("ASTRA_EMU_MINORI_INVENTORY_ROOT".into());
    }
    let mut pending = vec![root];
    let mut scanned = 0usize;
    let mut roles = BTreeMap::<String, Vec<Part>>::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_READ")? {
            let entry = entry.map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_READ")?;
            let kind = entry
                .file_type()
                .map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_METADATA")?;
            if kind.is_symlink() {
                return Err("ASTRA_EMU_MINORI_INVENTORY_SYMLINK".into());
            }
            if kind.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            scanned = scanned
                .checked_add(1)
                .ok_or("ASTRA_EMU_MINORI_INVENTORY_FILE_LIMIT")?;
            if scanned > MAX_SCANNED_FILES {
                return Err("ASTRA_EMU_MINORI_INVENTORY_FILE_LIMIT".into());
            }
            let Some((role, part)) = parse_archive_name(&entry.file_name())? else {
                continue;
            };
            let byte_size = entry
                .metadata()
                .map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_METADATA")?
                .len();
            if byte_size == 0 {
                return Err("ASTRA_EMU_MINORI_INVENTORY_EMPTY".into());
            }
            let parts = roles.entry(role).or_default();
            if parts.iter().any(|existing| existing.index == part) {
                return Err("ASTRA_EMU_MINORI_INVENTORY_DUPLICATE_PART".into());
            }
            parts.push(Part {
                index: part,
                byte_size,
            });
        }
    }
    if roles.is_empty() {
        return Err("ASTRA_EMU_MINORI_INVENTORY_MISSING".into());
    }

    let required = REQUIRED_ARCHIVE_ROLES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let required_role_set_match = roles.keys().cloned().collect::<BTreeSet<_>>() == required;
    let mut physical_file_count = 0u64;
    let mut physical_byte_count = 0u64;
    let mut hash_material = Vec::new();
    let mut archives = Vec::with_capacity(roles.len());
    for (role, mut parts) in roles {
        parts.sort_unstable_by_key(|part| part.index);
        for (expected, part) in parts.iter().enumerate() {
            if usize::from(part.index) != expected {
                return Err("ASTRA_EMU_MINORI_INVENTORY_PART_SEQUENCE".into());
            }
            hash_material.extend_from_slice(role.as_bytes());
            hash_material.push(0);
            hash_material.push(part.index);
            hash_material.extend_from_slice(&part.byte_size.to_le_bytes());
        }
        let count =
            u64::try_from(parts.len()).map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_FILE_LIMIT")?;
        let bytes = parts.iter().try_fold(0u64, |total, part| {
            total
                .checked_add(part.byte_size)
                .ok_or("ASTRA_EMU_MINORI_INVENTORY_SIZE")
        })?;
        physical_file_count = physical_file_count
            .checked_add(count)
            .ok_or("ASTRA_EMU_MINORI_INVENTORY_FILE_LIMIT")?;
        physical_byte_count = physical_byte_count
            .checked_add(bytes)
            .ok_or("ASTRA_EMU_MINORI_INVENTORY_SIZE")?;
        archives.push(ArchiveInventoryRole {
            required: REQUIRED_ARCHIVE_ROLES.contains(&role.as_str()),
            role,
            physical_file_count: count,
            physical_byte_count: bytes,
            highest_part: parts
                .last()
                .filter(|part| part.index > 0)
                .map(|part| char::from(b'A' + part.index - 1)),
        });
    }

    Ok(ArchiveInventoryReport {
        schema: "astra.emu.minori.archive_inventory.v1",
        logical_archive_count: u64::try_from(archives.len())
            .map_err(|_| "ASTRA_EMU_MINORI_INVENTORY_FILE_LIMIT")?,
        physical_file_count,
        physical_byte_count,
        required_role_set_match,
        inventory_hash: Hash256::from_sha256(&hash_material),
        archives,
    })
}

fn parse_archive_name(
    name: &std::ffi::OsStr,
) -> Result<Option<(String, u8)>, Box<dyn std::error::Error>> {
    let name = name.to_str().ok_or("ASTRA_EMU_MINORI_INVENTORY_FILENAME")?;
    let lowercase = name.to_ascii_lowercase();
    let Some(marker) = lowercase.rfind(".paz") else {
        return Ok(None);
    };
    let role = &lowercase[..marker];
    let suffix = &name[marker + 4..];
    if role.is_empty()
        || role.len() > 64
        || !role
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("ASTRA_EMU_MINORI_INVENTORY_ROLE".into());
    }
    let part = match suffix.as_bytes() {
        [] => 0,
        [letter] if letter.is_ascii_alphabetic() => letter.to_ascii_uppercase() - b'A' + 1,
        _ => return Ok(None),
    };
    Ok(Some((role.to_owned(), part)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursively_counts_eight_logical_archives_and_eighteen_files() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("archives");
        fs::create_dir(&nested).unwrap();
        for role in REQUIRED_ARCHIVE_ROLES {
            fs::write(nested.join(format!("{role}.paz")), [1]).unwrap();
        }
        for letter in b'A'..=b'J' {
            fs::write(nested.join(format!("bg.paz{}", char::from(letter))), [1]).unwrap();
        }
        let report = scan_archive_inventory(root.path()).unwrap();
        assert_eq!(report.logical_archive_count, 8);
        assert_eq!(report.physical_file_count, 18);
        assert_eq!(report.physical_byte_count, 18);
        assert!(report.required_role_set_match);
        let bg = report
            .archives
            .iter()
            .find(|archive| archive.role == "bg")
            .unwrap();
        assert_eq!(bg.physical_file_count, 11);
        assert_eq!(bg.highest_part, Some('J'));
    }

    #[test]
    fn missing_part_and_duplicate_case_variant_are_blocking() {
        let missing = tempfile::tempdir().unwrap();
        fs::write(missing.path().join("bg.paz"), [1]).unwrap();
        fs::write(missing.path().join("bg.pazB"), [1]).unwrap();
        assert_eq!(
            scan_archive_inventory(missing.path())
                .unwrap_err()
                .to_string(),
            "ASTRA_EMU_MINORI_INVENTORY_PART_SEQUENCE"
        );

        let duplicate = tempfile::tempdir().unwrap();
        fs::write(duplicate.path().join("bg.paz"), [1]).unwrap();
        fs::create_dir(duplicate.path().join("nested")).unwrap();
        fs::write(duplicate.path().join("nested").join("BG.PAZ"), [1]).unwrap();
        assert_eq!(
            scan_archive_inventory(duplicate.path())
                .unwrap_err()
                .to_string(),
            "ASTRA_EMU_MINORI_INVENTORY_DUPLICATE_PART"
        );
    }
}
