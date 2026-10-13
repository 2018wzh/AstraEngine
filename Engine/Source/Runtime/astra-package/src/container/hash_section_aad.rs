use super::*;

pub(super) fn hash_section_aad(aad: SectionAad<'_>) -> Result<Hash256, ContainerError> {
    let bytes = postcard::to_allocvec(&aad)
        .map_err(|err| ContainerError::PostcardEncode(err.to_string()))?;
    Ok(Hash256::from_sha256(&bytes))
}

pub(super) fn encode_payload(
    codec: &SectionCodec,
    payload: &[u8],
) -> Result<Vec<u8>, ContainerError> {
    match codec {
        SectionCodec::Postcard | SectionCodec::Raw => Ok(payload.to_vec()),
        SectionCodec::Zstd => {
            zstd::bulk::compress(payload, 3).map_err(|err| ContainerError::Zstd(err.to_string()))
        }
    }
}

pub(super) fn decode_payload(
    entry: &SectionEntry,
    stored: &[u8],
) -> Result<Vec<u8>, ContainerError> {
    let decoded_len = usize::try_from(entry.decoded_length)
        .map_err(|_| ContainerError::message("section decoded length exceeds address space"))?;
    let decoded = match entry.codec {
        SectionCodec::Postcard | SectionCodec::Raw => stored.to_vec(),
        SectionCodec::Zstd => zstd::bulk::decompress(stored, decoded_len)
            .map_err(|err| ContainerError::Zstd(err.to_string()))?,
    };
    if decoded.len() as u64 != entry.decoded_length {
        return Err(ContainerError::message(format!(
            "section {} decoded length mismatch",
            entry.id
        )));
    }
    if Hash256::from_sha256(&decoded) != entry.hash {
        return Err(ContainerError::message(format!(
            "section {} decoded hash mismatch",
            entry.id
        )));
    }
    Ok(decoded)
}

pub(super) fn validate_section_payloads(sections: &[SectionPayload]) -> Result<(), ContainerError> {
    if sections.len() > MAX_SECTION_COUNT {
        return Err(ContainerError::message(
            "container section count exceeds the supported bound",
        ));
    }
    let mut ids = BTreeSet::new();
    for section in sections {
        validate_section_descriptor(
            &section.id,
            &section.schema,
            section.payload.len() as u64,
            &section.migration,
        )?;
        if !ids.insert(section.id.as_str()) {
            return Err(ContainerError::message(format!(
                "duplicate section id {}",
                section.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_section_descriptor(
    id: &str,
    schema: &str,
    decoded_length: u64,
    migration: &MigrationPolicy,
) -> Result<(), ContainerError> {
    if !is_safe_section_symbol(id) {
        return Err(ContainerError::message(format!(
            "section id is empty or invalid: {id}"
        )));
    }
    if !is_safe_section_symbol(schema) {
        return Err(ContainerError::message(format!(
            "section schema is empty or invalid: {schema}"
        )));
    }
    if decoded_length > MAX_DECODED_SECTION_LEN {
        return Err(ContainerError::message(
            "section decoded length exceeds the supported bound",
        ));
    }
    if migration.minimum_supported_version > migration.current_version
        || migration.current_version.major == 0
    {
        return Err(ContainerError::message(
            "section migration policy is invalid for this container version",
        ));
    }
    Ok(())
}

pub(super) fn is_safe_section_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(super) fn align(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

pub(super) fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}
