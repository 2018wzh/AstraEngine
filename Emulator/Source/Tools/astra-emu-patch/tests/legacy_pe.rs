use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

// Every vector passes through the public transaction API and actual Luau context.
// All PE bytes below are handcrafted data, never loaded as executable images.
fn exercise(bytes: &[u8], body: &str, expected: Option<PatchErrorKind>, cancel: bool) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let patch = root.path().join("patch");
    let scripts = root.path().join("scripts");
    let output = root.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch).unwrap();
    fs::create_dir_all(scripts.join("common")).unwrap();
    fs::write(source.join("input.bin"), bytes).unwrap();
    fs::write(source.join("keep.bin"), b"public independent output").unwrap();
    fs::write(
        scripts.join("common/binary.luau"),
        include_str!("../../../../Metadata/patches/common/binary.luau"),
    )
    .unwrap();
    fs::write(
        scripts.join("common/pe.luau"),
        include_str!("../../../../Metadata/patches/common/pe.luau"),
    )
    .unwrap();
    let script = scripts.join("entry.luau");
    fs::write(
        &script,
        format!(
            r#"
return {{ id="public-legacy-pe", version=1, title="Public legacy PE vectors", apply=function(ctx)
    local binary = ctx.load("common/binary.luau")
    local pe = ctx.load("common/pe.luau")
    local source = ctx.open("source", "input.bin")
    {body}
    ctx.copy("source", "keep.bin", "keep.bin")
    return {{plugin_id="public.fixture",configuration={{}}}}
end }}
"#
        ),
    )
    .unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let result = apply(
        PatchRequest {
            script,
            source: source.clone(),
            patch,
            output: output.clone(),
        },
        cancelled,
        move |event| {
            if cancel && event.phase == "cancel inside helper" {
                flag.store(true, Ordering::Release);
            }
        },
    );
    if let Some(kind) = expected {
        assert_eq!(
            result
                .err()
                .expect("malformed vector unexpectedly succeeded")
                .kind(),
            kind
        );
        assert!(!output.exists());
    } else {
        let result = result.unwrap();
        assert_eq!(
            fs::read(result.output.join("keep.bin")).unwrap(),
            b"public independent output"
        );
    }
    assert_eq!(fs::read(source.join("input.bin")).unwrap(), bytes);
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn section_table(plus: bool) -> usize {
    if plus {
        0x188
    } else {
        0x178
    }
}
fn directory_table(plus: bool) -> usize {
    if plus {
        0x108
    } else {
        0xf8
    }
}

