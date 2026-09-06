use std::{collections::BTreeMap, fs, io::Read, sync::Arc};

use blowfish::cipher::{BlockCipherEncrypt, KeyInit};
use flate2::{write::ZlibEncoder, Compression};
use rc4::{Rc4, StreamCipher};

use super::*;

const INDEX_KEY: &[u8] = b"index-key";
const DATA_KEY: &[u8] = b"data-key";

fn decryptor(passwords: BTreeMap<String, String>) -> Arc<MusicaPazDecryptor> {
    Arc::new(
        MusicaPazDecryptor::new_with_locale(
            REQUIRED_ARCHIVE_ROLES
                .into_iter()
                .map(|role| {
                    (
                        role.to_owned(),
                        PazRoleScheme {
                            index_key: INDEX_KEY.to_vec(),
                            data_key: if role == "mov" {
                                Vec::new()
                            } else {
                                DATA_KEY.to_vec()
                            },
                            type_passwords: passwords.clone(),
                        },
                    )
                })
                .collect(),
            MusicaLocaleHook::japanese_cp932(),
        )
        .unwrap(),
    )
}

fn blowfish_encrypt(key: &[u8], plain: &[u8]) -> Vec<u8> {
    assert!(plain.len().is_multiple_of(8));
    let cipher: Blowfish = Blowfish::new_from_slice(key).unwrap();
    let mut bytes = plain.to_vec();
    for chunk in bytes.as_chunks_mut::<8>().0.iter_mut() {
        chunk[..4].reverse();
        chunk[4..].reverse();
        cipher.encrypt_block((&mut *chunk).into());
        chunk[..4].reverse();
        chunk[4..].reverse();
    }
    bytes
}

fn rc4_transform(version: u8, entry: &PazEntryDescriptor, bytes: &mut [u8]) {
    let key = entry_key_material_with_locale(entry, Some("pw"), MusicaLocaleHook::japanese_cp932())
        .unwrap();
    let mut cipher = Rc4::new_from_slice(&key).unwrap();
    let skip = if version >= 2 {
        (crc32(&key) >> 12 & 0xff) as usize
    } else {
        0
    };
    let mut discarded = vec![0; skip];
    cipher.apply_keystream(&mut discarded);
    cipher.apply_keystream(bytes);
}

fn archive_from_parts(root: &Path, bytes: &[u8], split: usize) -> ArchiveSource {
    let chunks = if split == 0 || split >= bytes.len() {
        vec![bytes]
    } else {
        vec![&bytes[..split], &bytes[split..]]
    };
    let mut parts = Vec::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        let path = root.join(format!("scr.paz{index}"));
        fs::write(&path, chunk).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        parts.push(ArchivePart {
            path,
            length: metadata.len(),
            modified: metadata.modified().ok(),
        });
    }
    ArchiveSource {
        role: "scr".into(),
        parts,
        version: 0,
        length: bytes.len() as u64,
        hash: Hash256::from_sha256(bytes),
        xor_key: 0,
    }
}

#[test]
fn v1_and_v2_rc4_chunks_resume_at_absolute_offsets() {
    let mut passwords = BTreeMap::new();
    passwords.insert("sc".into(), "pw".into());
    let decryptor = decryptor(passwords);
    let plain = (0u8..96).collect::<Vec<_>>();
    let entry = PazEntryDescriptor {
        archive_role: "scr".into(),
        entry_id: "scr:0".into(),
        name: "SCRIPT.SC".into(),
        crypto_name: b"SCRIPT.SC".to_vec(),
        offset: 0,
        unpacked_size: plain.len() as u64,
        stored_size: plain.len() as u64,
        aligned_size: plain.len() as u64,
        packed: false,
        video_key: None,
    };

    for version in [1, 2] {
        let mut transformed = plain.clone();
        rc4_transform(version, &entry, &mut transformed);
        let encrypted = blowfish_encrypt(DATA_KEY, &transformed);
        for (start, end) in [(0usize, 32usize), (16, 72), (64, 96)] {
            let decoded = decryptor
                .decrypt_entry_chunk(
                    version,
                    &entry,
                    start as u64,
                    encrypted[start..end].to_vec(),
                )
                .unwrap();
            assert_eq!(decoded, plain[start..end]);
        }
    }
}

