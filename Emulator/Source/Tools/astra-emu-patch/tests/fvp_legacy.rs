use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use flate2::{write::ZlibEncoder, Compression};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    sync::{atomic::AtomicBool, Arc},
};

fn original_hcb() -> Vec<u8> {
    let mut hcb = vec![0; 32];
    hcb[..4].copy_from_slice(&32u32.to_le_bytes());
    hcb[4..9].copy_from_slice(&[2, 20, 0, 0, 0]);
    hcb[9..14].copy_from_slice(&[14, 3, b'a', b'b', 0]);
    hcb[14..19].copy_from_slice(&[14, 3, b'c', b'd', 0]);
    hcb[19] = 4;
    hcb[20..24].copy_from_slice(&[1, 0, 0, 4]);
    hcb[24..28].copy_from_slice(&[1, 4, 0, 4]);
    hcb.extend_from_slice(&4u32.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
    hcb
}
fn dictionary(records: &[(u32, &[u8])]) -> Vec<u8> {
    let mut body = (records.len() as u32).to_le_bytes().to_vec();
    for (offset, bytes) in records {
        body.extend_from_slice(&offset.to_le_bytes());
        body.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        body.extend_from_slice(bytes);
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&body).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut wrapped = Vec::new();
    for word in [0, 123, body.len() as u32, compressed.len() as u32] {
        wrapped.extend_from_slice(&word.to_le_bytes());
    }
    wrapped.extend(compressed);
    wrapped
}
fn fixture(root: &std::path::Path, dat: &[u8]) -> PatchRequest {
    let source = root.join("source");
    let patch = root.join("patch");
    let scripts = root.join("scripts");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&patch).unwrap();
    fs::create_dir_all(scripts.join("common")).unwrap();
    fs::write(source.join("public.hcb"), original_hcb()).unwrap();
    fs::write(patch.join("patch.dat"), dat).unwrap();
    fs::write(
        scripts.join("common/fvp.luau"),
        include_str!("../../../../Metadata/patches/common/fvp.luau"),
    )
    .unwrap();
    let script = scripts.join("fixture.luau");
    fs::write(&script, r#"return {id="public-fvp",version=1,title="Public FVP",apply=function(ctx)
        local fvp=ctx.load("common/fvp.luau")
        local source=ctx.open("source","public.hcb")
        local records=fvp.dictionary(ctx,ctx.open("patch","patch.dat"))
        local patched=fvp.warning(ctx,source,{site=4,original_call=20,helper=24,wait_syscall=137,yield_syscall=133})
        local output=ctx.create("public.hcb") output:write(0,patched) output:close()
        fvp.write_table(ctx,"translation.bin",source:sha256(),ctx.hash.sha256(patched),records)
        return {plugin_id="astra.emu.fvp",configuration={translation_table="translation.bin",script_encoding="shift_jis"}}
    end}"#).unwrap();
    PatchRequest {
        script,
        source,
        patch,
        output: root.join("output"),
    }
}

#[test]
fn emits_sorted_utf8_table_and_exact_append_only_warning_with_original_hashes() {
    let root = tempfile::tempdir().unwrap();
    let dat = dictionary(&[(16, b"second\0"), (11, b"\xd6\xd0\xce\xc4\0")]);
    let request = fixture(root.path(), &dat);
    let source = original_hcb();
    let mut expected = source.clone();
    expected[4] = 6;
    expected[5..9].copy_from_slice(&(source.len() as u32).to_le_bytes());
    expected.extend_from_slice(&[
        2, 20, 0, 0, 0, 11, 250, 0, 14, 8, b'w', b'a', b'r', b'n', b'i', b'n', b'g', 0, 12, 5, 8,
        2, 24, 0, 0, 0, 11, 184, 11, 3, 137, 0, 3, 133, 0, 6, 9, 0, 0, 0,
    ]);
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(
        fs::read(result.output.join("public.hcb")).unwrap(),
        expected
    );
    let mut table = b"AFVPTR1\0".to_vec();
    table.extend_from_slice(&Sha256::digest(&source));
    table.extend_from_slice(&Sha256::digest(&expected));
    table.extend_from_slice(&2u32.to_le_bytes());
    for (offset, text) in [(11u32, "中文"), (16, "second")] {
        table.extend_from_slice(&offset.to_le_bytes());
        table.extend_from_slice(&(text.len() as u32).to_le_bytes());
        table.extend_from_slice(text.as_bytes());
    }
    assert_eq!(
        fs::read(result.output.join("translation.bin")).unwrap(),
        table
    );
    assert_eq!(
        fs::read(root.path().join("source/public.hcb")).unwrap(),
        source
    );
    assert_eq!(fs::read(root.path().join("patch/patch.dat")).unwrap(), dat);
}

#[test]
fn rejects_duplicate_zero_marker_embedded_nul_invalid_gbk_and_missing_terminator() {
    for records in [
        vec![(11, &b"a\0"[..]), (11, &b"b\0"[..])],
        vec![(11, &b""[..])],
        vec![(11, &b"a\0b\0"[..])],
        vec![(11, &b"\x81\0"[..])],
        vec![(11, &b"abc"[..])],
    ] {
        let root = tempfile::tempdir().unwrap();
        let request = fixture(root.path(), &dictionary(&records));
        assert!(apply(request, Arc::new(AtomicBool::new(false)), |_| {}).is_err());
        assert!(!root.path().join("output").exists());
    }
}

#[test]
fn rejects_crc_corruption_and_truncated_dictionary_before_publication() {
    let valid = dictionary(&[(11, b"a\0")]);
    for truncated in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut dat = valid.clone();
        if truncated {
            dat.pop();
        } else {
            *dat.last_mut().unwrap() ^= 1;
        }
        let request = fixture(root.path(), &dat);
        let error = apply(request, Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap();
        assert_eq!(
            error.kind(),
            if truncated {
                PatchErrorKind::Integrity
            } else {
                PatchErrorKind::Codec
            }
        );
        assert!(!root.path().join("output").exists());
    }
}
