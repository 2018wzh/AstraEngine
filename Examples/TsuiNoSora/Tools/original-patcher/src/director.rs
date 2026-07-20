use std::{collections::BTreeMap, fs, io::Cursor, path::Path};

use binrw::{BinReaderExt, BinWriterExt, Endian};
use serde::{Deserialize, Serialize};

use crate::error::{PatchError, PatchResult};

const CONTAINER_SIGNATURE: &[u8; 4] = b"XFIR";
const MOVIE_TYPE: &[u8; 4] = b"39VM";
const IMAP_TAG: &[u8; 4] = b"pami";
const MMAP_TAG: &[u8; 4] = b"pamm";
const CAST_TAG: &[u8; 4] = b"tSAC";
const CAST_TABLE_TAG: &[u8; 4] = b"*SAC";
const FREE_TAG: &[u8; 4] = b"eerf";
const EXIT_RESOURCE_ID: u32 = 88;
const DEBUG_RESOURCE_ID: u32 = 381;
const EXIT_MEMBER_INDEX: usize = 41;
const DEBUG_MEMBER_INDEX: usize = 62;
const ORIGINAL_EXIT_SCRIPT_ID: u32 = 9;
const DEBUG_SCRIPT_ID: u32 = 44;
const CAST_SCRIPT_ID_OFFSET: usize = 28;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectorPatchRecord {
    pub relative_path: String,
    pub exit_resource_id: u32,
    pub debug_resource_id: u32,
    pub exit_member_id: u32,
    pub debug_member_id: u32,
    pub old_script_id: u32,
    pub new_script_id: u32,
}

#[derive(Debug, Clone)]
struct Resource {
    id: u32,
    tag: [u8; 4],
    payload_offset: usize,
    payload_size: usize,
}

pub fn patch_exit_to_debug(path: &Path) -> PatchResult<DirectorPatchRecord> {
    let mut data = fs::read(path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_MENU_READ_FAILED",
            "read decompiled menu movie",
            error,
        )
    })?;
    let resources = parse_resources(&data)?;
    validate_cast_bindings(&data, &resources)?;
    let exit = require_resource(&resources, EXIT_RESOURCE_ID, CAST_TAG)?;
    let debug = require_resource(&resources, DEBUG_RESOURCE_ID, CAST_TAG)?;
    let exit_script = cast_script_id(&data, exit)?;
    let debug_script = cast_script_id(&data, debug)?;
    if exit_script != ORIGINAL_EXIT_SCRIPT_ID || debug_script != DEBUG_SCRIPT_ID {
        return Err(PatchError::validation(
            "TSUI_PATCH_SCRIPT_BINDING_MISMATCH",
            "menu script bindings do not match the supported 1999 edition",
        ));
    }
    write_cast_script_id(&mut data, exit, DEBUG_SCRIPT_ID)?;
    fs::write(path, &data).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_MENU_WRITE_FAILED",
            "write patched menu movie",
            error,
        )
    })?;
    verify_bytes(&data)?;
    Ok(patch_record())
}

pub fn verify_exit_to_debug(path: &Path) -> PatchResult<()> {
    let data = fs::read(path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_MENU_READ_FAILED",
            "read patched menu movie",
            error,
        )
    })?;
    verify_bytes(&data)
}

fn verify_bytes(data: &[u8]) -> PatchResult<()> {
    let resources = parse_resources(data)?;
    validate_cast_bindings(data, &resources)?;
    let exit = require_resource(&resources, EXIT_RESOURCE_ID, CAST_TAG)?;
    let debug = require_resource(&resources, DEBUG_RESOURCE_ID, CAST_TAG)?;
    if cast_script_id(data, exit)? != DEBUG_SCRIPT_ID
        || cast_script_id(data, debug)? != DEBUG_SCRIPT_ID
    {
        return Err(PatchError::validation(
            "TSUI_PATCH_SCRIPT_BINDING_NOT_APPLIED",
            "exit button is not bound to the original debug-menu behavior",
        ));
    }
    Ok(())
}

