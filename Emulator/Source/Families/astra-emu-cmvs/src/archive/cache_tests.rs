use super::*;

#[test]
fn cached_ranges_share_storage_and_survive_cache_release() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("archive.cpz");
    std::fs::write(&path, b"fixture source").unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let hash = Hash256::from_sha256(b"fixture");
    let bytes = OwnedByteBuffer::from(b"decoded bytes".to_vec());
    let pointer = bytes.as_ptr();
    let uri = "cmvs:/test.bin";
    // Prepopulate only the decoded-entry cache. This test exercises storage
    // ownership and source invalidation, not CPZ decryption or mounting.
    let archive = CmvsArchive {
        mount_id: "test".into(),
        prefix: "cmvs:/".into(),
        manifest: ArchiveManifest {
            schema: ARCHIVE_MANIFEST_SCHEMA.into(),
            family_id: CMVS_FAMILY_ID.into(),
            mount_id: "test".into(),
            prefix: "cmvs:/".into(),
            reader_id: CMVS_READER_ID.into(),
            reader_hash: hash,
            decrypt_provider_id: CMVS_DECRYPT_PROVIDER_ID.into(),
            private_profile_hash: hash,
            mount_profile_hash: hash,
            sources: vec![],
            entries: vec![],
        },
        archives: vec![ArchiveFile {
            role: "test".into(),
            path: path.clone(),
            byte_size: metadata.len(),
            source_stamp: source_stamp(&metadata).unwrap(),
            source_hash: hash,
            header: CmvsCpzHeader {
                version: CmvsCpzVersion::Cpz5,
                directory_count: 0,
                directory_entries_size: 0,
                file_entries_size: 0,
                index_offset: 0,
                index_size: 0,
                master_key: None,
                entry_key: None,
                index_key_size: None,
                cmvs_md5: None,
                index_md5: None,
                encrypted_entries: true,
                entry_name_offset: 0,
            },
        }],
        entries: BTreeMap::from([(
            uri.into(),
            MountedEntry {
                uri: uri.into(),
                entry_id: "test-0".into(),
                archive: 0,
                descriptor: Cpz5IndexEntry {
                    relative_path: "test.bin".into(),
                    offset: 0,
                    stored_size: bytes.len() as u64,
                    checksum: 0,
                    key: 0,
                },
            },
        )]),
        loose: BTreeMap::new(),
        scheme: CmvsSchemeProfile {
            version: 5,
            cpz5_secret: vec![0; 24],
            md5_variant: "A".into(),
            decoder_factor: 0,
            entry_init_key: 0,
            entry_sub_key: 0,
            entry_tail_key: 0,
            entry_key_pos: 0,
            index_seed: 0,
            index_addend: 0,
            index_subtrahend: 0,
            dir_key_addend: vec![0; 4],
        },
        private_profile_hash: hash,
        cache: None,
        ephemeral_entry: Mutex::new(Some(("test-0".into(), bytes))),
    };
    let first = archive.read_range(uri, 0, 7).unwrap();
    let second = archive.read_range(uri, 8, 5).unwrap();
    assert!(first.cache_hit && second.cache_hit);
    assert_eq!(first.bytes.as_ptr(), pointer);
    assert_eq!(second.bytes.as_ptr(), pointer.wrapping_add(8));
    assert!(!first.eof);
    assert!(second.eof);
    let mut stream = archive.open_stream(uri).unwrap();
    // Source changes still invalidate cached reads.
    std::fs::write(&path, b"changed").unwrap();
    assert_eq!(
        archive.read_range(uri, 0, 1).unwrap_err().code(),
        "ASTRA_EMU_CMVS_SOURCE_CHANGED"
    );
    archive.ephemeral_entry.lock().unwrap().take();
    drop(archive);
    assert_eq!(first.bytes.as_slice(), b"decoded");
    assert_eq!(second.bytes.as_slice(), b"bytes");
    let mut copied = Vec::new();
    stream.read_to_end(&mut copied).unwrap();
    assert_eq!(copied, b"decoded bytes");
}
