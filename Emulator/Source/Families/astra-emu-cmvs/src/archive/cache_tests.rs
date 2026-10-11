use super::*;

pub(super) fn fixture() -> (tempfile::TempDir, CmvsArchive) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("archive.cpz");
    std::fs::write(&path, b"fixture source").unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let hash = Hash256::from_sha256(b"fixture");
    let bytes = OwnedByteBuffer::from(b"decoded bytes".to_vec());
    let uri = "cmvs:/test.bin";
    // Prepopulate only the decoded-entry cache. This test exercises storage
    // ownership and source invalidation, not CPZ decryption or mounting.
    let archive = CmvsArchive {
        game_root: None,
        mount_id: "test".into(),
        prefix: "cmvs:/".into(),
        mount_profile_hash: hash,
        archives: vec![ArchiveFile {
            role: "test".into(),
            path: path.clone(),
            byte_size: metadata.len(),
            source_stamp: source_stamp(&metadata).unwrap(),
            source_hash: OnceLock::new(),
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
                ordinal: 0,
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
    (root, archive)
}

#[test]
fn cached_ranges_share_storage_and_survive_cache_release() {
    let (_root, archive) = fixture();
    let path = archive.archives[0].path.clone();
    let uri = "cmvs:/test.bin";
    let pointer = archive
        .ephemeral_entry
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .as_ptr();
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

#[test]
fn playback_defers_content_identity_until_manifest_is_requested() {
    let (_root, archive) = fixture();
    assert!(archive.archives[0].source_hash.get().is_none());
    archive.read_range("cmvs:/test.bin", 0, 3).unwrap();
    assert!(archive.archives[0].source_hash.get().is_none());
    let manifest = archive.manifest().unwrap();
    let expected = hash_file(&archive.archives[0].path).unwrap();
    assert_eq!(manifest.sources[0].source_revision, expected);
    assert_eq!(manifest.entries[0].source_revision, expected);
    assert_eq!(archive.archives[0].source_hash.get(), Some(&expected));
    assert_eq!(archive.manifest().unwrap(), manifest);
}

#[test]
fn explicit_validation_detects_payload_changes_even_with_preserved_revision() {
    let (_root, archive) = fixture();
    archive.manifest().unwrap();
    let source = &archive.archives[0];
    std::fs::write(&source.path, b"modified bytes").unwrap();
    File::options()
        .write(true)
        .open(&source.path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(source.source_stamp.modified))
        .unwrap();
    assert_eq!(
        archive.validate_sources().unwrap_err().code(),
        "ASTRA_EMU_CMVS_SOURCE_CHANGED"
    );
}

#[test]
fn malformed_source_is_rejected_before_content_hashing() {
    let (_root, archive) = fixture();
    let result = CmvsArchive::mount(
        "test".into(),
        "cmvs:/".into(),
        vec![("test".into(), archive.archives[0].path.clone())],
        archive.scheme.clone(),
        archive.private_profile_hash,
        archive.mount_profile_hash,
    );
    assert!(result.is_err());
    assert!(archive.archives[0].source_hash.get().is_none());
}