#[test]
fn entry_chunk_transform_reuses_the_owned_source_buffer() {
    let decryptor = decryptor(BTreeMap::new());
    let plain = (0u8..64).collect::<Vec<_>>();
    let entry = PazEntryDescriptor {
        archive_role: "scr".into(),
        entry_id: "scr:0".into(),
        name: "payload.sc".into(),
        crypto_name: b"payload.sc".to_vec(),
        offset: 0,
        unpacked_size: plain.len() as u64,
        stored_size: plain.len() as u64,
        aligned_size: plain.len() as u64,
        packed: false,
        video_key: None,
    };
    let encrypted = blowfish_encrypt(DATA_KEY, &plain);
    let source_allocation = encrypted.as_ptr();

    let decoded = decryptor
        .decrypt_entry_chunk(0, &entry, 0, encrypted)
        .unwrap();

    assert_eq!(decoded, plain);
    assert_eq!(decoded.as_ptr(), source_allocation);
}

#[test]
fn movie_v0_substitution_and_v1_periodic_rc4_are_range_stable() {
    let decryptor = decryptor(BTreeMap::new());
    let plain = (0u8..=255).cycle().take(320).collect::<Vec<_>>();
    let substitution = (0u8..=255).rev().collect::<Vec<_>>();
    let mut entry = PazEntryDescriptor {
        archive_role: "mov".into(),
        entry_id: "mov:0".into(),
        name: "movie.avi".into(),
        crypto_name: b"movie.avi".to_vec(),
        offset: 0,
        unpacked_size: plain.len() as u64,
        stored_size: plain.len() as u64,
        aligned_size: plain.len() as u64,
        packed: false,
        video_key: Some(substitution.clone()),
    };
    let encrypted_v0 = plain
        .iter()
        .map(|byte| substitution[*byte as usize])
        .collect::<Vec<_>>();
    assert_eq!(
        decryptor
            .decrypt_entry_chunk(0, &entry, 47, encrypted_v0[47..233].to_vec())
            .unwrap(),
        plain[47..233]
    );

    entry.video_key = Some((0u8..=255).collect());
    let entry_key =
        entry_key_material_with_locale(&entry, None, MusicaLocaleHook::japanese_cp932()).unwrap();
    let key = (0..256)
        .map(|index| index as u8 ^ entry_key[index % entry_key.len()])
        .collect::<Vec<_>>();
    let mut cipher = Rc4::new_from_slice(&key).unwrap();
    let mut keystream = vec![0; 0x140];
    cipher.apply_keystream(&mut keystream);
    let encrypted_v1 = plain
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ keystream[index])
        .collect::<Vec<_>>();
    assert_eq!(
        decryptor
            .decrypt_entry_chunk(1, &entry, 73, encrypted_v1[73..291].to_vec())
            .unwrap(),
        plain[73..291]
    );
}

#[test]
fn multipart_zlib_stream_is_incremental_and_enforces_decoded_size() {
    let root = tempfile::tempdir().unwrap();
    let plain = (0u8..=255).cycle().take(512 * 1024).collect::<Vec<_>>();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, &plain).unwrap();
    let compressed = encoder.finish().unwrap();
    let stored_size = compressed.len();
    let mut padded = compressed;
    padded.resize(stored_size.next_multiple_of(8), 0);
    let encrypted = blowfish_encrypt(DATA_KEY, &padded);
    let archive = archive_from_parts(root.path(), &encrypted, encrypted.len() / 2 + 3);
    let entry = PazEntryDescriptor {
        archive_role: "scr".into(),
        entry_id: "scr:0".into(),
        name: "payload.bin".into(),
        crypto_name: b"payload.bin".to_vec(),
        offset: 0,
        unpacked_size: plain.len() as u64,
        stored_size: stored_size as u64,
        aligned_size: padded.len() as u64,
        packed: true,
        video_key: None,
    };
    let raw = MusicaEntryStream {
        archive,
        entry: entry.clone(),
        decryptor: decryptor(BTreeMap::new()),
        encrypted_position: 0,
        pending: Vec::new(),
        pending_position: 0,
    };
    let mut stream = MusicaDecodedStream {
        inner: MusicaDecodedInner::Zlib(ZlibDecoder::new(raw)),
        remaining: entry.unpacked_size,
        eof_checked: false,
    };
    let mut prefix = vec![0; 7_919];
    stream.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix, plain[..prefix.len()]);
    let mut remainder = Vec::new();
    stream.read_to_end(&mut remainder).unwrap();
    assert_eq!(remainder, plain[prefix.len()..]);
}

