use super::*;

pub(super) fn read_container(bytes: &[u8]) -> Result<AstraContainerReader, ContainerError> {
    if bytes.len() < HEADER_LEN + FOOTER_LEN || &bytes[..8] != MAGIC {
        return Err(ContainerError::message("invalid Astra container magic"));
    }
    let version = SchemaVersion {
        major: u16::from_le_bytes(bytes[8..10].try_into().expect("header major")),
        minor: u16::from_le_bytes(bytes[10..12].try_into().expect("header minor")),
        patch: u16::from_le_bytes(bytes[12..14].try_into().expect("header patch")),
    };
    if version != CURRENT_CONTAINER_VERSION {
        return Err(ContainerError::message(if version.major == 1 {
            "ASTRA_CONTAINER_V1_MIGRATION_REQUIRED"
        } else {
            "unsupported Astra container version"
        }));
    }
    let kind = ContainerKind::from_tag(bytes[14])?;
    let section_count =
        u32::from_le_bytes(bytes[16..20].try_into().expect("section count")) as usize;
    let table_len = u64::from_le_bytes(bytes[20..28].try_into().expect("table len")) as usize;
    if section_count == 0 || section_count > MAX_SECTION_COUNT {
        return Err(ContainerError::message(
            "container section count is outside the supported bound",
        ));
    }
    if table_len == 0 || table_len > MAX_TABLE_LEN {
        return Err(ContainerError::message(
            "container section table exceeds the supported bound",
        ));
    }
    let alignment = u32::from_le_bytes(bytes[28..32].try_into().expect("alignment"));
    if alignment != ALIGNMENT {
        return Err(ContainerError::message(
            "unsupported Astra container alignment",
        ));
    }
    let footer_start = bytes.len() - FOOTER_LEN;
    let footer = &bytes[footer_start..];
    if &footer[..8] != FOOTER_MAGIC {
        return Err(ContainerError::message(
            "invalid Astra container v2 footer magic",
        ));
    }
    let footer_file_len = u64::from_le_bytes(footer[8..16].try_into().expect("footer file len"));
    if footer_file_len != bytes.len() as u64 {
        return Err(ContainerError::message(
            "container footer file length mismatch",
        ));
    }
    if footer[16] != kind.tag() {
        return Err(ContainerError::message("container footer kind mismatch"));
    }
    let footer_version = SchemaVersion {
        major: u16::from_le_bytes(footer[24..26].try_into().expect("footer major")),
        minor: u16::from_le_bytes(footer[26..28].try_into().expect("footer minor")),
        patch: u16::from_le_bytes(footer[28..30].try_into().expect("footer patch")),
    };
    if footer_version != version {
        return Err(ContainerError::message("container footer version mismatch"));
    }
    let footer_header_hash = Hash256::from_bytes(footer[32..64].try_into().expect("header hash"));
    if footer_header_hash != Hash256::from_sha256(&bytes[..HEADER_LEN]) {
        return Err(ContainerError::message("container header hash mismatch"));
    }
    let table_end = HEADER_LEN
        .checked_add(table_len)
        .ok_or_else(|| ContainerError::message("section table overflow"))?;
    if table_end > footer_start {
        return Err(ContainerError::message("section table out of bounds"));
    }
    let entries: Vec<SectionEntry> =
        postcard::from_bytes(&bytes[HEADER_LEN..table_end]).map_err(|err| {
            ContainerError::PostcardDecode(format!(
                "{}; table_len={table_len}; prefix={}",
                err,
                hex_prefix(&bytes[HEADER_LEN..table_end])
            ))
        })?;
    let footer_table_hash = Hash256::from_bytes(footer[64..96].try_into().expect("table hash"));
    if footer_table_hash != Hash256::from_sha256(&bytes[HEADER_LEN..table_end]) {
        return Err(ContainerError::message(
            "container section table hash mismatch",
        ));
    }
    if entries.len() != section_count {
        return Err(ContainerError::message(
            "section count does not match table",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut ranges = Vec::with_capacity(entries.len());
    for entry in &entries {
        validate_section_descriptor(
            &entry.id,
            &entry.schema,
            entry.decoded_length,
            &entry.migration,
        )?;
        if !ids.insert(entry.id.as_str()) {
            return Err(ContainerError::message(format!(
                "duplicate section id {}",
                entry.id
            )));
        }
        if entry.offset % ALIGNMENT as u64 != 0 {
            return Err(ContainerError::message(format!(
                "section {} is not aligned",
                entry.id
            )));
        }
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| ContainerError::message("section length overflow"))?;
        if end > footer_start {
            return Err(ContainerError::message(format!(
                "section {} out of bounds",
                entry.id
            )));
        }
        if start < table_end {
            return Err(ContainerError::message(format!(
                "section {} overlaps the container header or table",
                entry.id
            )));
        }
        ranges.push((start, end, entry.id.as_str()));
        let stored = &bytes[start..end];
        if Hash256::from_sha256(stored) != entry.stored_hash {
            return Err(ContainerError::message(format!(
                "section {} stored hash mismatch",
                entry.id
            )));
        }
        if let Some(descriptor) = &entry.encryption {
            let expected_aad = section_aad_hash_from_entry(kind, entry)?;
            if descriptor.aad_hash != expected_aad {
                return Err(ContainerError::Crypto(format!(
                    "section {} encryption AAD hash mismatch",
                    entry.id
                )));
            }
        }
        if entry.encryption.is_none() {
            let decoded = decode_payload(entry, stored)?;
            if Hash256::from_sha256(&decoded) != entry.hash {
                return Err(ContainerError::message(format!(
                    "section {} decoded hash mismatch",
                    entry.id
                )));
            }
        }
    }
    ranges.sort_unstable_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(ContainerError::message(format!(
                "section {} overlaps section {}",
                pair[0].2, pair[1].2
            )));
        }
    }
    let content_root = section_merkle_root(&entries)?;
    let footer_content_root =
        Hash256::from_bytes(footer[96..128].try_into().expect("content root"));
    if footer_content_root != content_root {
        return Err(ContainerError::message("container content root mismatch"));
    }
    Ok(AstraContainerReader {
        kind,
        entries,
        bytes: Arc::from(bytes),
        source: None,
        content_root,
        crypto: None,
    })
}

