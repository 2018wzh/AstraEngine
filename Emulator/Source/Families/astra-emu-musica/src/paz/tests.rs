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
                    rc4_skip_crc: true,
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

fn index_fixture_entry(id: &str, offset: u64, size: u64) -> ArchiveEntryDescriptor {
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
fn archive_revision_uses_index_and_metadata_without_opening_payloads() {
    let mut parts = vec![ArchivePart {
        path: PathBuf::from("unopened.paz"),
        length: 1024,
        modified: Some(UNIX_EPOCH + std::time::Duration::from_secs(1)),
    }];
    let first = archive_revision(&parts, 1, 0, b"index");
    parts[0].path = PathBuf::from("relocated.paz");
    assert_eq!(first, archive_revision(&parts, 1, 0, b"index"));
    assert_ne!(first, archive_revision(&parts, 1, 0, b"different index"));
    parts[0].length += 1;
    assert_ne!(first, archive_revision(&parts, 1, 0, b"index"));
    parts[0].length -= 1;
    parts[0].modified = Some(UNIX_EPOCH + std::time::Duration::from_secs(2));
    assert_ne!(first, archive_revision(&parts, 1, 0, b"index"));
}

#[test]
fn overlapping_encrypted_entry_ranges_are_blocking() {
    let entries = vec![
        index_fixture_entry("first", 0, 16),
        index_fixture_entry("second", 8, 16),
    ];

    assert_eq!(
        validate_entry_ranges(32, &entries).unwrap_err().code(),
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
        assert_eq!(entry.source_revision, source.source_revision);
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

#[test]
fn movie_spooling_preserves_range_limit_and_streams_large_payloads() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let temp = tempfile::tempdir().unwrap();
    for role in REQUIRED_ARCHIVE_ROLES {
        if role != "mov" {
            fs::write(
                temp.path().join(format!("{role}.paz")),
                fixture_archive(role, 0),
            )
            .unwrap();
        }
    }
    let length = ARCHIVE_MAX_READ_BYTES + 8;
    let mut index = 1u32.to_le_bytes().to_vec();
    index.extend((0u8..=255).rev());
    index.extend_from_slice(b"movie.bin\0");
    let descriptor = index.len();
    index.extend_from_slice(&0u64.to_le_bytes());
    for _ in 0..3 {
        index.extend_from_slice(&(length as u32).to_le_bytes());
    }
    index.extend_from_slice(&0i32.to_le_bytes());
    while !index.len().is_multiple_of(8) {
        index.push(0);
    }
    let offset = index.len() as u64 + 4;
    index[descriptor..descriptor + 8].copy_from_slice(&offset.to_le_bytes());
    let encoded = blowfish_encrypt(FIXTURE_KEY, &index);
    let mut movie = File::create(temp.path().join("mov.paz")).unwrap();
    movie
        .write_all(&(encoded.len() as u32).to_le_bytes())
        .unwrap();
    movie.write_all(&encoded).unwrap();
    movie.set_len(offset + length).unwrap();
    drop(movie);
    let archive = mount_fixture(temp.path(), 0);
    let uri = "musica:/mov/movie.bin";
    assert_eq!(
        archive.read_range(uri, 0, length).unwrap_err().code(),
        "ASTRA_EMU_VFS_READ_LIMIT"
    );
    struct Plaintext<'a> {
        bytes: u64,
        stop: &'a AtomicBool,
        cancel: bool,
    }
    impl Write for Plaintext<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            assert!(bytes.iter().all(|byte| *byte == 255));
            self.bytes += bytes.len() as u64;
            if self.cancel {
                self.stop.store(true, Ordering::Release);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let stop = AtomicBool::new(false);
    let mut output = Plaintext {
        bytes: 0,
        stop: &stop,
        cancel: false,
    };
    assert!(archive
        .write_movie(uri, &mut output, length - 1, &stop)
        .is_err());
    assert_eq!(output.bytes, 0);
    archive
        .write_movie(uri, &mut output, length, &stop)
        .unwrap();
    assert_eq!(output.bytes, length);
    output.bytes = 0;
    output.cancel = true;
    assert_eq!(
        archive
            .write_movie(uri, &mut output, length, &stop)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_MOVIE_CANCELLED"
    );
    assert_eq!(output.bytes, 64 * 1024);
}

#[test]
fn movie_spooling_matches_entry_decryption_across_archive_parts() {
    use std::sync::atomic::AtomicBool;
    for version in [0, 1, 2] {
        let temp = tempfile::tempdir().unwrap();
        for role in REQUIRED_ARCHIVE_ROLES {
            let bytes = fixture_archive(role, version);
            let path = temp.path().join(format!("{role}.paz"));
            if role == "mov" {
                let split = bytes.len() - 3;
                fs::write(path, &bytes[..split]).unwrap();
                fs::write(temp.path().join("mov.pazA"), &bytes[split..]).unwrap();
            } else {
                fs::write(path, bytes).unwrap();
            }
        }
        let archive = mount_fixture(temp.path(), version);
        let expected = archive.read_range("musica:/mov/mov.bin", 0, 8).unwrap();
        let mut actual = Vec::new();
        archive
            .write_movie(
                "musica:/mov/mov.bin",
                &mut actual,
                8,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(actual.as_slice(), expected.bytes.as_slice());
    }
}

#[test]
fn movie_type_password_is_used_across_cipher_and_volume_boundaries() {
    use std::sync::atomic::AtomicBool;
    let temp = tempfile::tempdir().unwrap();
    for role in REQUIRED_ARCHIVE_ROLES {
        if role != "mov" {
            fs::write(
                temp.path().join(format!("{role}.paz")),
                fixture_archive(role, 2),
            )
            .unwrap();
        }
    }
    let length = 0x10008u32;
    let video_key: Vec<u8> = (0..=255).collect();
    let mut index = 1u32.to_le_bytes().to_vec();
    index.extend_from_slice(&video_key);
    index.extend_from_slice(b"opening.avi\0");
    let descriptor = index.len();
    index.extend_from_slice(&0u64.to_le_bytes());
    for _ in 0..3 {
        index.extend_from_slice(&length.to_le_bytes());
    }
    index.extend_from_slice(&0i32.to_le_bytes());
    while !index.len().is_multiple_of(8) {
        index.push(0);
    }
    let offset = 0x24 + index.len() as u64;
    index[descriptor..descriptor + 8].copy_from_slice(&offset.to_le_bytes());
    // Independent fixture material from the PAZ entry-key format, including avi password.
    let material = b"opening.avi 00010008 fixture-video-password";
    let key: Vec<_> = video_key
        .iter()
        .enumerate()
        .map(|(index, value)| value ^ material[index % material.len()])
        .collect();
    let mut cipher = Rc4::new_from_slice(&key).unwrap();
    let mut mask = vec![0; 0x10000];
    cipher.apply_keystream(&mut mask);
    let plaintext: Vec<_> = (0..length)
        .map(|index| (index.wrapping_mul(13) + 7) as u8)
        .collect();
    let mut bytes = vec![0; 0x20];
    bytes.extend_from_slice(&((index.len() as u32) ^ 0x5a5a5a5a).to_le_bytes());
    bytes.extend(
        blowfish_encrypt(FIXTURE_KEY, &index)
            .into_iter()
            .map(|byte| byte ^ 0x5a),
    );
    bytes.extend(
        plaintext
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % mask.len()] ^ 0x5a),
    );
    let split = offset as usize + 0xffff;
    fs::write(temp.path().join("mov.paz"), &bytes[..split]).unwrap();
    fs::write(temp.path().join("mov.pazA"), &bytes[split..]).unwrap();
    let mut archive = mount_fixture(temp.path(), 2);
    Arc::get_mut(&mut archive.decrypt_provider)
        .unwrap()
        .roles
        .get_mut("mov")
        .unwrap()
        .type_passwords
        .insert("avi".into(), "fixture-video-password".into());
    let ranged = archive
        .read_range("musica:/mov/opening.avi", 0, length.into())
        .unwrap();
    assert_eq!(ranged.bytes.as_slice(), plaintext);
    let mut streamed = Vec::new();
    archive
        .write_movie(
            "musica:/mov/opening.avi",
            &mut streamed,
            length.into(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(streamed, plaintext);
}

#[test]
fn explicit_crc_skip_option_decrypts_both_native_v2_entry_variants() {
    let entry = ArchiveEntryDescriptor {
        archive_role: "scr".into(), entry_id: "fixture".into(), name: "fixture.sc".into(),
        offset: 0, unpacked_size: 8, stored_size: 8, aligned_size: 8, packed: false, video_key: None,
    };
    let material = b"fixture.sc 00000008 public-password";
    let plaintext = b"public!!";
    for rc4_skip_crc in [false, true] {
        let mut cipher = Rc4::new_from_slice(material).unwrap();
        if rc4_skip_crc {
            let skip = ((crc32fast::hash(material) >> 12) & 0xff) as usize;
            cipher.apply_keystream(&mut vec![0; skip]);
        }
        let mut encrypted = plaintext.to_vec();
        cipher.apply_keystream(&mut encrypted);
        let encrypted = blowfish_encrypt(FIXTURE_KEY, &encrypted);
        let roles = REQUIRED_ARCHIVE_ROLES.into_iter().map(|role| (
            role.into(),
            PazRoleScheme {
                index_key: FIXTURE_KEY.to_vec(), data_key: FIXTURE_KEY.to_vec(), rc4_skip_crc,
                type_passwords: BTreeMap::from([("sc".into(), "public-password".into())]),
                archive_xor: None, video_key: None,
            },
        )).collect();
        let provider = MusicaPazDecryptProvider::new(Hash256::from_sha256(b"fixture"), roles).unwrap();
        assert_eq!(provider.decrypt_entry_chunk(2, &entry, 0, &encrypted).unwrap(), plaintext);
    }
}