fn parse_resources(data: &[u8]) -> PatchResult<BTreeMap<u32, Resource>> {
    require_slice(data, 0, 12, "Director container header")?;
    if &data[0..4] != CONTAINER_SIGNATURE || &data[8..12] != MOVIE_TYPE {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_SIGNATURE_INVALID",
            "ProjectorRays output is not the expected Director 7 movie",
        ));
    }
    let declared_size = read_u32(data, 4, Endian::Little)? as usize;
    if declared_size.checked_add(8) != Some(data.len()) {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_SIZE_MISMATCH",
            "Director container declared size does not match the file",
        ));
    }
    require_slice(data, 12, 32, "Director imap")?;
    if &data[12..16] != IMAP_TAG || read_u32(data, 16, Endian::Little)? != 24 {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_IMAP_INVALID",
            "Director imap is missing or malformed",
        ));
    }
    let mmap_offset = read_u32(data, 24, Endian::Little)? as usize;
    require_slice(data, mmap_offset, 8, "Director mmap chunk")?;
    if &data[mmap_offset..mmap_offset + 4] != MMAP_TAG {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_MMAP_MISSING",
            "Director mmap was not found at the imap-provided offset",
        ));
    }
    let mmap_size = read_u32(data, mmap_offset + 4, Endian::Little)? as usize;
    let payload_offset = mmap_offset + 8;
    let mmap_end = payload_offset.checked_add(mmap_size).ok_or_else(|| {
        format_error(
            "TSUI_PATCH_DIRECTOR_INTEGER_OVERFLOW",
            "Director mmap size overflowed",
        )
    })?;
    require_slice(data, payload_offset, mmap_size, "Director mmap payload")?;
    let header_size = read_u16(data, payload_offset, Endian::Little)? as usize;
    let entry_size = read_u16(data, payload_offset + 2, Endian::Little)? as usize;
    let total_count = read_u32(data, payload_offset + 4, Endian::Little)?;
    let resource_count = read_u32(data, payload_offset + 8, Endian::Little)?;
    if header_size != 24 || entry_size != 20 || total_count != resource_count {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_MMAP_LAYOUT_UNSUPPORTED",
            "Director mmap layout is not the exact ProjectorRays contract",
        ));
    }
    let entries_offset = payload_offset + header_size;
    let entries_size = (resource_count as usize)
        .checked_mul(entry_size)
        .ok_or_else(|| {
            format_error(
                "TSUI_PATCH_DIRECTOR_INTEGER_OVERFLOW",
                "Director resource table size overflowed",
            )
        })?;
    if entries_offset
        .checked_add(entries_size)
        .is_none_or(|end| end > mmap_end)
    {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_MMAP_TRUNCATED",
            "Director resource entries extend beyond mmap",
        ));
    }

    let mut resources = BTreeMap::new();
    for id in 0..resource_count {
        let entry_offset = entries_offset + id as usize * entry_size;
        let tag: [u8; 4] = data[entry_offset..entry_offset + 4]
            .try_into()
            .expect("validated resource tag range");
        let chunk_size = read_u32(data, entry_offset + 4, Endian::Little)? as usize;
        let chunk_offset = read_u32(data, entry_offset + 8, Endian::Little)? as usize;
        let flags = read_u16(data, entry_offset + 12, Endian::Little)?;
        let unknown = read_u16(data, entry_offset + 14, Endian::Little)?;
        if tag == [0; 4] && chunk_size == 0 && chunk_offset == 0 {
            continue;
        }
        if tag == *FREE_TAG {
            if chunk_size == 0 && chunk_offset == 0 && flags == 12 && unknown == 0 {
                continue;
            }
            return Err(format_error(
                "TSUI_PATCH_DIRECTOR_FREE_ENTRY_INVALID",
                "Director free mmap entry contains active or malformed fields",
            ));
        }
        require_slice(data, chunk_offset, 8, "Director resource chunk")?;
        if data[chunk_offset..chunk_offset + 4] != tag
            || read_u32(data, chunk_offset + 4, Endian::Little)? as usize != chunk_size
        {
            return Err(format_error(
                "TSUI_PATCH_DIRECTOR_RESOURCE_MAP_MISMATCH",
                "Director mmap entry does not match its resource chunk",
            ));
        }
        let resource = Resource {
            id,
            tag,
            payload_offset: chunk_offset + 8,
            payload_size: chunk_size,
        };
        require_slice(
            data,
            resource.payload_offset,
            resource.payload_size,
            "Director resource payload",
        )?;
        if resources.insert(id, resource).is_some() {
            return Err(format_error(
                "TSUI_PATCH_DIRECTOR_RESOURCE_DUPLICATE",
                "Director resource id is duplicated",
            ));
        }
    }
    Ok(resources)
}

