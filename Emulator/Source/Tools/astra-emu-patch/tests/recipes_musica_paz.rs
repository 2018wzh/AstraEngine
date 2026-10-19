use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

// Data-only PE32 fixture. This is never executable code: its only section is
// read-only initialized data containing original pointer pairs or GBK names.
fn image(base: u32) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x14c_u16.to_le_bytes());
    bytes[0x86..0x88].copy_from_slice(&1_u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xe0_u16.to_le_bytes());
    bytes[0x96..0x98].copy_from_slice(&0x102_u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x10b_u16.to_le_bytes());
    put32(&mut bytes, 0xb4, base);
    put32(&mut bytes, 0xb8, 4096);
    put32(&mut bytes, 0xbc, 512);
    put32(&mut bytes, 0xd0, 8192);
    put32(&mut bytes, 0xd4, 512);
    put32(&mut bytes, 0xf4, 16);
    bytes[0x178..0x17d].copy_from_slice(b".data");
    put32(&mut bytes, 0x180, 512);
    put32(&mut bytes, 0x184, 4096);
    put32(&mut bytes, 0x188, 512);
    put32(&mut bytes, 0x18c, 512);
    put32(&mut bytes, 0x19c, 0x4000_0040);
    bytes
}

fn fixtures() -> (Vec<u8>, Vec<u8>) {
    let mut original = image(0x400000);
    put32(&mut original, 512, 0x401180);
    put32(&mut original, 516, 0x401100);
    put32(&mut original, 520, 0x401190);
    put32(&mut original, 524, 0x401120);
    original[768..776].copy_from_slice(b"speaker\0");
    // Non-ASCII raw identity. Its Unicode representation must never be hashed.
    original[800..805].copy_from_slice(b"raw\x81\x40");
    let mut patch = image(0x10000000);
    patch[768..772].copy_from_slice(b"\xb9\xab\xbf\xaa"); // 公开
    patch[800..804].copy_from_slice(b"\xc3\xfb\xd7\xd6"); // 名字
    (original, patch)
}

fn request(root: &Path, original: &[u8], patch: &[u8]) -> PatchRequest {
    let source = root.join("source");
    let patch_root = root.join("patch");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch_root).unwrap();
    fs::write(source.join("original.exe"), original).unwrap();
    fs::write(patch_root.join("names.dll"), patch).unwrap();
    let common = root.join("common");
    fs::create_dir(&common).unwrap();
    fs::write(
        common.join("musica.luau"),
        include_str!("../../../../Patches/common/musica.luau"),
    )
    .unwrap();
    fs::write(
        common.join("pe.luau"),
        include_str!("../../../../Patches/common/pe.luau"),
    )
    .unwrap();
    fs::write(
        common.join("binary.luau"),
        include_str!("../../../../Patches/common/binary.luau"),
    )
    .unwrap();
    let script = root.join("names.luau");
    fs::write(&script, r#"
return { id="public-musica-names", version=1, title="Public PE names", apply=function(ctx)
    local executable = ctx.open("source", "original.exe")
    local patch = ctx.open("patch", "names.dll")
    local names = ctx.load("common/musica.luau").names(ctx, executable, patch,
        {table_rva=0x1000,targets={0x1100,0x1120}})
    local out=ctx.create("names.json")
    out:write(0,buffer.fromstring(ctx.json.encode(names)))
    out:close()
    return {plugin_id="astra.emu.musica",configuration={script_encoding="gbk",translation_table="names.json"}}
end }
"#).unwrap();
    PatchRequest {
        script,
        source,
        patch: patch_root,
        output: root.join("output"),
    }
}

fn raw_hash(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .filter(|byte| **byte != 0)
        .fold(0_u32, |hash, byte| {
            hash.wrapping_mul(131).wrapping_add(u32::from(*byte)) & 0x7fff_ffff
        })
}

#[test]
fn pe_pointer_pairs_preserve_raw_identity_and_strict_gbk_targets() {
    let root = tempfile::tempdir().unwrap();
    let (original, patch) = fixtures();
    let request = request(root.path(), &original, &patch);
    let source_path = request.source.join("original.exe");
    let patch_path = request.patch.join("names.dll");
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    let table: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("names.json")).unwrap()).unwrap();
    assert_eq!(table["source_encoding"], "gbk");
    assert_eq!(table["version"], 2);
    assert_eq!(table["hash_collision_policy"], "first_record");
    assert_eq!(table["dialogue"], serde_json::json!([]));
    let names = table["names"].as_array().unwrap();
    for (source, target) in [
        (b"speaker".as_slice(), "公开"),
        (b"raw\x81\x40".as_slice(), "名字"),
    ] {
        let row = names
            .iter()
            .find(|row| row["source_hash"] == raw_hash(source))
            .unwrap();
        assert_eq!(row["text"], target);
        assert_eq!(
            row["source_sha256"],
            format!("{:x}", Sha256::digest(source))
        );
    }
    assert_eq!(fs::read(source_path).unwrap(), original);
    assert_eq!(fs::read(patch_path).unwrap(), patch);
}

#[test]
fn invalid_target_gbk_fails_without_publishing() {
    let root = tempfile::tempdir().unwrap();
    let (original, mut patch) = fixtures();
    patch[768..773].copy_from_slice(b"\x81\0\0\0\0");
    let request = request(root.path(), &original, &patch);
    let output = request.output.clone();
    let error = apply(request, Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .expect("invalid GBK must fail");
    assert_eq!(error.kind(), PatchErrorKind::Codec);
    assert!(!output.exists());
}

#[test]
fn unmapped_original_pointer_fails_at_pe_reference_boundary() {
    let root = tempfile::tempdir().unwrap();
    let (mut original, patch) = fixtures();
    put32(&mut original, 516, 0x409000);
    let request = request(root.path(), &original, &patch);
    let output = request.output.clone();
    assert!(apply(request, Arc::new(AtomicBool::new(false)), |_| {}).is_err());
    assert!(!output.exists());
}

#[test]
fn duplicate_raw_speaker_identity_retains_ordered_native_records() {
    let root = tempfile::tempdir().unwrap();
    let (mut original, patch) = fixtures();
    put32(&mut original, 524, 0x401100);
    let request = request(root.path(), &original, &patch);
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    let table: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("names.json")).unwrap()).unwrap();
    let hash = raw_hash(b"speaker");
    let sha = format!("{:x}", Sha256::digest(b"speaker"));
    assert_eq!(
        table["names"],
        serde_json::json!([
            {"source_hash":hash,"source_sha256":sha,"text":"公开"},
            {"source_hash":hash,"source_sha256":sha,"text":"名字"}
        ])
    );
}

#[test]
fn unterminated_name_fails_without_reading_outside_backed_file() {
    let root = tempfile::tempdir().unwrap();
    let (original, mut patch) = fixtures();
    patch[768..].fill(b'A');
    let request = request(root.path(), &original, &patch);
    let output = request.output.clone();
    assert!(apply(request, Arc::new(AtomicBool::new(false)), |_| {}).is_err());
    assert!(!output.exists());
}