#[test]
fn v2_zlib_stream_preserves_checksum_across_decrypt_chunks() {
    let root = tempfile::tempdir().unwrap();
    let mut state = 0x6d2b_79f5_u32;
    let plain = (0..2 * STREAM_CHUNK_BYTES)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect::<Vec<_>>();
    let mut encoded_plain = plain.clone();
    encoded_plain.extend_from_slice(&[0; 8]);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, &encoded_plain).unwrap();
    let compressed = encoder.finish().unwrap();
    assert!(compressed.len() as u64 > STREAM_CHUNK_BYTES);
    let stored_size = compressed.len();
    let mut padded = compressed;
    padded.resize(stored_size.next_multiple_of(8), 0);
    let expected_padded = padded.clone();
    let entry = PazEntryDescriptor {
        archive_role: "scr".into(),
        entry_id: "scr:0".into(),
        name: "payload.sc".into(),
        crypto_name: b"payload.sc".to_vec(),
        offset: 0,
        unpacked_size: plain.len() as u64,
        stored_size: stored_size as u64,
        aligned_size: padded.len() as u64,
        packed: true,
        video_key: None,
    };
    let encrypted = blowfish_encrypt(DATA_KEY, &padded);
    for (index, chunk) in encrypted.chunks(STREAM_CHUNK_BYTES as usize).enumerate() {
        let offset = index as u64 * STREAM_CHUNK_BYTES;
        let decoded = decryptor(BTreeMap::from([("sc".into(), "pw".into())]))
            .decrypt_entry_chunk(2, &entry, offset, chunk.to_vec())
            .unwrap();
        assert!(
            decoded == expected_padded[offset as usize..offset as usize + chunk.len()],
            "decrypt chunk {index} did not preserve the Blowfish block stream"
        );
    }
    let mut archive = archive_from_parts(root.path(), &encrypted, encrypted.len() / 2 + 3);
    archive.version = 2;
    let raw = MusicaEntryStream {
        archive,
        entry: entry.clone(),
        decryptor: decryptor(BTreeMap::from([("sc".into(), "pw".into())])),
        encrypted_position: 0,
        pending: Vec::new(),
        pending_position: 0,
    };
    let mut stream = MusicaDecodedStream {
        inner: MusicaDecodedInner::Zlib(ZlibDecoder::new(raw)),
        remaining: entry.unpacked_size,
        eof_checked: false,
    };
    let mut decoded = Vec::new();
    stream.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, plain);
}

#[test]
fn decoded_zero_padding_is_bounded_and_nonzero_padding_is_rejected() {
    for (padding, accepted) in [(vec![0; 16], true), (vec![0; 17], false), (vec![1], false)] {
        let root = tempfile::tempdir().unwrap();
        let plain = b"decoded payload".to_vec();
        let mut encoded_plain = plain.clone();
        encoded_plain.extend_from_slice(&padding);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        std::io::Write::write_all(&mut encoder, &encoded_plain).unwrap();
        let compressed = encoder.finish().unwrap();
        let stored_size = compressed.len();
        let mut padded = compressed;
        padded.resize(stored_size.next_multiple_of(8), 0);
        let encrypted = blowfish_encrypt(DATA_KEY, &padded);
        let archive = archive_from_parts(root.path(), &encrypted, 0);
        let entry = PazEntryDescriptor {
            archive_role: "scr".into(),
            entry_id: "scr:0".into(),
            name: "payload.bin".into(),
            crypto_name: b"payload.bin".to_vec(),
            offset: 0,
            unpacked_size: plain.len() as u64,
            stored_size: stored_size as u64,
            aligned_size: padded.len() as u64,
            packed: true,
            video_key: None,
        };
        let raw = MusicaEntryStream {
            archive,
            entry: entry.clone(),
            decryptor: decryptor(BTreeMap::new()),
            encrypted_position: 0,
            pending: Vec::new(),
            pending_position: 0,
        };
        let mut stream = MusicaDecodedStream {
            inner: MusicaDecodedInner::Zlib(ZlibDecoder::new(raw)),
            remaining: entry.unpacked_size,
            eof_checked: false,
        };
        let mut decoded = Vec::new();
        let result = stream.read_to_end(&mut decoded);
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            assert_eq!(decoded, plain);
        }
    }
}