fn validate_cast_bindings(data: &[u8], resources: &BTreeMap<u32, Resource>) -> PatchResult<()> {
    let cast_tables: Vec<_> = resources
        .values()
        .filter(|resource| resource.tag == *CAST_TABLE_TAG)
        .collect();
    if cast_tables.len() != 1 {
        return Err(format_error(
            "TSUI_PATCH_CAST_TABLE_COUNT_INVALID",
            "menu movie must contain exactly one CAS* cast table",
        ));
    }
    let table = cast_tables[0];
    if table.payload_size % 4 != 0 {
        return Err(format_error(
            "TSUI_PATCH_CAST_TABLE_TRUNCATED",
            "CAS* cast table is not aligned to 32-bit entries",
        ));
    }
    let count = table.payload_size / 4;
    if EXIT_MEMBER_INDEX >= count || DEBUG_MEMBER_INDEX >= count {
        return Err(format_error(
            "TSUI_PATCH_CAST_TABLE_TRUNCATED",
            "CAS* cast table does not contain the required members",
        ));
    }
    let mut exit_positions = Vec::new();
    let mut debug_positions = Vec::new();
    for index in 0..count {
        let id = read_u32(data, table.payload_offset + index * 4, Endian::Big)?;
        if id == EXIT_RESOURCE_ID {
            exit_positions.push(index);
        }
        if id == DEBUG_RESOURCE_ID {
            debug_positions.push(index);
        }
    }
    if exit_positions != [EXIT_MEMBER_INDEX] || debug_positions != [DEBUG_MEMBER_INDEX] {
        return Err(format_error(
            "TSUI_PATCH_CAST_BINDING_MISMATCH",
            "CAS* does not uniquely bind the expected exit and debug cast members",
        ));
    }
    Ok(())
}

fn require_resource<'a>(
    resources: &'a BTreeMap<u32, Resource>,
    id: u32,
    tag: &[u8; 4],
) -> PatchResult<&'a Resource> {
    let resource = resources.get(&id).ok_or_else(|| {
        format_error(
            "TSUI_PATCH_DIRECTOR_RESOURCE_MISSING",
            "required Director resource is missing",
        )
    })?;
    if &resource.tag != tag || resource.id != id {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_RESOURCE_TYPE_MISMATCH",
            "required Director resource has the wrong type",
        ));
    }
    Ok(resource)
}

fn cast_script_id(data: &[u8], resource: &Resource) -> PatchResult<u32> {
    if resource.payload_size < CAST_SCRIPT_ID_OFFSET + 4
        || read_u32(data, resource.payload_offset, Endian::Big)? != 11
        || read_u32(data, resource.payload_offset + 8, Endian::Big)? != 2
        || read_u32(data, resource.payload_offset + 12, Endian::Big)? != 20
    {
        return Err(format_error(
            "TSUI_PATCH_CAST_SCRIPT_LAYOUT_INVALID",
            "CASt behavior resource layout is invalid",
        ));
    }
    read_u32(
        data,
        resource.payload_offset + CAST_SCRIPT_ID_OFFSET,
        Endian::Big,
    )
}

fn write_cast_script_id(data: &mut [u8], resource: &Resource, value: u32) -> PatchResult<()> {
    let offset = resource.payload_offset + CAST_SCRIPT_ID_OFFSET;
    let mut cursor = Cursor::new(&mut data[offset..offset + 4]);
    cursor.write_type(&value, Endian::Big).map_err(|_| {
        format_error(
            "TSUI_PATCH_CAST_SCRIPT_WRITE_FAILED",
            "failed to write CASt script binding",
        )
    })
}