fn image(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x520];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 0x84, if plus { 0x8664 } else { 0x14c });
    put16(&mut bytes, 0x86, 2);
    put16(&mut bytes, 0x94, if plus { 0xf0 } else { 0xe0 });
    put16(&mut bytes, 0x98, if plus { 0x20b } else { 0x10b });
    if plus {
        bytes[0xb0..0xb8].copy_from_slice(&0x1_4000_0000_u64.to_le_bytes());
    } else {
        put32(&mut bytes, 0xb4, 0x400000);
    }
    put32(&mut bytes, 0xb8, 0x1000);
    put32(&mut bytes, 0xbc, 0x200);
    put32(&mut bytes, 0xd0, 0x3000);
    put32(&mut bytes, 0xd4, 0x200);
    let dirs = directory_table(plus);
    put32(&mut bytes, dirs - 4, 16);
    put32(&mut bytes, dirs + 16, 0x1000);
    put32(&mut bytes, dirs + 20, 0x120);
    // Security is a physical range outside all sections, not an RVA/overlay extension.
    put32(&mut bytes, dirs + 32, 0x500);
    put32(&mut bytes, dirs + 36, 0x10);
    let sections = section_table(plus);
    for (at, name, rva, virtual_size, offset, size) in [
        (sections, b".rsrc\0\0\0", 0x1000, 0x300, 0x200, 0x200),
        (sections + 40, b".data\0\0\0", 0x2000, 0x100, 0x400, 0x100),
    ] {
        bytes[at..at + 8].copy_from_slice(name);
        put32(&mut bytes, at + 8, virtual_size);
        put32(&mut bytes, at + 12, rva);
        put32(&mut bytes, at + 16, size);
        put32(&mut bytes, at + 20, offset);
        put32(&mut bytes, at + 36, 0x40000040);
    }
    // Original order: named type/name/Chinese-language, then numeric type/name/language.
    for (relative, named, ids) in [
        (0, 1, 1),
        (0x30, 1, 0),
        (0x60, 0, 1),
        (0x90, 0, 1),
        (0xb0, 0, 1),
    ] {
        put16(&mut bytes, 0x200 + relative + 12, named);
        put16(&mut bytes, 0x200 + relative + 14, ids);
    }
    for (relative, name, target) in [
        (0x10, 0x800000d0, 0x80000030),
        (0x18, 10, 0x80000060),
        (0x40, 0x800000e0, 0x80000090),
        (0x70, 7, 0x800000b0),
        (0xa0, 0x804, 0x100),
        (0xc0, 1033, 0x110),
    ] {
        put32(&mut bytes, 0x200 + relative, name);
        put32(&mut bytes, 0x204 + relative, target);
    }
    for (relative, text) in [(0xd0, "类型😀"), (0xe0, "名")] {
        let units: Vec<_> = text.encode_utf16().collect();
        put16(&mut bytes, 0x200 + relative, units.len() as u16);
        for (index, unit) in units.into_iter().enumerate() {
            put16(&mut bytes, 0x202 + relative + index * 2, unit);
        }
    }
    for (relative, rva, size, codepage) in [(0x100, 0x2004, 4, 65001), (0x110, 0x2010, 3, 932)] {
        put32(&mut bytes, 0x200 + relative, rva);
        put32(&mut bytes, 0x204 + relative, size);
        put32(&mut bytes, 0x208 + relative, codepage);
    }
    bytes[0x404..0x408].copy_from_slice(b"DATA");
    bytes[0x410..0x413].copy_from_slice(b"xyz");
    bytes[0x500..0x520].fill(b'O');
    bytes
}

#[test]
fn binary_reader_exact_endian_signed_and_safe_integer_vectors() {
    let mut bytes = vec![0; 48];
    bytes[..8].copy_from_slice(&[0x12, 0x34, 0x56, 0x78, 0xff, 0xff, 0xff, 0xff]);
    bytes[8..16].copy_from_slice(&9007199254740991_u64.to_le_bytes());
    bytes[16..24].copy_from_slice(&9007199254740992_u64.to_le_bytes());
    bytes[24..29].copy_from_slice(b"text\0");
    exercise(
        &bytes,
        r#"
    local r = binary.reader(ctx, source)
    assert(r:size() == 48)
    r:range(48, 0)
    assert(r:u8(0) == 0x12 and r:u16le(0) == 0x3412 and r:u16be(0) == 0x1234)
    assert(r:u32le(0) == 0x78563412 and r:u32be(0) == 0x12345678 and r:i32le(4) == -1)
    assert(r:u64le(8) == 9007199254740991)
    assert(buffer.tostring(r:cstring(24, 5)) == "text")
    assert(buffer.len(r:cstring(28, 16)) == 0)
    assert(buffer.len(r:bytes(48, 0)) == 0)
    assert(binary.add(9007199254740990, 1) == 9007199254740991)
    assert(binary.align(9, 6) == 12 and binary.align(12, 6) == 12)
    assert(binary.align(9007199254740990, 3) == 9007199254740990)
    for _, f in {
        function() r:range(-1, 0) end, function() r:range(0.5, 1) end,
        function() r:range(48, 1) end, function() r:range(0, math.huge) end,
        function() r:u64le(16) end, function() r:u32le(46) end,
        function() r:cstring(24, 4) end, function() r:cstring(24, 0) end,
        function() r:cstring(0, 16777217) end,
        function() binary.add(9007199254740991, 1) end,
        function() binary.add(9007199254740991, 0.5) end,
        function() binary.add(0/0, 1) end,
        function() binary.align(1, 0) end, function() binary.align(1, 0.5) end,
        function() binary.align(9007199254740991, 2) end,
    } do assert(not pcall(f)) end
    "#,
        None,
        false,
    );
}

