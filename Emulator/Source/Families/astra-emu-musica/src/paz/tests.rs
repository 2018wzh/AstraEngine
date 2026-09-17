use super::*;
use blowfish::cipher::BlockCipherEncrypt;
use std::{fs, io::Write};

const FIXTURE_KEY: &[u8] = b"fixture-key";

fn fixture_archive(role: &str, version: u8) -> Vec<u8> {
    let payload = b"fixture\0";
    let mut index = Vec::new();
    index.extend_from_slice(&1u32.to_le_bytes());
    if role == "mov" {
        index.extend(0u8..=255);
    }
    index.extend_from_slice(format!("{role}.bin\0").as_bytes());
    let descriptor_offset = index.len();
    index.extend_from_slice(&0u64.to_le_bytes());
    index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    index.extend_from_slice(&0i32.to_le_bytes());
    while !index.len().is_multiple_of(8) {
        index.push(0);
    }
    let header_size = if version == 0 { 4 } else { 0x24 };
    let payload_offset = header_size + index.len() as u64;
    index[descriptor_offset..descriptor_offset + 8].copy_from_slice(&payload_offset.to_le_bytes());

    let index = blowfish_encrypt(FIXTURE_KEY, &index);
    let payload = if role == "mov" {
        payload.to_vec()
    } else {
        blowfish_encrypt(FIXTURE_KEY, payload)
    };
    let mut archive = Vec::new();
    if version == 0 {
        archive.extend_from_slice(&(index.len() as u32).to_le_bytes());
        archive.extend_from_slice(&index);
        archive.extend_from_slice(&payload);
    } else {
        let xor_key = 0x5au8;
        let derived = u32::from_le_bytes([xor_key; 4]);
        archive.resize(0x20, 0);
        archive.extend_from_slice(&((index.len() as u32) ^ derived).to_le_bytes());
        archive.extend(index.into_iter().map(|byte| byte ^ xor_key));
        archive.extend(payload.iter().map(|byte| *byte ^ xor_key));
    }
    archive
}

fn mount_fixture(root: &Path, version: u8) -> MusicaMountedVfs {
    let configs = REQUIRED_ARCHIVE_ROLES
        .iter()
        .map(|role| PazArchiveConfig {
            role: (*role).into(),
            path: root.join(format!("{role}.paz")),
            game_root: root.to_path_buf(),
            version,
            index_size_xor: if version == 0 { 0 } else { 0x5a5a5a5a },
        })
        .collect();
    let roles = REQUIRED_ARCHIVE_ROLES
        .into_iter()
        .map(|role| {
            (
                role.into(),
                PazRoleScheme {
                    index_key: FIXTURE_KEY.to_vec(),
                    data_key: if role == "mov" {
                        Vec::new()
                    } else {
                        FIXTURE_KEY.to_vec()
                    },
                    type_passwords: BTreeMap::new(),
                    archive_xor: None,
                    video_key: None,
                },
            )
        })
        .collect();
    let provider = Arc::new(
        MusicaPazDecryptProvider::new(Hash256::from_sha256(b"fixture-profile"), roles).unwrap(),
    );
    MusicaMountedVfs::mount(
        "fixture",
        "musica:/",
        configs,
        provider,
        Hash256::from_sha256(b"fixture-mount-profile"),
    )
    .unwrap()
}

fn blowfish_encrypt(key: &[u8], plaintext: &[u8]) -> Vec<u8> {
    assert!(plaintext.len().is_multiple_of(8));
    let cipher: Blowfish = Blowfish::new_from_slice(key).unwrap();
    let mut bytes = plaintext.to_vec();
    for chunk in bytes.as_chunks_mut::<8>().0.iter_mut() {
        chunk[..4].reverse();
        chunk[4..].reverse();
        cipher.encrypt_block((&mut *chunk).into());
        chunk[..4].reverse();
        chunk[4..].reverse();
    }
    bytes
}

fn hash_fixture_entry(id: &str, offset: u64, size: u64) -> ArchiveEntryDescriptor {
    ArchiveEntryDescriptor {
        archive_role: "bg".into(),
        entry_id: id.into(),
        name: format!("{id}.bin"),
        offset,
        unpacked_size: size,
        stored_size: size,
        aligned_size: size,
        packed: false,
        video_key: None,
    }
}