fn read_u16(data: &[u8], offset: usize, endian: Endian) -> PatchResult<u16> {
    require_slice(data, offset, 2, "Director 16-bit field")?;
    Cursor::new(&data[offset..offset + 2])
        .read_type(endian)
        .map_err(|_| {
            format_error(
                "TSUI_PATCH_DIRECTOR_FIELD_INVALID",
                "invalid Director field",
            )
        })
}

fn read_u32(data: &[u8], offset: usize, endian: Endian) -> PatchResult<u32> {
    require_slice(data, offset, 4, "Director 32-bit field")?;
    Cursor::new(&data[offset..offset + 4])
        .read_type(endian)
        .map_err(|_| {
            format_error(
                "TSUI_PATCH_DIRECTOR_FIELD_INVALID",
                "invalid Director field",
            )
        })
}

fn require_slice(data: &[u8], offset: usize, size: usize, role: &'static str) -> PatchResult<()> {
    if offset.checked_add(size).is_none_or(|end| end > data.len()) {
        return Err(format_error(
            "TSUI_PATCH_DIRECTOR_TRUNCATED",
            format!("{role} is truncated"),
        ));
    }
    Ok(())
}

fn format_error(code: &'static str, message: impl Into<String>) -> PatchError {
    PatchError::validation(code, message)
}

