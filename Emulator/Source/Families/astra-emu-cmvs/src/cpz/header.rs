use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CmvsCpzVersion {
    Cpz1,
    Cpz2,
    Cpz5,
    Cpz6,
    Cpz7,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmvsCpzHeader {
    pub version: CmvsCpzVersion,
    pub directory_count: u32,
    pub directory_entries_size: u64,
    pub file_entries_size: u64,
    pub index_offset: u64,
    pub index_size: u64,
    pub(crate) master_key: Option<u32>,
    pub(crate) entry_key: Option<u32>,
    pub(crate) index_key_size: Option<u32>,
    pub(crate) cmvs_md5: Option<[u32; 4]>,
    pub(crate) index_md5: Option<[u8; 16]>,
    pub encrypted_entries: bool,
    pub entry_name_offset: u32,
}

impl CmvsCpzHeader {
    #[must_use]
    pub fn requires_private_profile(&self) -> bool {
        self.master_key.is_some() || self.entry_key.is_some() || self.index_key_size.is_some()
    }

    pub fn verify_index_identity(&self, index: &[u8]) -> Result<(), CoreError> {
        if u64::try_from(index.len()).ok() != Some(self.index_size) {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_SIZE",
                "CPZ index byte count does not match its header",
            ));
        }
        let expected = self.index_md5.ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_HASH",
                "CPZ version does not carry an index identity hash",
            )
        })?;
        if Md5::digest(index).as_slice() != expected {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_INDEX_HASH",
                "CPZ index identity hash does not match",
            ));
        }
        Ok(())
    }
}

pub fn parse_cpz_header(source: &[u8], source_len: u64) -> Result<CmvsCpzHeader, CoreError> {
    let signature = source
        .get(..4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_CPZ_HEADER", "CPZ header is truncated"))?;
    match signature {
        b"CPZ1" => parse_legacy(source, source_len, CmvsCpzVersion::Cpz1),
        b"CPZ2" => parse_legacy(source, source_len, CmvsCpzVersion::Cpz2),
        b"CPZ5" | b"CPZ6" | b"CPZ7" => parse_modern(source, source_len),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_MAGIC",
            "CPZ archive magic is unsupported",
        )),
    }
}

pub(super) fn parse_legacy(
    source: &[u8],
    source_len: u64,
    version: CmvsCpzVersion,
) -> Result<CmvsCpzHeader, CoreError> {
    let (directory_count, index_size, index_offset): (u32, u32, u64) = match version {
        CmvsCpzVersion::Cpz1 => (le_u32(source, 4)?, le_u32(source, 8)?, 0x10),
        CmvsCpzVersion::Cpz2 => (
            le_u32(source, 4)? ^ 0xe47c_59f3,
            le_u32(source, 8)? ^ 0x3f71_de2a,
            0x14,
        ),
        _ => unreachable!("legacy parser only receives CPZ1/CPZ2"),
    };
    let index_end = index_offset
        .checked_add(u64::from(index_size))
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_CPZ_RANGE", "CPZ index range overflowed"))?;
    if directory_count == 0 || u64::from(index_size) > MAX_INDEX_BYTES || index_end > source_len {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_HEADER",
            "CPZ legacy header has invalid directory or index bounds",
        ));
    }
    let entry_key = if version == CmvsCpzVersion::Cpz2 {
        Some(le_u32(source, 0x10)? ^ 0x40de_832c)
    } else {
        None
    };
    Ok(CmvsCpzHeader {
        version,
        directory_count,
        directory_entries_size: u64::from(index_size),
        file_entries_size: 0,
        index_offset,
        index_size: u64::from(index_size),
        master_key: None,
        entry_key,
        index_key_size: None,
        cmvs_md5: None,
        index_md5: None,
        encrypted_entries: true,
        entry_name_offset: 0x18,
    })
}

