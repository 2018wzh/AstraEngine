use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

const ACR_MODULE: &str = include_str!("../../../../Metadata/patches/common/musica_acr.luau");

fn run(
    root: &Path,
    input: &[u8],
    body: &str,
) -> Result<astra_emu_patch::PatchResult, astra_emu_patch::PatchError> {
    let source = root.join("source");
    let scripts = root.join("scripts");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(scripts.join("common")).unwrap();
    fs::write(source.join("public.bin"), input).unwrap();
    fs::write(scripts.join("common/musica_acr.luau"), ACR_MODULE).unwrap();
    let script = scripts.join("public.luau");
    fs::write(&script, format!(r#"return {{ id = "public-musica-legacy", version = 1, title = "Public legacy data fixture", apply = function(ctx)
        local legacy = ctx.load("common/musica_acr.luau")
        {body}
        return {{plugin_id = "astra.emu.musica", configuration = {{}}}}
    end }}"#)).unwrap();
    let result = apply(
        PatchRequest {
            script,
            source: source.clone(),
            patch: source.clone(),
            output: root.join("output"),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );
    assert_eq!(fs::read(source.join("public.bin")).unwrap(), input);
    result
}

fn hash(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .copied()
        .filter(|b| *b != 0)
        .fold(0u32, |h, b| h.wrapping_mul(131).wrapping_add(b as u32))
        & 0x7fff_ffff
}

fn acr(records: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0u8; 16 + records.len() * 20];
    bytes[0..4].copy_from_slice(&(records.len() as u32).to_le_bytes());
    for (i, (original, translated)) in records.iter().enumerate() {
        let source = bytes.len() as u32;
        bytes.extend_from_slice(original);
        let target = bytes.len() as u32;
        bytes.extend_from_slice(translated);
        for (j, value) in [
            hash(original),
            source,
            original.len() as u32,
            target,
            translated.len() as u32,
        ]
        .iter()
        .enumerate()
        {
            bytes[16 + i * 20 + j * 4..20 + i * 20 + j * 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    let stored = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&stored.to_le_bytes());
    bytes[12..16].copy_from_slice(&stored.to_le_bytes());
    bytes
}

const CONVERT: &str = r#"
    local records = legacy.read(ctx, ctx.open("source", "public.bin"))
    local result = legacy.translation_table(ctx, {}, records,
        {source="gbk",names="gbk",dialogue="cp932"})
    local out = ctx.create("table.json")
    out:write(0, buffer.fromstring(ctx.json.encode(result)))
    out:close()
"#;

#[test]
fn acr_absolute_offsets_and_original_identity_preserve_ordered_native_rows() {
    let root = tempfile::tempdir().unwrap();
    let input = acr(&[(b"original", b"translated"), (b"original", b"translated")]);
    let result = run(root.path(), &input, CONVERT).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("table.json")).unwrap()).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(value["source_encoding"], "gbk");
    assert_eq!(value["hash_collision_policy"], "first_record");
    assert_eq!(value["dialogue"].as_array().unwrap().len(), 2);
    assert_eq!(value["dialogue"][0]["source_hash"], hash(b"original"));
    assert_eq!(value["dialogue"][0]["text"], "translated");
    assert_eq!(value["dialogue"][0], value["dialogue"][1]);
    assert_eq!(
        value["dialogue"][0]["source_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn rolling_hash_ignores_nuls_and_wraps_exactly_at_31_bits() {
    let root = tempfile::tempdir().unwrap();
    run(
        root.path(),
        b"",
        &format!(
            r#"
        assert(legacy.source_hash(buffer.fromstring("a\0b\0c")) == {})
        assert(legacy.source_hash(buffer.fromstring(string.rep("z", 200))) == {})
    "#,
            hash(b"abc"),
            hash(&[b'z'; 200])
        ),
    )
    .unwrap();
}

#[test]
fn acr_retains_real_hash_collision_with_distinct_original_identity_audit() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(hash(&[1, 132]), hash(&[2, 1]));
    let input = acr(&[(&[1, 132], b"one"), (&[2, 1], b"two")]);
    let result = run(root.path(), &input, r#"
        local records=legacy.read(ctx,ctx.open("source","public.bin"))
        local result,audit=legacy.translation_table(ctx,{},records,
            {source="gbk",names="gbk",dialogue="cp932"})
        assert(audit.different_identity_collisions==1)
        assert(#result.dialogue==2 and result.dialogue[1].text=="one" and result.dialogue[2].text=="two")
        assert(result.dialogue[1].source_hash==result.dialogue[2].source_hash)
        assert(result.dialogue[1].source_sha256~=result.dialogue[2].source_sha256)
        local out=ctx.create("table.json");out:write(0,buffer.fromstring(ctx.json.encode(result)));out:close()
    "#).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("table.json")).unwrap()).unwrap();
    assert_eq!(value["dialogue"].as_array().unwrap().len(), 2);
}

#[test]
fn acr_retains_conflicting_duplicates_but_refuses_mutated_source_bytes() {
    let root = tempfile::tempdir().unwrap();
    let input = acr(&[(b"original", b"one"), (b"original", b"two")]);
    let result = run(root.path(), &input, CONVERT).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("table.json")).unwrap()).unwrap();
    assert_eq!(value["dialogue"][0]["text"], "one");
    assert_eq!(value["dialogue"][1]["text"], "two");
    let rejected = tempfile::tempdir().unwrap();
    assert_eq!(
        run(
            rejected.path(),
            &input,
            r#"
            local records=legacy.read(ctx,ctx.open("source","public.bin"))
            buffer.writeu8(records[1].source,0,0)
            legacy.translation_table(ctx,{},records,{source="gbk",names="gbk",dialogue="cp932"})
        "#
        )
        .err()
        .unwrap()
        .kind(),
        PatchErrorKind::Integrity
    );
    assert!(!rejected.path().join("output").exists());
}

#[test]
fn names_and_dialogue_follow_their_different_native_codepages() {
    let root = tempfile::tempdir().unwrap();
    let input = acr(&[
        (b"speaker", b"\xd6\xd0\xce\xc4"),
        (b"message", b"\x93\xfa\x96\x7b\x8c\xea"),
    ]);
    run(
        root.path(),
        &input,
        r#"
        local rows=legacy.read(ctx,ctx.open("source","public.bin"))
        local result=legacy.translation_table(ctx,{rows[1]},{rows[2]},
            {source="gbk",names="gbk",dialogue="cp932"})
        assert(result.names[1].text=="中文" and result.dialogue[1].text=="日本語")
    "#,
    )
    .unwrap();
}

#[test]
fn acr_rejects_range_hash_and_header_corruption_separately() {
    for corruption in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let mut input = acr(&[(b"original", b"translated")]);
        match corruption {
            0 => input[20..24].copy_from_slice(&u32::MAX.to_le_bytes()),
            1 => input[16] ^= 1,
            _ => input[8] ^= 1,
        }
        let error = run(root.path(), &input, CONVERT).err().unwrap();
        assert_eq!(
            error.kind(),
            if corruption == 0 {
                PatchErrorKind::Bounds
            } else {
                PatchErrorKind::Integrity
            }
        );
        assert!(!root.path().join("output").exists());
    }
}