#[test]
fn random_raw_ranges_reopen_the_encrypted_source_without_plaintext_cache() {
    let root = tempfile::tempdir().unwrap();
    let mut passwords = BTreeMap::new();
    passwords.insert("sc".into(), "pw".into());
    let decryptor = decryptor(passwords);
    let plain = (0u8..128).collect::<Vec<_>>();
    let entry = PazEntryDescriptor {
        archive_role: "scr".into(),
        entry_id: "scr:0".into(),
        name: "SCRIPT.SC".into(),
        crypto_name: b"SCRIPT.SC".to_vec(),
        offset: 0,
        unpacked_size: plain.len() as u64,
        stored_size: plain.len() as u64,
        aligned_size: plain.len() as u64,
        packed: false,
        video_key: None,
    };
    let mut transformed = plain.clone();
    rc4_transform(2, &entry, &mut transformed);
    let encrypted = blowfish_encrypt(DATA_KEY, &transformed);
    let mut archive = archive_from_parts(root.path(), &encrypted, 67);
    archive.version = 2;
    let mounted_entry = MountedEntry {
        descriptor: entry,
        uri: "musica:/scr/script.sc".into(),
        archive: 0,
    };
    let mounted = MusicaMountedVfs {
        mount_id: "test".into(),
        prefix: "musica:/".into(),
        manifest: LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "musica".into(),
            mount_id: "test".into(),
            prefix: "musica:/".into(),
            reader_id: "test".into(),
            reader_hash: Hash256::from_sha256(b"reader"),
            launch_profile_hash: Hash256::from_sha256(b"launch"),
            sources: Vec::new(),
            entries: Vec::new(),
        },
        archives: vec![archive],
        entries: BTreeMap::new(),
        folded_entries: BTreeMap::new(),
        decryptor,
    };
    assert_eq!(
        mounted
            .read_raw_range(&mounted_entry, 13, 57)
            .unwrap()
            .unwrap(),
        plain[13..70]
    );
    fs::remove_file(&mounted.archives[0].parts[0].path).unwrap();
    assert_eq!(
        mounted
            .read_raw_range(&mounted_entry, 13, 57)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_SOURCE_CHANGED"
    );
}

#[test]
fn entry_lookup_matches_windows_ascii_case_and_rejects_folded_conflicts() {
    let descriptor = PazEntryDescriptor {
        archive_role: "bg".into(),
        entry_id: "bg:0".into(),
        name: "WHITE.png".into(),
        crypto_name: b"WHITE.png".to_vec(),
        offset: 0,
        unpacked_size: 8,
        stored_size: 8,
        aligned_size: 8,
        packed: false,
        video_key: None,
    };
    let canonical_uri = "musica:/bg/WHITE.png".to_owned();
    let entry = MountedEntry {
        descriptor: descriptor.clone(),
        uri: canonical_uri.clone(),
        archive: 0,
    };
    let mut entries = BTreeMap::new();
    let mut folded_entries = BTreeMap::new();
    let mut entry_ids = BTreeSet::new();
    insert_mounted_entry(&mut entries, &mut folded_entries, &mut entry_ids, entry).unwrap();

    let mounted = MusicaMountedVfs {
        mount_id: "test".into(),
        prefix: "musica:/".into(),
        manifest: LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "musica".into(),
            mount_id: "test".into(),
            prefix: "musica:/".into(),
            reader_id: "test".into(),
            reader_hash: Hash256::from_sha256(b"reader"),
            launch_profile_hash: Hash256::from_sha256(b"launch"),
            sources: Vec::new(),
            entries: Vec::new(),
        },
        archives: Vec::new(),
        entries,
        folded_entries,
        decryptor: decryptor(BTreeMap::new()),
    };
    assert_eq!(
        mounted.entry("musica:/bg/White.png").unwrap().uri,
        canonical_uri
    );

    let mut entries = BTreeMap::new();
    let mut folded_entries = BTreeMap::new();
    let mut entry_ids = BTreeSet::new();
    insert_mounted_entry(
        &mut entries,
        &mut folded_entries,
        &mut entry_ids,
        MountedEntry {
            descriptor: descriptor.clone(),
            uri: "musica:/bg/WHITE.png".into(),
            archive: 0,
        },
    )
    .unwrap();
    let mut conflict_descriptor = descriptor;
    conflict_descriptor.entry_id = "bg:1".into();
    conflict_descriptor.name = "White.png".into();
    conflict_descriptor.crypto_name = b"White.png".to_vec();
    conflict_descriptor.offset = 8;
    let conflict = insert_mounted_entry(
        &mut entries,
        &mut folded_entries,
        &mut entry_ids,
        MountedEntry {
            descriptor: conflict_descriptor,
            uri: "musica:/bg/White.png".into(),
            archive: 0,
        },
    )
    .unwrap_err();
    assert_eq!(conflict.code(), "ASTRA_EMU_MUSICA_ENTRY_CASE_CONFLICT");
}
