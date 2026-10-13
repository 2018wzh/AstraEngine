use super::*;

#[test]
fn in_memory_reader_clones_share_immutable_container_storage() {
    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(SectionPayload::raw(
            "alpha",
            "schema.alpha",
            vec![0x5a; 1024 * 1024],
        ))
        .write()
        .unwrap();
    let reader = AstraContainerReader::new(blob.as_bytes()).unwrap();
    let clone = reader.clone();

    assert!(Arc::ptr_eq(&reader.bytes, &clone.bytes));
    assert_eq!(
        reader.read_section("alpha").unwrap(),
        vec![0x5a; 1024 * 1024]
    );
}

#[test]
fn reader_rejects_duplicate_ids_from_a_tampered_authority_table() {
    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(SectionPayload::raw("alpha", "schema.alpha", b"a".to_vec()))
        .add_section(SectionPayload::raw("bravo", "schema.bravo", b"b".to_vec()))
        .write()
        .unwrap();
    let mut bytes = blob.into_bytes();
    let table_len = u64::from_le_bytes(bytes[20..28].try_into().unwrap()) as usize;
    let mut entries: Vec<SectionEntry> =
        postcard::from_bytes(&bytes[HEADER_LEN..HEADER_LEN + table_len]).unwrap();
    entries[1].id = entries[0].id.clone();
    entries[1].schema = "schema.other".to_string();
    let table = postcard::to_allocvec(&entries).unwrap();
    assert_eq!(table.len(), table_len);
    bytes[HEADER_LEN..HEADER_LEN + table_len].copy_from_slice(&table);
    rewrite_footer(&mut bytes);

    let error = AstraContainerReader::new(&bytes).unwrap_err();
    assert!(error.to_string().contains("duplicate section id alpha"));
}

#[test]
fn reader_rejects_overlapping_section_ranges() {
    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(SectionPayload::raw(
            "alpha",
            "schema.alpha",
            b"aaaa".to_vec(),
        ))
        .add_section(SectionPayload::raw(
            "bravo",
            "schema.bravo",
            b"bbbb".to_vec(),
        ))
        .write()
        .unwrap();
    let mut bytes = blob.into_bytes();
    let table_len = u64::from_le_bytes(bytes[20..28].try_into().unwrap()) as usize;
    let mut entries: Vec<SectionEntry> =
        postcard::from_bytes(&bytes[HEADER_LEN..HEADER_LEN + table_len]).unwrap();
    entries[1].offset = entries[0].offset;
    entries[1].stored_hash = entries[0].stored_hash;
    entries[1].hash = entries[0].hash;
    let table = postcard::to_allocvec(&entries).unwrap();
    assert_eq!(table.len(), table_len);
    bytes[HEADER_LEN..HEADER_LEN + table_len].copy_from_slice(&table);
    rewrite_footer(&mut bytes);

    let error = AstraContainerReader::new(&bytes).unwrap_err();
    assert!(error.to_string().contains("overlaps section"));
}

fn rewrite_footer(bytes: &mut [u8]) {
    let footer_start = bytes.len() - FOOTER_LEN;
    let table_len = u64::from_le_bytes(bytes[20..28].try_into().unwrap()) as usize;
    let table = &bytes[HEADER_LEN..HEADER_LEN + table_len];
    let entries: Vec<SectionEntry> = postcard::from_bytes(table).unwrap();
    let table_hash = Hash256::from_sha256(table);
    let content_root = section_merkle_root(&entries).unwrap();
    bytes[footer_start + 64..footer_start + 96].copy_from_slice(table_hash.as_bytes());
    bytes[footer_start + 96..footer_start + 128].copy_from_slice(content_root.as_bytes());
}