fn cnbg(name: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 24];
    bytes[..8].copy_from_slice(b"FUYINPAK");
    bytes[8..12].copy_from_slice(&24u32.to_le_bytes());
    let name_at = bytes.len() as u32;
    bytes.extend_from_slice(name);
    let bmp_at = bytes.len() as u32;
    // Public two-pixel, bottom-up BGRA bitmap, opaque red followed by green.
    let mut bmp = vec![0u8; 62];
    bmp[0..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&62u32.to_le_bytes());
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&2i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
    bmp[34..38].copy_from_slice(&8u32.to_le_bytes());
    bmp[54..].copy_from_slice(&[0, 0, 255, 255, 0, 255, 0, 255]);
    bytes.extend_from_slice(&bmp);
    let index = bytes.len() as u32;
    for value in [
        name_at,
        name.len() as u32,
        bmp_at,
        bmp.len() as u32,
        bmp.len() as u32,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes[16..20].copy_from_slice(&index.to_le_bytes());
    bytes[20..24].copy_from_slice(&20u32.to_le_bytes());
    bytes
}

#[test]
fn cnbg_uses_cp932_resource_path_and_converts_real_bmp_pixels() {
    let root = tempfile::tempdir().unwrap();
    let input = cnbg(b"ui/\x83\x65\x83\x58\x83\x67.bmp\0");
    let result = run(
        root.path(),
        &input,
        r#"
        local source = ctx.open("source", "public.bin")
        local entries = legacy.cnbg_index(ctx, source)
        assert(#entries == 1 and entries[1].resource == "ui/テスト.png")
        local png = legacy.cnbg_png(ctx, source, entries[1], {width=2, height=1})
        local out = ctx.create("override.png"); out:write(0, png); out:close()
    "#,
    )
    .unwrap();
    let image = image::load_from_memory(&fs::read(result.output.join("override.png")).unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 255]);
}

