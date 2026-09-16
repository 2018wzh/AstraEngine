use super::*;

/// Decrypts one CPZ5 archive entry after its bounded source range has been
/// read.  The caller owns source mutation checks and never receives key
/// material through this API.
pub fn decrypt_cpz5_entry(
    header: &CmvsCpzHeader,
    entry: &Cpz5IndexEntry,
    encrypted: &mut [u8],
    profile: &CmvsSchemeProfile,
) -> Result<(), CoreError> {
    if header.version != CmvsCpzVersion::Cpz5 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_ENTRY_VERSION",
            "CPZ5 entry decoder received a different archive version",
        ));
    }
    if u64::try_from(encrypted.len()).ok() != Some(entry.stored_size) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_ENTRY_SIZE",
            "CPZ5 encrypted entry length does not match the index",
        ));
    }
    if !header.encrypted_entries {
        return Ok(());
    }
    let master_key = header.master_key.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_PROFILE",
            "CPZ5 header lacks its master key",
        )
    })?;
    let encrypted_cmvs_md5 = header.cmvs_md5.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_PROFILE",
            "CPZ5 header lacks its CMVS MD5 state",
        )
    })?;
    let cmvs_md5 = crate::cmvs_md5::compute(encrypted_cmvs_md5, &profile.md5_variant)?;
    let decoder = Cpz5Decoder::new(profile.clone(), cmvs_md5[3], master_key);
    let seed = (master_key ^ entry.key)
        .wrapping_add(header.directory_count)
        .wrapping_sub(profile.entry_sub_key)
        ^ header.entry_key.unwrap_or_default();
    decoder.decrypt_entry(encrypted, cmvs_md5, seed)
}