#[test]
fn binary_read_limit_and_chunked_cstring_are_exact() {
    let mut bytes = vec![b'x'; 16 * 1024 * 1024 + 1];
    bytes[65536 + 3] = 0;
    exercise(
        &bytes,
        r#"
    local r = binary.reader(ctx, source)
    assert(buffer.len(r:bytes(0, 16777216)) == 16777216)
    assert(not pcall(function() r:bytes(0, 16777217) end))
    assert(buffer.len(r:cstring(0, 65540)) == 65539)
    assert(not pcall(function() r:cstring(0, 65539) end))
    "#,
        None,
        false,
    );
}

#[test]
fn pe32_and_pe32plus_preserve_sections_names_resources_and_overlay() {
    for plus in [false, true] {
        let body = format!(
            r#"
        local p = pe.parse(ctx, source)
        assert(p.machine == {} and p.base == {})
        assert(p.image_size == 0x3000 and p.headers_size == 0x200 and p.overlay == 0x500)
        assert(p.reader:size() == 0x520 and #p.sections == 2 and #p.directories == 16)
        local s = p.sections[1]
        assert(s.name == ".rsrc" and s.rva == 0x1000 and s.virtual_size == 0x300)
        assert(s.offset == 0x200 and s.size == 0x200 and s.flags == 0x40000040)
        assert(p.sections[2].name == ".data")
        assert(p.directories[3].rva == 0x1000 and p.directories[3].size == 0x120)
        assert(p.directories[5].rva == 0x500 and p.directories[5].size == 16)
        assert(pe.offset(p, 0x40, 4) == 0x40)
        assert(pe.offset(p, 0x1000, 0x200) == 0x200)
        assert(pe.offset(p, 0x2004, 4) == 0x404 and pe.offset(p, 0x2010, 0) == 0x410)
        local entries = pe.resources(ctx, p)
        assert(#entries == 2)
        local a, b = entries[1], entries[2]
        assert(#a.path == 3 and a.path[1] == "类型😀" and a.path[2] == "名" and a.path[3] == 0x804)
        assert(a.offset == 0x404 and a.size == 4 and a.codepage == 65001 and a.data_rva == 0x2004)
        assert(buffer.tostring(p.reader:bytes(a.offset,a.size)) == "DATA")
        assert(#b.path == 3 and b.path[1] == 10 and b.path[2] == 7 and b.path[3] == 1033)
        assert(b.offset == 0x410 and b.size == 3 and b.codepage == 932 and b.data_rva == 0x2010)
        assert(buffer.tostring(p.reader:bytes(b.offset,b.size)) == "xyz")
        "#,
            if plus { 0x8664 } else { 0x14c },
            if plus { 0x1_4000_0000_u64 } else { 0x400000 }
        );
        exercise(&image(plus), &body, None, false);
    }
}

#[test]
fn malformed_rvas_bss_and_partial_overlaps_reject() {
    for (rva, length) in [
        (0x1200_u64, 1_u64),
        (0x1300, 0),
        (0x1fff, 2),
        (0x2100, 1),
        (0xffffffff, 2),
        (0x100000000, 0),
        (0x1ff, 2),
        (0x11ff, 2),
    ] {
        exercise(
            &image(false),
            &format!("local p=pe.parse(ctx,source); pe.offset(p,{rva},{length})"),
            Some(PatchErrorKind::Bounds),
            false,
        );
    }
    let mut bytes = image(false);
    put32(&mut bytes, section_table(false) + 40 + 12, 0x1100);
    // Query begins before the second section but intersects it halfway through.
    exercise(
        &bytes,
        "local p=pe.parse(ctx,source); pe.offset(p,0x10f0,32)",
        Some(PatchErrorKind::Bounds),
        false,
    );
    exercise(
        &bytes,
        "local p=pe.parse(ctx,source); pe.offset(p,0x1100,1)",
        Some(PatchErrorKind::Bounds),
        false,
    );
    let mut bytes = image(false);
    put32(&mut bytes, section_table(false) + 12, 0x100);
    exercise(
        &bytes,
        "local p=pe.parse(ctx,source); pe.offset(p,0x100,1)",
        Some(PatchErrorKind::Bounds),
        false,
    );
}

#[test]
fn malformed_headers_overflow_and_truncation_reject() {
    for length in [0, 63, 0x97, 0x180, 0x4ff] {
        let bytes = image(false);
        exercise(
            &bytes[..length],
            "pe.parse(ctx,source)",
            Some(PatchErrorKind::Bounds),
            false,
        );
    }
    for (at, value) in [
        (0x3c, 0xfffffff0),
        (0xd4, 0x100),
        (directory_table(false) - 4, 17),
        (section_table(false) + 12, 0xffffff00),
        (section_table(false) + 20, 0xffffff00),
        (directory_table(false) + 16, 0xfffffff0),
    ] {
        let mut bytes = image(false);
        put32(&mut bytes, at, value);
        exercise(
            &bytes,
            "pe.parse(ctx,source)",
            Some(PatchErrorKind::Bounds),
            false,
        );
    }
    let mut bytes = image(false);
    put16(&mut bytes, 0x94, 2);
    exercise(
        &bytes,
        "pe.parse(ctx,source)",
        Some(PatchErrorKind::Bounds),
        false,
    );
    let mut bytes = image(true);
    bytes[0xb0..0xb8].copy_from_slice(&9007199254740992_u64.to_le_bytes());
    exercise(
        &bytes,
        "pe.parse(ctx,source)",
        Some(PatchErrorKind::Bounds),
        false,
    );
    let mut bytes = image(false);
    put16(&mut bytes, 0x98, 0x107);
    exercise(
        &bytes,
        "pe.parse(ctx,source)",
        Some(PatchErrorKind::UnsupportedInput),
        false,
    );
}

#[test]
fn malformed_resource_trees_ranges_cycles_and_utf16_reject() {
    for (at, value) in [
        (0x214, 0x80000000),
        (0x244, 0x80000030),
        (0x214, 0x80000118),
        (0x210, 0x8000011f),
        (0x2a4, 0x118),
        (0x300, 0x1200),
        (0x304, 0xffffffff),
        (directory_table(false) + 20, 0x201),
    ] {
        let mut bytes = image(false);
        put32(&mut bytes, at, value);
        exercise(
            &bytes,
            "pe.resources(ctx,pe.parse(ctx,source))",
            Some(PatchErrorKind::Bounds),
            false,
        );
    }
    let mut bytes = image(false);
    put16(&mut bytes, 0x20e, 0xffff);
    exercise(
        &bytes,
        "pe.resources(ctx,pe.parse(ctx,source))",
        Some(PatchErrorKind::Bounds),
        false,
    );
    let mut bytes = image(false);
    put16(&mut bytes, 0x2d0, 0xffff);
    exercise(
        &bytes,
        "pe.resources(ctx,pe.parse(ctx,source))",
        Some(PatchErrorKind::Bounds),
        false,
    );
    for unit in [0x0041, 0xd800] {
        let mut bytes = image(false);
        put16(&mut bytes, 0x2d8, unit);
        exercise(
            &bytes,
            "pe.resources(ctx,pe.parse(ctx,source))",
            Some(PatchErrorKind::Codec),
            false,
        );
    }
}

#[test]
fn cancellation_reaches_reader_and_resource_helpers_in_real_context() {
    exercise(
        &vec![b'x'; 131072],
        r#"
    local r=binary.reader(ctx,source)
    ctx.progress("cancel inside helper",0,1)
    r:cstring(0,131072)
    "#,
        Some(PatchErrorKind::Cancelled),
        true,
    );
    exercise(
        &image(true),
        r#"
    local p=pe.parse(ctx,source)
    ctx.progress("cancel inside helper",0,1)
    pe.resources(ctx,p)
    "#,
        Some(PatchErrorKind::Cancelled),
        true,
    );
}