#[test]
fn cnbg_rejects_traversal_truncation_and_wrong_original_dimensions() {
    for case in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let mut input = cnbg(if case == 0 {
            b"../image.bmp"
        } else {
            b"image.bmp"
        });
        if case == 1 {
            input.pop();
        }
        let error = run(
            root.path(),
            &input,
            r#"
            local source = ctx.open("source", "public.bin")
            local entries = legacy.cnbg_index(ctx, source)
            legacy.cnbg_png(ctx, source, entries[1], {width=3, height=1})
        "#,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.kind(),
            match case {
                0 => PatchErrorKind::Path,
                1 => PatchErrorKind::Bounds,
                _ => PatchErrorKind::Integrity,
            }
        );
        assert!(!root.path().join("output").exists());
    }
}

#[test]
fn cnbg_preserves_zero_alpha_and_checks_pinned_crc_before_png_publication() {
    let mut input = cnbg(b"public.bmp");
    let index = u32::from_le_bytes(input[16..20].try_into().unwrap()) as usize;
    let bitmap = u32::from_le_bytes(input[index + 8..index + 12].try_into().unwrap()) as usize;
    input[bitmap + 57] = 0;
    input[bitmap + 61] = 31;
    let crc = crc32fast::hash(&input[bitmap..bitmap + 62]);
    for wrong_crc in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let result = run(
            root.path(),
            &input,
            &format!(
                r#"
            local source=ctx.open("source","public.bin")
            local entries=legacy.cnbg_index(ctx,source)
            local png=legacy.cnbg_png(ctx,source,entries[1],{{width=2,height=1,crc32={}}})
            local out=ctx.create("public.png");out:write(0,png);out:close()
        "#,
                crc ^ u32::from(wrong_crc)
            ),
        );
        if wrong_crc {
            assert_eq!(result.err().unwrap().kind(), PatchErrorKind::Integrity);
            assert!(!root.path().join("output").exists());
        } else {
            let image = image::open(result.unwrap().output.join("public.png"))
                .unwrap()
                .to_rgba8();
            assert_eq!(image.as_raw(), &[255, 0, 0, 0, 0, 255, 0, 31]);
        }
    }
}

#[test]
fn cnbg_rejects_header_overlap_declared_size_and_unproven_tail_variants() {
    for case in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let mut input = cnbg(b"public.bmp");
        let index = u32::from_le_bytes(input[16..20].try_into().unwrap()) as usize;
        let bitmap = u32::from_le_bytes(input[index + 8..index + 12].try_into().unwrap()) as usize;
        match case {
            0 => input[0] ^= 1,
            1 => input[index..index + 4].copy_from_slice(&(bitmap as u32).to_le_bytes()),
            2 => input[bitmap + 2] ^= 1,
            3 => input[bitmap + 34] ^= 1,
            _ => input.extend_from_slice(&[0, 0]),
        }
        let error = run(
            root.path(),
            &input,
            r#"
            local source=ctx.open("source","public.bin")
            local entries=legacy.cnbg_index(ctx,source)
            legacy.cnbg_png(ctx,source,entries[1],{width=2,height=1})
        "#,
        )
        .err()
        .unwrap();
        assert!(matches!(
            error.kind(),
            PatchErrorKind::UnsupportedInput | PatchErrorKind::Integrity
        ));
        assert!(!root.path().join("output").exists());
    }
}