/// Decrypt and validate a CPZ5 directory/index table.
///
/// CPZ5 predates the per-archive keys introduced by CPZ7.  GARbro's reader
/// therefore uses the all-zero archive key for this version; treating a
/// sibling `start.ps3` as mandatory would reject valid CPZ5 update archives.
pub fn parse_cpz5_index(
    header: &CmvsCpzHeader,
    encrypted_index: &[u8],
    profile: &CmvsSchemeProfile,
) -> Result<Vec<Cpz5IndexEntry>, CoreError> {
    if header.version != CmvsCpzVersion::Cpz5 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_INDEX_VERSION",
            "CPZ5 index reader received a different archive version",
        ));
    }
    header.verify_index_identity(encrypted_index)?;
    let directory_bytes = checked_index_size(header.directory_entries_size)?;
    let file_bytes = checked_index_size(header.file_entries_size)?;
    let table_bytes = directory_bytes.checked_add(file_bytes).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_INDEX_RANGE",
            "CPZ5 index table size overflowed",
        )
    })?;
    if encrypted_index.len() != table_bytes {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_INDEX_SIZE",
            "CPZ5 index layout does not match its header",
        ));
    }
    let master_key = header.master_key.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_PROFILE",
            "CPZ5 header lacks its master key",
        )
    })?;
    let encrypted_cmvs_md5 = header.cmvs_md5.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_CPZ_PROFILE",
            "CPZ5 header lacks its CMVS MD5 state",
        )
    })?;
    let cmvs_md5 = crate::cmvs_md5::compute(encrypted_cmvs_md5, &profile.md5_variant)?;
    let scheme = Cpz5Scheme::from_profile(profile)?;
    let mut index = encrypted_index.to_vec();
    scheme.decrypt_index_stage_1(&mut index, master_key ^ 0x3795_b39a);

    let mut decoder = Cpz5Decoder::new(profile.clone(), master_key, cmvs_md5[1]);
    decoder.decode(&mut index[..directory_bytes], 0x3a);
    let mut key = index_key(cmvs_md5, master_key);
    scheme.decrypt_directory(&mut index[..directory_bytes], key, 0);
    decoder.initialize(master_key, cmvs_md5[2]);

    let base_offset = header
        .index_offset
        .checked_add(header.index_size)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_RANGE",
                "CPZ5 data offset overflowed",
            )
        })?;
    let mut directory_offset = 0usize;
    let mut entries = Vec::new();
    for directory_index in 0..header.directory_count {
        let directory_size = usize_from_u32(read_u32(&index, directory_offset, "directory size")?)?;
        if directory_size <= 0x10
            || directory_offset
                .checked_add(directory_size)
                .is_none_or(|end| end > directory_bytes)
        {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                "CPZ5 directory record is outside the directory table",
            ));
        }
        let file_count = usize_from_u32(read_u32(&index, directory_offset + 4, "file count")?)?;
        if file_count > 0xffff {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                "CPZ5 directory file count exceeds the format bound",
            ));
        }
        let entries_offset =
            usize_from_u32(read_u32(&index, directory_offset + 8, "entries offset")?)?;
        let directory_key = read_u32(&index, directory_offset + 0x0c, "directory key")?;
        let directory_name = decode_cp932_cstring(
            &index[directory_offset + 0x10..directory_offset + directory_size],
            "directory name",
        )?;
        let next_entries_offset = if directory_index + 1 == header.directory_count {
            file_bytes
        } else {
            let next_field = directory_offset
                .checked_add(directory_size)
                .and_then(|offset| offset.checked_add(8))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                        "CPZ5 directory offset overflowed",
                    )
                })?;
            usize_from_u32(read_u32(&index, next_field, "next entries offset")?)?
        };
        let entries_size = next_entries_offset
            .checked_sub(entries_offset)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                    "CPZ5 directory entry ranges overlap or reverse",
                )
            })?;
        if entries_size == 0 || next_entries_offset > file_bytes {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                "CPZ5 directory entry range is outside the file table",
            ));
        }
        let entries_start = directory_bytes.checked_add(entries_offset).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                "CPZ5 entry offset overflowed",
            )
        })?;
        let entries_end = entries_start
            .checked_add(entries_size)
            .filter(|end| *end <= index.len())
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
                    "CPZ5 entry range is outside the index",
                )
            })?;
        decoder.decode(&mut index[entries_start..entries_end], 0x7e);
        key = std::array::from_fn(|slot| {
            cmvs_md5[slot] ^ directory_key.wrapping_add(scheme.directory_key_addend[slot])
        });
        scheme.decrypt_entries(&mut index[entries_start..entries_end], key, 0);

        let mut entry_offset = entries_start;
        for _ in 0..file_count {
            let entry_size = usize_from_u32(read_u32(&index, entry_offset, "entry size")?)?;
            let minimum = usize::try_from(header.entry_name_offset).map_err(|_| {
                invalid("ASTRA_EMU_CMVS_CPZ_ENTRY", "CPZ5 name offset exceeds usize")
            })?;
            if entry_size <= minimum
                || entry_offset
                    .checked_add(entry_size)
                    .is_none_or(|end| end > entries_end)
            {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_CPZ_ENTRY",
                    "CPZ5 entry record is outside its directory table",
                ));
            }
            let name_start = entry_offset.checked_add(minimum).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_CPZ_ENTRY",
                    "CPZ5 entry name offset overflowed",
                )
            })?;
            let name =
                decode_cp932_cstring(&index[name_start..entry_offset + entry_size], "entry name")?;
            let relative_path = canonical_cmvs_path(&directory_name, &name)?;
            let stored_offset = read_u64(&index, entry_offset + 4, "entry offset")?;
            let stored_size = u64::from(read_u32(&index, entry_offset + 0x0c, "entry size")?);
            let checksum_offset = entry_offset + 0x10;
            let checksum = read_u32(&index, checksum_offset, "entry checksum")?;
            let key =
                read_u32(&index, checksum_offset + 4, "entry key")?.wrapping_add(directory_key);
            let offset = base_offset.checked_add(stored_offset).ok_or_else(|| {
                invalid("ASTRA_EMU_CMVS_CPZ_ENTRY", "CPZ5 entry offset overflowed")
            })?;
            entries.push(Cpz5IndexEntry {
                relative_path,
                offset,
                stored_size,
                checksum,
                key,
            });
            entry_offset += entry_size;
        }
        if entry_offset != entries_end {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_ENTRY",
                "CPZ5 directory entry count does not consume its table exactly",
            ));
        }
        directory_offset += directory_size;
    }
    if directory_offset != directory_bytes {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_DIRECTORY",
            "CPZ5 directory records do not consume the directory table exactly",
        ));
    }
    Ok(entries)
}