fn patch_record() -> DirectorPatchRecord {
    DirectorPatchRecord {
        relative_path: "DATA/MENU.dxr".to_owned(),
        exit_resource_id: EXIT_RESOURCE_ID,
        debug_resource_id: DEBUG_RESOURCE_ID,
        exit_member_id: (EXIT_MEMBER_INDEX + 1) as u32,
        debug_member_id: (DEBUG_MEMBER_INDEX + 1) as u32,
        old_script_id: ORIGINAL_EXIT_SCRIPT_ID,
        new_script_id: DEBUG_SCRIPT_ID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(exit_script: u32, duplicate_exit: bool) -> Vec<u8> {
        let resource_count = 382_u32;
        let mmap_offset = 44_usize;
        let mmap_size = 24 + resource_count as usize * 20;
        let mut data = vec![0_u8; mmap_offset + 8 + mmap_size];
        data[0..4].copy_from_slice(CONTAINER_SIGNATURE);
        data[8..12].copy_from_slice(MOVIE_TYPE);
        data[12..16].copy_from_slice(IMAP_TAG);
        put_u32_le(&mut data, 16, 24);
        put_u32_le(&mut data, 24, mmap_offset as u32);
        data[mmap_offset..mmap_offset + 4].copy_from_slice(MMAP_TAG);
        put_u32_le(&mut data, mmap_offset + 4, mmap_size as u32);
        let payload = mmap_offset + 8;
        put_u16_le(&mut data, payload, 24);
        put_u16_le(&mut data, payload + 2, 20);
        put_u32_le(&mut data, payload + 4, resource_count);
        put_u32_le(&mut data, payload + 8, resource_count);
        let entries = payload + 24;

        let mut chunks = Vec::new();
        for id in 0..resource_count {
            let (tag, payload_bytes) = match id {
                88 => (*CAST_TAG, cast_payload(exit_script)),
                381 => (*CAST_TAG, cast_payload(DEBUG_SCRIPT_ID)),
                1 => {
                    let mut table = vec![0_u8; 63 * 4];
                    put_u32_be(&mut table, EXIT_MEMBER_INDEX * 4, EXIT_RESOURCE_ID);
                    put_u32_be(&mut table, DEBUG_MEMBER_INDEX * 4, DEBUG_RESOURCE_ID);
                    if duplicate_exit {
                        put_u32_be(&mut table, 0, EXIT_RESOURCE_ID);
                    }
                    (*CAST_TABLE_TAG, table)
                }
                _ => (*b"llun", Vec::new()),
            };
            let chunk_offset = data.len() + chunks.len();
            let entry = entries + id as usize * 20;
            data[entry..entry + 4].copy_from_slice(&tag);
            put_u32_le(&mut data, entry + 4, payload_bytes.len() as u32);
            put_u32_le(&mut data, entry + 8, chunk_offset as u32);
            chunks.extend_from_slice(&tag);
            chunks.extend_from_slice(&(payload_bytes.len() as u32).to_le_bytes());
            chunks.extend_from_slice(&payload_bytes);
        }
        data.extend_from_slice(&chunks);
        let declared = (data.len() - 8) as u32;
        put_u32_le(&mut data, 4, declared);
        data
    }

    fn cast_payload(script_id: u32) -> Vec<u8> {
        let mut payload = vec![0_u8; 40];
        put_u32_be(&mut payload, 0, 11);
        put_u32_be(&mut payload, 8, 2);
        put_u32_be(&mut payload, 12, 20);
        put_u32_be(&mut payload, CAST_SCRIPT_ID_OFFSET, script_id);
        payload
    }

    fn put_u16_le(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32_be(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn patches_only_exit_script_binding() {
        let mut data = fixture(ORIGINAL_EXIT_SCRIPT_ID, false);
        let resources = parse_resources(&data).expect("resources");
        validate_cast_bindings(&data, &resources).expect("bindings");
        let exit = require_resource(&resources, EXIT_RESOURCE_ID, CAST_TAG).expect("exit");
        write_cast_script_id(&mut data, exit, DEBUG_SCRIPT_ID).expect("write");
        verify_bytes(&data).expect("verified patch");
    }

    #[test]
    fn rejects_truncated_mmap() {
        let mut data = fixture(ORIGINAL_EXIT_SCRIPT_ID, false);
        data.truncate(100);
        let error = parse_resources(&data).expect_err("truncation must fail");
        assert!(error.code.starts_with("TSUI_PATCH_DIRECTOR_"));
    }

    #[test]
    fn rejects_duplicate_cast_binding() {
        let data = fixture(ORIGINAL_EXIT_SCRIPT_ID, true);
        let resources = parse_resources(&data).expect("resources");
        let error = validate_cast_bindings(&data, &resources).expect_err("duplicate must fail");
        assert_eq!(error.code, "TSUI_PATCH_CAST_BINDING_MISMATCH");
    }

    #[test]
    fn accepts_zeroed_free_mmap_entries() {
        let mut data = fixture(ORIGINAL_EXIT_SCRIPT_ID, false);
        let mmap_offset = 44_usize;
        let entries = mmap_offset + 8 + 24;
        let free_entry = entries + 2 * 20;
        data[free_entry..free_entry + 20].fill(0);
        let resources = parse_resources(&data).expect("resources");
        assert!(!resources.contains_key(&2));
        validate_cast_bindings(&data, &resources).expect("bindings");
    }

    #[test]
    fn accepts_linked_free_mmap_entries_only_with_exact_flags() {
        let mut data = fixture(ORIGINAL_EXIT_SCRIPT_ID, false);
        let mmap_offset = 44_usize;
        let entries = mmap_offset + 8 + 24;
        let free_entry = entries + 2 * 20;
        data[free_entry..free_entry + 4].copy_from_slice(FREE_TAG);
        data[free_entry + 4..free_entry + 12].fill(0);
        put_u16_le(&mut data, free_entry + 12, 12);
        put_u16_le(&mut data, free_entry + 14, 0);
        put_u32_le(&mut data, free_entry + 16, u32::MAX);
        let resources = parse_resources(&data).expect("resources");
        assert!(!resources.contains_key(&2));

        put_u16_le(&mut data, free_entry + 12, 0);
        let error = parse_resources(&data).expect_err("malformed free entry");
        assert_eq!(error.code, "TSUI_PATCH_DIRECTOR_FREE_ENTRY_INVALID");
    }

    #[test]
    fn rejects_little_endian_cast_script_id() {
        let mut data = fixture(ORIGINAL_EXIT_SCRIPT_ID, false);
        let resources = parse_resources(&data).expect("resources");
        let exit = require_resource(&resources, EXIT_RESOURCE_ID, CAST_TAG).expect("exit");
        let offset = exit.payload_offset + CAST_SCRIPT_ID_OFFSET;
        data[offset..offset + 4].copy_from_slice(&ORIGINAL_EXIT_SCRIPT_ID.to_le_bytes());
        assert_ne!(
            cast_script_id(&data, exit).expect("script id"),
            ORIGINAL_EXIT_SCRIPT_ID
        );
    }
}
