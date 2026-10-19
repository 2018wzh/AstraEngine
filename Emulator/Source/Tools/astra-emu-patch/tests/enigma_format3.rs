//! Public, independently encoded format-3 directory/aPLib fixtures.
//! CRCs below are fixture expectations, not invented Enigma wire fields.
use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn u64le(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn utf16z(out: &mut Vec<u8>, value: &str) {
    for ch in value.encode_utf16().chain(std::iter::once(0)) {
        out.extend_from_slice(&ch.to_le_bytes());
    }
}
fn node(out: &mut Vec<u8>, id: u32, level: u32, children: u32, name: &str, kind: u8) {
    u32le(out, id);
    u32le(out, level);
    u32le(out, children);
    utf16z(out, name);
    out.push(kind);
}

// aPLib's first byte is a literal; each subsequent literal has tag bit 0.
// EOF is the 110 short-match branch with zero offset. Tags are MSB-first
// and interleaved with data, rather than a separate bitstream.
fn literal_aplib(plain: &[u8]) -> Vec<u8> {
    assert!(!plain.is_empty());
    struct Bits {
        bytes: Vec<u8>,
        tag: usize,
        used: u8,
    }
    impl Bits {
        fn bit(&mut self, value: bool) {
            if self.used == 8 {
                self.tag = self.bytes.len();
                self.bytes.push(0);
                self.used = 0;
            }
            if value {
                self.bytes[self.tag] |= 0x80 >> self.used;
            }
            self.used += 1;
        }
    }
    let mut bits = Bits {
        bytes: vec![plain[0]],
        tag: 0,
        used: 8,
    };
    for &byte in &plain[1..] {
        bits.bit(false);
        bits.bytes.push(byte);
    }
    bits.bit(true);
    bits.bit(true);
    bits.bit(false);
    bits.bytes.push(0);
    bits.bytes
}

struct Fixture {
    directory: Vec<u8>,
    payload: Vec<u8>,
    expected: Vec<Vec<u8>>,
    file_record: usize,
    size_at: usize,
    flags_at: usize,
    first_table: usize,
    first_packed: usize,
}
impl Fixture {
    fn new() -> Self {
        let expected = vec![
            (0..65537).map(|i| (i % 251) as u8).collect::<Vec<_>>(),
            Vec::new(),
            b"public tail".to_vec(),
        ];
        let mut directory = Vec::new();
        node(&mut directory, 0, 0, 1, "", 0);
        node(&mut directory, 1, 1, 3, "%DEFAULT FOLDER%", 3);
        directory.extend_from_slice(&[0; 29]);
        let mut payload = Vec::new();
        let mut file_record = 0;
        let mut size_at = 0;
        let mut flags_at = 0;
        for (index, plain) in expected.iter().enumerate() {
            if index == 0 {
                file_record = directory.len();
            }
            let name = ["公开.bin", "zero.bin", "tail.bin"][index];
            node(&mut directory, index as u32 + 2, 2, 0, name, 2);
            if index == 0 {
                flags_at = directory.len();
                size_at = directory.len() + 2;
            }
            directory.extend_from_slice(&[0, 0]);
            u64le(&mut directory, plain.len() as u64);
            directory.extend_from_slice(&[0; 29]);
            utf16z(&mut directory, "");
            u64le(&mut directory, payload.len() as u64);
            let blocks = plain.chunks(65536).map(literal_aplib).collect::<Vec<_>>();
            let table_size = blocks.len() * 12;
            let stored = table_size + blocks.iter().map(Vec::len).sum::<usize>();
            u64le(&mut directory, stored as u64);
            let mut offset = table_size;
            for block in &blocks {
                u64le(&mut payload, offset as u64);
                u32le(&mut payload, block.len() as u32);
                offset += block.len();
            }
            for block in blocks {
                payload.extend_from_slice(&block);
            }
        }
        Self {
            directory,
            payload,
            expected,
            file_record,
            size_at,
            flags_at,
            first_table: 0,
            first_packed: 24,
        }
    }

    fn apply(
        &self,
        root: &Path,
    ) -> Result<astra_emu_patch::PatchResult, astra_emu_patch::PatchError> {
        let source = root.join("source");
        let scripts = root.join("scripts");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(scripts.join("common")).unwrap();
        let mut input = self.directory.clone();
        input.extend_from_slice(&self.payload);
        fs::write(source.join("container.bin"), input).unwrap();
        fs::write(
            scripts.join("common/enigma_vfs.luau"),
            include_str!("../../../../Patches/common/enigma_vfs.luau"),
        )
        .unwrap();
        // Main owns the shared bounded reader. Use its production implementation,
        // never a test-only replacement of the binary or codec APIs.
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Patches/common/binary.luau"),
            scripts.join("common/binary.luau"),
        )
        .unwrap();
        let crc = self
            .expected
            .iter()
            .map(|v| crc32fast::hash(v).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let script = format!(
            r#"
return {{ id = "public-enigma-format3", version = 1, title = "Public format3 fixture",
 apply = function(ctx)
    local vfs = ctx.load("common/enigma_vfs.luau")
    local archive = vfs.directory(ctx, ctx.open("source", "container.bin"), {{
        directory_offset = 0, directory_length = {directory},
        payload_offset = {directory}, payload_length = {payload}
    }})
    assert(#archive.nodes == 5 and #archive.files == 3)
    assert(archive.files[1].name == "公开.bin")
    assert(archive.files[1].parent.name == "%DEFAULT FOLDER%")
    assert(#archive.files[1].blocks == 2 and archive.files[2].size == 0)
    local expected_crc = {{{crc}}}
    for index, file in ipairs(archive.files) do
        local out = ctx.create("decoded-" .. index .. ".bin")
        vfs.decode(ctx, archive, file, function(offset, bytes) out:write(offset, bytes) end)
        assert(out:size() == file.size)
        if ctx.hash.crc32(out:read(0, out:size())) ~= expected_crc[index] then
            ctx.fail("INTEGRITY", "public fixture independently expected CRC mismatch")
        end
        out:close()
    end
    return {{plugin_id = "public.enigma-fixture", configuration = {{}}}}
 end }}
"#,
            directory = self.directory.len(),
            payload = self.payload.len()
        );
        let script_path = scripts.join("fixture.luau");
        fs::write(&script_path, script).unwrap();
        apply(
            PatchRequest {
                script: script_path,
                source: source.clone(),
                patch: source,
                output: root.join("output"),
            },
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
    }
}

#[test]
fn format3_decodes_all_blocks_and_empty_file_with_independent_bytes_and_crc() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new();
    let result = fixture.apply(root.path()).unwrap();
    for (index, expected) in fixture.expected.iter().enumerate() {
        assert_eq!(
            fs::read(result.output.join(format!("decoded-{}.bin", index + 1))).unwrap(),
            *expected
        );
    }
    let mut original = fixture.directory.clone();
    original.extend_from_slice(&fixture.payload);
    assert_eq!(
        fs::read(root.path().join("source/container.bin")).unwrap(),
        original
    );
}

fn rejects(fixture: Fixture, expected: PatchErrorKind) {
    let root = tempfile::tempdir().unwrap();
    let error = fixture.apply(root.path()).err().unwrap();
    assert_eq!(error.kind(), expected);
    assert!(!root.path().join("output").exists());
}

#[test]
fn format3_rejects_truncated_node_record() {
    let mut f = Fixture::new();
    f.directory.truncate(f.directory.len() - 1);
    rejects(f, PatchErrorKind::Bounds);
}
#[test]
fn format3_rejects_duplicate_identity() {
    let mut f = Fixture::new();
    f.directory[f.file_record..f.file_record + 4].copy_from_slice(&1_u32.to_le_bytes());
    rejects(f, PatchErrorKind::Integrity);
}
#[test]
fn format3_rejects_invalid_level() {
    let mut f = Fixture::new();
    f.directory[f.file_record + 4..f.file_record + 8].copy_from_slice(&9_u32.to_le_bytes());
    rejects(f, PatchErrorKind::Integrity);
}
#[test]
fn format3_rejects_unsupported_flags_instead_of_decoding_guessed_semantics() {
    let mut f = Fixture::new();
    f.directory[f.flags_at] = 1;
    rejects(f, PatchErrorKind::UnsupportedInput);
}
#[test]
fn format3_rejects_unrepresentable_expanded_length() {
    let mut f = Fixture::new();
    f.directory[f.size_at..f.size_at + 8].copy_from_slice(&(1_u64 << 53).to_le_bytes());
    rejects(f, PatchErrorKind::Bounds);
}
#[test]
fn format3_rejects_block_pointing_inside_its_index() {
    let mut f = Fixture::new();
    f.payload[f.first_table..f.first_table + 8].copy_from_slice(&0_u64.to_le_bytes());
    rejects(f, PatchErrorKind::Integrity);
}
#[test]
fn format3_rejects_truncated_block_extent() {
    let mut f = Fixture::new();
    f.payload.pop();
    rejects(f, PatchErrorKind::Bounds);
}
#[test]
fn aplib_rejects_missing_terminator_at_actual_decoder_boundary() {
    let mut f = Fixture::new();
    // First packed block's final byte is the zero short-match offset.
    let length = u32::from_le_bytes(f.payload[8..12].try_into().unwrap()) as usize;
    f.payload[f.first_packed + length - 1] = 2;
    rejects(f, PatchErrorKind::Codec);
}
#[test]
fn decoded_crc_mismatch_is_not_treated_as_conversion_success() {
    let mut f = Fixture::new();
    f.payload[f.first_packed] ^= 1;
    rejects(f, PatchErrorKind::Integrity);
}

#[test]
fn aplib_rejects_incorrect_declared_output_length() {
    let mut f = Fixture::new();
    f.directory[f.size_at..f.size_at + 8].copy_from_slice(&65538_u64.to_le_bytes());
    rejects(f, PatchErrorKind::Codec);
}