pub(super) fn parse_modern(source: &[u8], source_len: u64) -> Result<CmvsCpzHeader, CoreError> {
    let version = match source[3] {
        b'5' => CmvsCpzVersion::Cpz5,
        b'6' => CmvsCpzVersion::Cpz6,
        b'7' => CmvsCpzVersion::Cpz7,
        _ => {
            return Err(invalid(
                "ASTRA_EMU_CMVS_CPZ_VERSION",
                "CPZ version is unsupported",
            ))
        }
    };
    let header_bytes = if version == CmvsCpzVersion::Cpz7 {
        CPZ7_HEADER_BYTES
    } else {
        CPZ5_HEADER_BYTES
    };
    if source.len() < header_bytes {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_HEADER",
            "CPZ header is truncated",
        ));
    }
    let checksum_length = if version == CmvsCpzVersion::Cpz7 {
        0x40
    } else {
        0x3c
    };
    let stored_checksum = le_u32(
        source,
        if version == CmvsCpzVersion::Cpz7 {
            0x44
        } else {
            0x3c
        },
    )?;
    let init_checksum = if version == CmvsCpzVersion::Cpz7 {
        le_u32(source, 0x40)?.wrapping_sub(0x6dc5_a9b4)
    } else {
        CPZ5_INIT_CHECKSUM
    };
    if additive_checksum(&source[..checksum_length], init_checksum) != stored_checksum {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_CHECKSUM",
            "CPZ header checksum does not match",
        ));
    }
    let index_md5: [u8; 16] = source[0x10..0x20]
        .try_into()
        .map_err(|_| invalid("ASTRA_EMU_CMVS_CPZ_HEADER", "CPZ header is truncated"))?;
    let cmvs_constants = if version == CmvsCpzVersion::Cpz5 {
        [0x43de_7c19, 0xcc65_f415, 0xd016_a93c, 0x97a3_ba9a]
    } else {
        [0x43de_7c1a, 0xcc65_f416, 0xd016_a93d, 0x97a3_ba9b]
    };
    let cmvs_md5 = [
        le_u32(source, 0x20)? ^ cmvs_constants[0],
        le_u32(source, 0x24)? ^ cmvs_constants[1],
        le_u32(source, 0x28)? ^ cmvs_constants[2],
        le_u32(source, 0x2c)? ^ cmvs_constants[3],
    ];
    let (
        directory_count,
        directory_entries_size,
        file_entries_size,
        master_key,
        encrypted_entries,
        entry_key,
    ) = if version == CmvsCpzVersion::Cpz5 {
        (
            (le_u32(source, 4)? as i32 ^ -0x01c5_ac27) as u32,
            (le_u32(source, 8)? as i32 ^ 0x37f2_98e7) as u32,
            (le_u32(source, 0x0c)? as i32 ^ 0x7a6f_3a2c) as u32,
            le_u32(source, 0x30)? ^ 0xae7d_39bf,
            (le_u32(source, 0x34)? ^ 0xfb73_a955) != 0,
            0,
        )
    } else {
        let raw_entry_key = le_u32(source, 0x38)? ^ 0x37ac_f832;
        (
            (le_u32(source, 4)? as i32 ^ -0x01c5_ac26) as u32,
            (le_u32(source, 8)? as i32 ^ 0x37f2_98e8) as u32,
            (le_u32(source, 0x0c)? as i32 ^ 0x7a6f_3a2d) as u32,
            le_u32(source, 0x30)? ^ 0xae7d_39b7,
            (le_u32(source, 0x34)? ^ 0xfb73_a956) != 0,
            0x7da8_f173u32
                .wrapping_mul(raw_entry_key.rotate_right(5))
                .wrapping_add(0x1371_2765),
        )
    };
    let index_key_size = if version == CmvsCpzVersion::Cpz7 {
        let raw = le_u32(source, 0x40)?;
        Some(raw ^ 0x65ef_99f3)
    } else {
        None
    };
    let index_size = u64::from(directory_entries_size)
        .checked_add(u64::from(file_entries_size))
        .and_then(|size| size.checked_add(index_key_size.map(u64::from).unwrap_or(0)))
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_CPZ_RANGE", "CPZ index size overflowed"))?;
    let index_offset = header_bytes as u64;
    if directory_count == 0
        || index_size == 0
        || index_size > MAX_INDEX_BYTES
        || index_offset
            .checked_add(index_size)
            .is_none_or(|end| end > source_len)
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CPZ_HEADER",
            "CPZ header has invalid index bounds",
        ));
    }
    Ok(CmvsCpzHeader {
        version,
        directory_count,
        directory_entries_size: u64::from(directory_entries_size),
        file_entries_size: u64::from(file_entries_size),
        index_offset,
        index_size,
        master_key: Some(master_key),
        entry_key: Some(entry_key),
        index_key_size,
        cmvs_md5: Some(cmvs_md5),
        index_md5: Some(index_md5),
        encrypted_entries,
        entry_name_offset: if version == CmvsCpzVersion::Cpz7 {
            0x1c
        } else {
            0x18
        },
    })
}
