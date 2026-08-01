use std::io::Read;

use astra_byte_source::MemoryByteSource;
use astra_emu_family_core::{
    LegacyOpaqueFamilyConfig, LegacyVfsFamilyFactory, LegacyVfsMountContext,
};

use super::*;

#[test]
fn fvp_factory_mounts_nested_entries_with_bounded_ranges_and_streams() {
    let root = tempfile::tempdir().unwrap();
    let archive_path = root.path().join("Graph.bin");
    std::fs::write(
        &archive_path,
        archive(&[("BG/Scene", b"hzc1-image"), ("Movie/Op", b"movie-data")]),
    )
    .unwrap();
    let options = FvpVfsFamilyOptions {
        nls: FvpNls::Utf8,
        archives: vec!["Graph.bin".into()],
        max_entries_per_archive: 16,
    };
    let payload = serde_json::to_vec(&options).unwrap();
    let context = LegacyVfsMountContext {
        game_root: root.path().canonicalize().unwrap(),
        profile_id: "fixture".into(),
        profile_hash: Hash256::from_sha256(b"profile"),
        mount_id: "fixture-fvp".into(),
        prefix: "fvp:/".into(),
        private_patch: None,
        family_config: LegacyOpaqueFamilyConfig {
            schema_id: FVP_FAMILY_OPTIONS_SCHEMA.into(),
            schema_hash: Hash256::from_sha256(FVP_FAMILY_OPTIONS_SCHEMA.as_bytes()),
            payload,
        },
    };
    let mounted = FvpVfsFamilyFactory.mount(&context).unwrap();
    assert_eq!(mounted.read_dir("fvp:/").unwrap()[0].name, "graph");
    assert_eq!(
        mounted.stat("fvp:/graph/bg/scene").unwrap().size,
        b"hzc1-image".len() as u64
    );
    assert_eq!(
        mounted
            .read_range("fvp:/graph/bg/scene", 5, 5)
            .unwrap()
            .bytes,
        b"image"
    );
    let mut stream = mounted.open_stream("fvp:/graph/movie/op").unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"movie-data");
    mounted.validate_sources().unwrap();
    std::fs::write(
        archive_path,
        archive(&[("BG/Scene", b"hzc1-IMAGE"), ("Movie/Op", b"movie-data")]),
    )
    .unwrap();
    assert_eq!(
        mounted.validate_sources().unwrap_err().code(),
        "ASTRA_EMU_VFS_SOURCE_CHANGED"
    );
}

#[test]
fn fvp_factory_rejects_duplicate_archive_roles() {
    let options = FvpVfsFamilyOptions {
        nls: FvpNls::Utf8,
        archives: vec!["Graph.bin".into(), "graph.BIN".into()],
        max_entries_per_archive: 16,
    };
    assert_eq!(
        validate_options(&options).unwrap_err().code(),
        "ASTRA_EMU_FVP_ARCHIVE_DUPLICATE"
    );
}

#[test]
fn single_pass_audit_rejects_overlapping_entry_ranges() {
    let mut bytes = archive(&[("one", b"1111"), ("two", b"2222")]);
    let first_offset = bytes[12..16].to_vec();
    bytes[24..28].copy_from_slice(&first_offset);
    let source: Arc<dyn BoundedByteSource> = Arc::new(MemoryByteSource::new(bytes));
    let archive = FvpArchive::open_source(source, FvpNls::Utf8, 8).unwrap();
    assert_eq!(
        audit_archive(&archive).unwrap_err().code(),
        "ASTRA_EMU_FVP_ENTRY_OVERLAP"
    );
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut names = Vec::new();
    let mut name_offsets = Vec::new();
    for (name, _) in entries {
        name_offsets.push(names.len() as u32);
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }
    let payload_start = 8 + entries.len() * 12 + names.len();
    let mut payload_offset = payload_start as u32;
    let mut bytes = (entries.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for ((_, payload), name_offset) in entries.iter().zip(name_offsets) {
        bytes.extend_from_slice(&name_offset.to_le_bytes());
        bytes.extend_from_slice(&payload_offset.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        payload_offset += payload.len() as u32;
    }
    bytes.extend_from_slice(&names);
    for (_, payload) in entries {
        bytes.extend_from_slice(payload);
    }
    bytes
}