#[test]
fn archive_and_entry_hashes_share_one_ordered_stream() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = (0u8..100).collect::<Vec<_>>();
    let first = temp.path().join("archive.paz");
    let second = temp.path().join("archive.pazA");
    fs::write(&first, &bytes[..47]).unwrap();
    fs::write(&second, &bytes[47..]).unwrap();
    let parts = [&first, &second]
        .into_iter()
        .map(|path| {
            let metadata = fs::metadata(path).unwrap();
            ArchivePart {
                path: path.clone(),
                length: metadata.len(),
                modified: metadata.modified().ok(),
            }
        })
        .collect::<Vec<_>>();
    let entries = vec![
        hash_fixture_entry("first", 3, 17),
        hash_fixture_entry("cross-part", 40, 20),
        hash_fixture_entry("empty", 80, 0),
    ];

    let (source_hash, entry_hashes) =
        hash_parts_and_entries(&parts, bytes.len() as u64, &entries).unwrap();

    assert_eq!(source_hash, Hash256::from_sha256(&bytes));
    assert_eq!(entry_hashes[0], Hash256::from_sha256(&bytes[3..20]));
    assert_eq!(entry_hashes[1], Hash256::from_sha256(&bytes[40..60]));
    assert_eq!(entry_hashes[2], Hash256::from_sha256(&[]));
}

#[test]
fn overlapping_encrypted_entry_ranges_are_blocking() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("archive.paz");
    fs::write(&path, [0u8; 32]).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let parts = vec![ArchivePart {
        path,
        length: metadata.len(),
        modified: metadata.modified().ok(),
    }];
    let entries = vec![
        hash_fixture_entry("first", 0, 16),
        hash_fixture_entry("second", 8, 16),
    ];

    assert_eq!(
        hash_parts_and_entries(&parts, 32, &entries)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_ENTRY_OVERLAP"
    );
}

#[test]
fn traversal_is_rejected() {
    assert_eq!(
        normalize_entry_name("../secret.sc").unwrap_err().code(),
        "ASTRA_EMU_MUSICA_ENTRY_PATH"
    );
}
#[test]
fn required_roles_are_strict() {
    let configs = vec![];
    assert_eq!(
        validate_role_set(&configs).unwrap_err().code(),
        "ASTRA_EMU_MUSICA_ARCHIVE_MISSING"
    );
}

#[test]
fn v0_fixture_mounts_all_roles_and_reads_across_a_volume_boundary() {
    let temp = tempfile::tempdir().unwrap();
    for role in REQUIRED_ARCHIVE_ROLES {
        let archive = fixture_archive(role, 0);
        let path = temp.path().join(format!("{role}.paz"));
        if role == "scr" {
            let split = archive.len() - 6;
            fs::write(&path, &archive[..split]).unwrap();
            fs::write(temp.path().join("scr.pazA"), &archive[split..]).unwrap();
        } else {
            fs::write(path, archive).unwrap();
        }
    }
    let vfs = mount_fixture(temp.path(), 0);
    assert_eq!(vfs.manifest().entries.len(), REQUIRED_ARCHIVE_ROLES.len());
    for entry in &vfs.manifest().entries {
        let source = vfs
            .manifest()
            .sources
            .iter()
            .find(|source| source.source_id == entry.source_id)
            .unwrap();
        assert_eq!(entry.source_hash, source.source_hash);
    }
    let root = vfs.read_dir("musica:/").unwrap();
    assert_eq!(root.len(), REQUIRED_ARCHIVE_ROLES.len());
    assert!(root
        .iter()
        .all(|node| node.kind == ArchiveNodeKind::Directory));
    let read = vfs.read_range("musica:/scr/scr.bin", 3, 4).unwrap();
    assert_eq!(read.bytes.as_slice(), b"ture");
    assert!(!read.cache_hit);
}

#[test]
fn source_mutation_after_mount_is_blocking() {
    let temp = tempfile::tempdir().unwrap();
    for role in REQUIRED_ARCHIVE_ROLES {
        fs::write(
            temp.path().join(format!("{role}.paz")),
            fixture_archive(role, 0),
        )
        .unwrap();
    }
    let vfs = mount_fixture(temp.path(), 0);
    fs::OpenOptions::new()
        .append(true)
        .open(temp.path().join("scr.paz"))
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert_eq!(
        vfs.read_range("musica:/scr/scr.bin", 0, 1)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_SOURCE_CHANGED"
    );
}

#[test]
fn v1_and_v2_fixtures_apply_archive_xor_and_random_reads() {
    for version in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        for role in REQUIRED_ARCHIVE_ROLES {
            fs::write(
                temp.path().join(format!("{role}.paz")),
                fixture_archive(role, version),
            )
            .unwrap();
        }
        let vfs = mount_fixture(temp.path(), version);
        let read = vfs.read_range("musica:/voice/voice.bin", 1, 6).unwrap();
        assert_eq!(read.bytes.as_slice(), b"ixture");
    }
}
