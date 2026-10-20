use flate2::{write::ZlibEncoder, Compression};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct PublicOverlay {
    pub source: PathBuf,
    pub patch: PathBuf,
    pub script: PathBuf,
    pub output: PathBuf,
    pub original: Vec<u8>,
    pub expected: Vec<u8>,
    pub header: Vec<u8>,
    pub blocks: Vec<(u32, u32, u32, Vec<u8>)>,
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn bmp(bgr: [u8; 3]) -> Vec<u8> {
    let mut bytes = vec![0; 58];
    bytes[..2].copy_from_slice(b"BM");
    put32(&mut bytes, 2, 58);
    put32(&mut bytes, 10, 54);
    put32(&mut bytes, 14, 40);
    put32(&mut bytes, 18, 1);
    put32(&mut bytes, 22, 1);
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    put32(&mut bytes, 34, 4);
    bytes[54..57].copy_from_slice(&bgr);
    bytes
}
fn slot(text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 32];
    bytes[..text.len()].copy_from_slice(text);
    bytes
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl PublicOverlay {
    pub fn new(root: &Path) -> Self {
        let source = root.join("public-source");
        let patch = root.join("public-patch");
        let script = root.join("public-overlay.luau");
        let output = root.join("public-output");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&patch).unwrap();
        fs::create_dir(root.join("common")).unwrap();
        fs::write(
            root.join("common/binary.luau"),
            include_str!("../../../../../Metadata/patches/common/binary.luau"),
        )
        .unwrap();
        fs::write(
            root.join("common/pe.luau"),
            include_str!("../../../../../Metadata/patches/common/pe.luau"),
        )
        .unwrap();
        let mut original = b"PUBLIC_ASSETS_V1".to_vec();
        assert_eq!(original.len(), 16);
        let source_parts = [
            slot(b"public first"),
            slot(b"public second"),
            bmp([0, 0, 255]),
            bmp([255, 0, 0]),
        ];
        let translated_parts = [
            slot("公开甲".as_bytes()),
            slot("公开乙".as_bytes()),
            bmp([0, 255, 0]),
            bmp([0, 255, 255]),
        ];
        let mut expected = original.clone();
        let mut offset = 16_u32;
        let mut blocks = Vec::new();
        for (source_part, translated) in source_parts.into_iter().zip(translated_parts) {
            assert_eq!(source_part.len(), translated.len());
            original.extend_from_slice(&source_part);
            expected.extend_from_slice(&translated);
            let mut compressed = ZlibEncoder::new(Vec::new(), Compression::default());
            compressed.write_all(&translated).unwrap();
            blocks.push((
                offset,
                translated.len() as u32,
                crc32fast::hash(&translated),
                compressed.finish().unwrap(),
            ));
            offset += translated.len() as u32;
        }
        fs::write(source.join("assets.bin"), &original).unwrap();
        fs::write(
            source.join("unchanged.dat"),
            b"complete independent public fixture",
        )
        .unwrap();
        fs::write(source.join("zero.dat"), []).unwrap();
        // Non-executable data-only x64 PE32+ image, one initialized-data section.
        // The recipe computes the overlay location from its section table.
        let mut header = vec![0; 1024];
        header[..2].copy_from_slice(b"MZ");
        put32(&mut header, 0x3c, 0x80);
        header[0x80..0x84].copy_from_slice(b"PE\0\0");
        header[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
        header[0x86..0x88].copy_from_slice(&1_u16.to_le_bytes());
        header[0x94..0x96].copy_from_slice(&0xf0_u16.to_le_bytes());
        header[0x96..0x98].copy_from_slice(&0x22_u16.to_le_bytes());
        header[0x98..0x9a].copy_from_slice(&0x20b_u16.to_le_bytes());
        header[0xb0..0xb8].copy_from_slice(&0x1_4000_0000_u64.to_le_bytes());
        put32(&mut header, 0xb8, 4096);
        put32(&mut header, 0xbc, 512);
        put32(&mut header, 0xd0, 8192);
        put32(&mut header, 0xd4, 512);
        header[0x188..0x18d].copy_from_slice(b".data");
        put32(&mut header, 0x190, 512);
        put32(&mut header, 0x194, 4096);
        put32(&mut header, 0x198, 512);
        put32(&mut header, 0x19c, 512);
        put32(&mut header, 0x1ac, 0x4000_0040);
        let fixture = Self {
            source,
            patch,
            script,
            output,
            original,
            expected,
            header,
            blocks,
        };
        fixture.write_patch();
        fixture.write_script();
        fixture
    }

    pub fn write_patch(&self) {
        let mut bytes = self.header.clone();
        bytes.extend_from_slice(b"APFX");
        bytes.extend_from_slice(&(self.blocks.len() as u32).to_le_bytes());
        for (offset, length, crc, compressed) in &self.blocks {
            for value in [*offset, *length, compressed.len() as u32, *crc] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(compressed);
        }
        fs::write(self.patch.join("overlay.exe"), bytes).unwrap();
    }

    pub fn write_script(&self) {
        let source_hash = digest(&fs::read(self.source.join("assets.bin")).unwrap());
        let patch_hash = digest(&fs::read(self.patch.join("overlay.exe")).unwrap());
        fs::write(
            &self.script,
            format!(
                r#"
return {{ id="public-pe-overlay", version=1, title="公开 PE overlay 夹具", apply=function(ctx)
    local source = ctx.open("source", "assets.bin")
    local patch = ctx.open("patch", "overlay.exe")
    if source:sha256() ~= "{source_hash}" or patch:sha256() ~= "{patch_hash}" then
        ctx.fail("UNSUPPORTED_INPUT", "public input fingerprint mismatch")
    end
    local pe = ctx.load("common/pe.luau")
    local image = pe.parse(ctx, patch)
    assert(image.machine == 0x8664 and image.base == 0x140000000)
    assert(#image.sections == 1 and image.sections[1].name == ".data")
    assert(#pe.resources(ctx, image) == 0 and image.overlay == 1024)
    local overlay = image.overlay
    local index = patch:read(overlay, 8)
    assert(buffer.readstring(index,0,4) == "APFX" and buffer.readu32(index,4) == 4)
    ctx.copy("source", "assets.bin", "assets.bin")
    ctx.copy("source", "unchanged.dat", "unchanged.dat")
    ctx.copy("source", "zero.dat", "zero.dat")
    local output = ctx.edit("assets.bin")
    local at = overlay + 8
    for i=1,4 do
        local row = patch:read(at,16)
        local offset = buffer.readu32(row,0)
        local size = buffer.readu32(row,4)
        local packed = buffer.readu32(row,8)
        local crc = buffer.readu32(row,12)
        at += 16
        local decoded = ctx.codec.inflate(patch:read(at,packed),size)
        at += packed
        if ctx.hash.crc32(decoded) ~= crc then ctx.fail("INTEGRITY","public block CRC mismatch") end
        if i >= 3 then
            local image = ctx.image.decode(decoded)
            assert(image.width == 1 and image.height == 1)
            local red = i == 3 and 0 or 255
            assert(buffer.readu8(image.rgba,0) == red and buffer.readu8(image.rgba,1) == 255)
            assert(buffer.readu8(image.rgba,2) == 0 and buffer.readu8(image.rgba,3) == 255)
        end
        output:write(offset,decoded)
        ctx.progress("patching public assets",i,4)
    end
    assert(at == patch:size())
    output:close()
    return {{plugin_id="public.fixture",configuration={{script_encoding="utf8"}}}}
end }}
"#
            ),
        )
        .unwrap();
    }

    pub fn assert_inputs(&self) {
        assert_eq!(
            fs::read(self.source.join("assets.bin")).unwrap(),
            self.original
        );
        assert_eq!(
            fs::read(self.source.join("unchanged.dat")).unwrap(),
            b"complete independent public fixture"
        );
        assert_eq!(fs::metadata(self.source.join("zero.dat")).unwrap().len(), 0);
    }
}