pub(super) fn read_container_source(
    source: Arc<dyn BoundedByteSource>,
    expected_content_root: Hash256,
) -> Result<AstraContainerReader, ContainerError> {
    let stat = source
        .stat()
        .map_err(|error| ContainerError::message(error.to_string()))?;
    if stat.len < (HEADER_LEN + FOOTER_LEN) as u64 {
        return Err(ContainerError::message("invalid Astra container length"));
    }
    let header = read_source_range(
        source.as_ref(),
        stat,
        ByteRange {
            offset: 0,
            len: HEADER_LEN as u64,
        },
    )?;
    if &header[..8] != MAGIC {
        return Err(ContainerError::message("invalid Astra container magic"));
    }
    let version = SchemaVersion {
        major: u16::from_le_bytes(header[8..10].try_into().expect("header major")),
        minor: u16::from_le_bytes(header[10..12].try_into().expect("header minor")),
        patch: u16::from_le_bytes(header[12..14].try_into().expect("header patch")),
    };
    if version != CURRENT_CONTAINER_VERSION {
        return Err(ContainerError::message(if version.major == 1 {
            "ASTRA_CONTAINER_V1_MIGRATION_REQUIRED"
        } else {
            "unsupported Astra container version"
        }));
    }
    let kind = ContainerKind::from_tag(header[14])?;
    let section_count =
        u32::from_le_bytes(header[16..20].try_into().expect("section count")) as usize;
    let table_len = u64::from_le_bytes(header[20..28].try_into().expect("table len"));
    if section_count == 0 || section_count > MAX_SECTION_COUNT {
        return Err(ContainerError::message(
            "container section count is outside the supported bound",
        ));
    }
    if table_len == 0 || table_len > MAX_TABLE_LEN as u64 {
        return Err(ContainerError::message(
            "container section table exceeds the supported bound",
        ));
    }
    if u32::from_le_bytes(header[28..32].try_into().expect("alignment")) != ALIGNMENT {
        return Err(ContainerError::message(
            "unsupported Astra container alignment",
        ));
    }
    let footer_offset = stat.len - FOOTER_LEN as u64;
    let footer = read_source_range(
        source.as_ref(),
        stat,
        ByteRange {
            offset: footer_offset,
            len: FOOTER_LEN as u64,
        },
    )?;
    if &footer[..8] != FOOTER_MAGIC
        || u64::from_le_bytes(footer[8..16].try_into().expect("footer len")) != stat.len
        || footer[16] != kind.tag()
    {
        return Err(ContainerError::message(
            "container footer identity mismatch",
        ));
    }
    let footer_version = SchemaVersion {
        major: u16::from_le_bytes(footer[24..26].try_into().expect("footer major")),
        minor: u16::from_le_bytes(footer[26..28].try_into().expect("footer minor")),
        patch: u16::from_le_bytes(footer[28..30].try_into().expect("footer patch")),
    };
    if footer_version != version
        || Hash256::from_bytes(footer[32..64].try_into().expect("header hash"))
            != Hash256::from_sha256(&header)
    {
        return Err(ContainerError::message(
            "container footer header binding mismatch",
        ));
    }
    let table_end = (HEADER_LEN as u64)
        .checked_add(table_len)
        .ok_or_else(|| ContainerError::message("section table overflow"))?;
    if table_end > footer_offset {
        return Err(ContainerError::message("section table out of bounds"));
    }
    let table = read_source_range(
        source.as_ref(),
        stat,
        ByteRange {
            offset: HEADER_LEN as u64,
            len: table_len,
        },
    )?;
    if Hash256::from_bytes(footer[64..96].try_into().expect("table hash"))
        != Hash256::from_sha256(&table)
    {
        return Err(ContainerError::message(
            "container section table hash mismatch",
        ));
    }
    let entries: Vec<SectionEntry> = postcard::from_bytes(&table)
        .map_err(|error| ContainerError::PostcardDecode(error.to_string()))?;
    if entries.len() != section_count {
        return Err(ContainerError::message(
            "section count does not match table",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut ranges = Vec::with_capacity(entries.len());
    for entry in &entries {
        validate_section_descriptor(
            &entry.id,
            &entry.schema,
            entry.decoded_length,
            &entry.migration,
        )?;
        if !ids.insert(entry.id.as_str()) {
            return Err(ContainerError::message(format!(
                "duplicate section id {}",
                entry.id
            )));
        }
        let end = entry
            .offset
            .checked_add(entry.length)
            .ok_or_else(|| ContainerError::message("section length overflow"))?;
        if entry.offset % ALIGNMENT as u64 != 0 || entry.offset < table_end || end > footer_offset {
            return Err(ContainerError::message(format!(
                "section {} range is invalid",
                entry.id
            )));
        }
        if let Some(descriptor) = &entry.encryption {
            if descriptor.aad_hash != section_aad_hash_from_entry(kind, entry)? {
                return Err(ContainerError::Crypto(format!(
                    "section {} encryption AAD hash mismatch",
                    entry.id
                )));
            }
        }
        ranges.push((entry.offset, end, entry.id.as_str()));
    }
    ranges.sort_unstable_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(ContainerError::message(format!(
                "section {} overlaps section {}",
                pair[0].2, pair[1].2
            )));
        }
    }
    let content_root = section_merkle_root(&entries)?;
    let footer_root = Hash256::from_bytes(footer[96..128].try_into().expect("content root"));
    if footer_root != content_root || content_root != expected_content_root {
        return Err(ContainerError::message(
            "container content root does not match launch identity",
        ));
    }
    Ok(AstraContainerReader {
        kind,
        entries,
        bytes: Arc::from([]),
        source: Some((source, stat)),
        content_root,
        crypto: None,
    })
}

pub(super) fn read_source_range(
    source: &dyn BoundedByteSource,
    stat: ByteSourceStat,
    range: ByteRange,
) -> Result<Vec<u8>, ContainerError> {
    let end = range
        .offset
        .checked_add(range.len)
        .ok_or_else(|| ContainerError::message("byte source range overflow"))?;
    if end > stat.len {
        return Err(ContainerError::message("byte source range out of bounds"));
    }
    let capacity = usize::try_from(range.len)
        .map_err(|_| ContainerError::message("byte source range cannot fit in memory"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut offset = range.offset;
    while offset < end {
        let len = (end - offset).min(DEFAULT_MAX_RANGE_BYTES);
        let result = source
            .read_range(
                stat.revision,
                ByteRange { offset, len },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .map_err(|error| ContainerError::message(error.to_string()))?;
        bytes.extend_from_slice(result.bytes.as_slice());
        offset = offset
            .checked_add(len)
            .ok_or_else(|| ContainerError::message("byte source range overflow"))?;
    }
    Ok(bytes)
}

pub(super) fn section_merkle_root(entries: &[SectionEntry]) -> Result<Hash256, ContainerError> {
    let mut level = entries
        .iter()
        .map(|entry| {
            postcard::to_allocvec(entry)
                .map(|bytes| Hash256::from_sha256(&bytes))
                .map_err(|error| ContainerError::PostcardEncode(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if level.is_empty() {
        return Err(ContainerError::message(
            "container Merkle tree has no leaves",
        ));
    }
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let right = pair.get(1).copied().unwrap_or(pair[0]);
            let mut material = [0_u8; 64];
            material[..32].copy_from_slice(pair[0].as_bytes());
            material[32..].copy_from_slice(right.as_bytes());
            next.push(Hash256::from_sha256(&material));
        }
        level = next;
    }
    Ok(level[0])
}

#[derive(Serialize)]
pub(super) struct SectionAad<'a> {
    pub(super) kind: ContainerKind,
    pub(super) id: &'a str,
    pub(super) schema: &'a str,
    pub(super) version: SchemaVersion,
    pub(super) codec: &'a SectionCodec,
    pub(super) decoded_length: u64,
    pub(super) decoded_hash: Hash256,
    pub(super) migration: &'a MigrationPolicy,
}

pub fn section_aad_hash(
    kind: ContainerKind,
    section: &SectionPayload,
) -> Result<Hash256, ContainerError> {
    section_aad_hash_from_section(kind, section, Hash256::from_sha256(&section.payload))
}

pub(super) fn section_aad_hash_from_section(
    kind: ContainerKind,
    section: &SectionPayload,
    decoded_hash: Hash256,
) -> Result<Hash256, ContainerError> {
    hash_section_aad(SectionAad {
        kind,
        id: &section.id,
        schema: &section.schema,
        version: section.version,
        codec: &section.codec,
        decoded_length: section.payload.len() as u64,
        decoded_hash,
        migration: &section.migration,
    })
}

pub(super) fn section_aad_hash_from_entry(
    kind: ContainerKind,
    entry: &SectionEntry,
) -> Result<Hash256, ContainerError> {
    hash_section_aad(SectionAad {
        kind,
        id: &entry.id,
        schema: &entry.schema,
        version: entry.version,
        codec: &entry.codec,
        decoded_length: entry.decoded_length,
        decoded_hash: entry.hash,
        migration: &entry.migration,
    })
}
