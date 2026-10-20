use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use flate2::{write::ZlibEncoder, Compression};
use md5::{Digest, Md5};
use std::{
    fs,
    io::Write,
    sync::{atomic::AtomicBool, Arc},
};

const MODULE: &str = include_str!("../../../../Metadata/patches/common/molebox.luau");
const BINARY: &str = include_str!("../../../../Metadata/patches/common/binary.luau");
const SEED: &[u8; 16] = b"public-seed-1234";
const SCHEDULE: [usize; 60] = [
    11, 12, 13, 14, 8, 9, 10, 4, 5, 6, 7, 1, 2, 3, 12, 13, 14, 0, 9, 10, 11, 5, 6, 7, 8, 2, 3, 4,
    13, 14, 0, 1, 10, 11, 12, 6, 7, 8, 9, 3, 4, 5, 14, 0, 1, 2, 11, 12, 13, 7, 8, 9, 10, 4, 5, 6,
    0, 1, 2, 3,
];

// Public fixture encryption is the inverse of the recovered decode primitive.
// Its substitution is deliberately public and synthetic, never a shipped table.
fn encrypt_block(bytes: &mut [u8; 8]) {
    let key = SCHEDULE.map(|i| SEED[i]);
    let s = |value: u8| value ^ 0x5a;
    for i in 0..4 {
        bytes[i + 4] ^= s(bytes[i] ^ key[56 + i]);
    }
    for round in (0..8).rev() {
        let k = round * 7;
        bytes[0] ^= s(bytes[7] ^ key[k + 6]);
        bytes[3] ^= s(bytes[6] ^ key[k + 5]);
        bytes[2] ^= s(bytes[4] ^ bytes[5]);
        bytes[1] ^= s(bytes[4] ^ key[k + 4]);
        bytes[7] ^= s(bytes[3] ^ key[k + 3]);
        bytes[6] ^= s(bytes[2] ^ key[k + 2]);
        bytes[5] ^= s(bytes[1] ^ key[k + 1]);
        bytes[4] ^= s(bytes[0] ^ key[k]);
    }
}

fn encrypt(bytes: &[u8]) -> Vec<u8> {
    let mut output = bytes.to_vec();
    let mut previous = [0u8; 8];
    for block in output.as_chunks_mut::<8>().0 {
        let mut current = *block;
        for i in 0..8 {
            current[i] ^= previous[i];
        }
        encrypt_block(&mut current);
        block.copy_from_slice(&current);
        previous = current;
    }
    output
}

fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |mut crc, value| {
        crc ^= *value as u16;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xa001 } else { 0 };
        }
        crc
    })
}

fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

struct Fixture {
    bytes: Vec<u8>,
    expected: Vec<u8>,
    index: Vec<u8>,
    index_offset: usize,
    header_offset: usize,
    bad_index_crc: bool,
}

impl Fixture {
    fn new() -> Self {
        let blocks = [vec![b'A'; 49152], b"public!".to_vec()];
        let mut bytes = vec![0u8; 4];
        let mut index = vec![0u8; 136];
        let path = "SCENE.SC"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        index[20..36].copy_from_slice(&Md5::digest(path));
        u32_at(&mut index, 40, 7);
        u32_at(&mut index, 44, 49159);
        u32_at(&mut index, 48, 2);
        for (i, block) in blocks.iter().enumerate() {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(block).unwrap();
            let coded = encrypt(&encoder.finish().unwrap());
            let at = 52 + i * 12;
            index[at..at + 2].copy_from_slice(&(coded.len() as u16).to_le_bytes());
            index[at + 2..at + 4].copy_from_slice(&(block.len() as u16).to_le_bytes());
            index[at + 4..at + 6].copy_from_slice(&6u16.to_le_bytes());
            index[at + 6..at + 8].copy_from_slice(&crc16(block).to_le_bytes());
            u32_at(&mut index, at + 8, (bytes.len() - 4) as u32);
            bytes.extend_from_slice(&coded);
        }
        u32_at(&mut index, 76 + 40, 0x8000_0000);
        u32_at(&mut index, 76 + 48, 1);
        u32_at(&mut index, 76 + 52, 0);
        u32_at(&mut index, 132, 0);
        let index_offset = bytes.len();
        let mut fixture = Self {
            bytes,
            expected: blocks.concat(),
            index,
            index_offset,
            header_offset: index_offset + 136,
            bad_index_crc: false,
        };
        fixture.rebuild();
        fixture
    }

    fn rebuild(&mut self) {
        self.bytes.truncate(self.index_offset);
        self.bytes.extend_from_slice(&encrypt(&self.index));
        let mut header = vec![0u8; 48];
        u32_at(&mut header, 4, self.index.len() as u32);
        u32_at(&mut header, 8, 76);
        u32_at(&mut header, 12, self.header_offset as u32 - 4);
        u32_at(
            &mut header,
            16,
            crc32fast::hash(&self.index) ^ u32::from(self.bad_index_crc),
        );
        u32_at(&mut header, 20, 132);
        u32_at(&mut header, 24, 1);
        header[40..48].copy_from_slice(b"STELPACK");
        self.bytes.extend_from_slice(&encrypt(&header));
        u32_at(&mut self.bytes, 0, self.header_offset as u32);
    }

    fn apply(
        &self,
        root: &std::path::Path,
        body: &str,
    ) -> Result<astra_emu_patch::PatchResult, astra_emu_patch::PatchError> {
        let source = root.join("source");
        let scripts = root.join("scripts");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(scripts.join("common")).unwrap();
        fs::write(source.join("public.mys"), &self.bytes).unwrap();
        fs::write(scripts.join("common/molebox.luau"), MODULE).unwrap();
        fs::write(scripts.join("common/binary.luau"), BINARY).unwrap();
        let script = scripts.join("public.luau");
        fs::write(&script, format!(r#"return {{id="public-mys",version=1,title="Public STELPACK fixture",apply=function(ctx)
            local m = ctx.load("common/molebox.luau")
            local seed = buffer.fromstring("public-seed-1234")
            local substitution = buffer.create(256)
            for i=0,255 do buffer.writeu8(substitution,i,bit32.bxor(i,0x5a)) end
            {body}
            return {{plugin_id="astra.emu.musica",configuration={{}}}}
        end}}"#)).unwrap();
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
        assert_eq!(fs::read(source.join("public.mys")).unwrap(), self.bytes);
        result
    }
}

const EXTRACT: &str = r#"
    local package = m.open(ctx,ctx.open("source","public.mys"),seed,substitution)
    assert(#package.nodes==1 and package.block_count==2)
    local mapping = m.bind_paths(ctx, package, {{path="scene.sc",role="scr",uppercase_utf16le=ctx.encoding.encode("SCENE.SC","utf16le")}})
    assert(mapping[package.nodes[1]].path=="scene.sc")
    local output=ctx.create("scene.sc")
    m.read_member(ctx,package,package.nodes[1],function(at,bytes) output:write(at,bytes) end)
    output:close()
"#;

#[test]
fn every_stelpack_block_is_decoded_crc_checked_and_written_in_order() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new();
    let result = fixture.apply(root.path(), EXTRACT).unwrap();
    assert_eq!(
        fs::read(result.output.join("scene.sc")).unwrap(),
        fixture.expected
    );
}

#[test]
fn second_block_crc_failure_rolls_back_the_already_written_first_block() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    fixture.index[52 + 12 + 6] ^= 1;
    fixture.rebuild();
    assert_eq!(
        fixture.apply(root.path(), EXTRACT).err().unwrap().kind(),
        PatchErrorKind::Integrity
    );
    assert!(!root.path().join("output").exists());
}

#[test]
fn index_crc_lookup_set_and_payload_range_fail_at_distinct_boundaries() {
    for case in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = Fixture::new();
        match case {
            0 => fixture.bad_index_crc = true,
            1 => u32_at(&mut fixture.index, 132, 4),
            _ => u32_at(&mut fixture.index, 52 + 8, u32::MAX),
        }
        fixture.rebuild();
        assert_eq!(
            fixture.apply(root.path(), EXTRACT).err().unwrap().kind(),
            if case == 2 {
                PatchErrorKind::Bounds
            } else {
                PatchErrorKind::Integrity
            }
        );
        assert!(!root.path().join("output").exists());
    }
}

#[test]
fn declared_zlib_length_is_exact_and_missing_path_identity_is_a_hard_gate() {
    for case in 0..2 {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = Fixture::new();
        let body = if case == 0 {
            u32_at(&mut fixture.index, 44, 49158);
            fixture.index[52 + 12 + 2..52 + 12 + 4].copy_from_slice(&6u16.to_le_bytes());
            EXTRACT
        } else {
            r#"local package=m.open(ctx,ctx.open("source","public.mys"),seed,substitution)
                m.bind_paths(ctx,package,{{path="other.sc",uppercase_utf16le=ctx.encoding.encode("OTHER.SC","utf16le")}})"#
        };
        fixture.rebuild();
        assert_eq!(
            fixture.apply(root.path(), body).err().unwrap().kind(),
            if case == 0 {
                PatchErrorKind::Codec
            } else {
                PatchErrorKind::UnsupportedInput
            }
        );
        assert!(!root.path().join("output").exists());
    }
}

#[test]
fn loader_lz_preserves_literal_run_escape_and_bounded_back_reference() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new();
    fixture.apply(root.path(), r#"
        local coded=buffer.fromstring("\0\14abcdefghijklmno\128P\3\0")
        local decoded,consumed=m.loader_lz(ctx,coded,19)
        assert(buffer.tostring(decoded)=="abcdefghijklmnoPbcd" and consumed==21)
        local bytes=buffer.fromstring("public-tail")
        local decoded=m.decode(ctx,bytes,seed,substitution,true)
        assert(buffer.readu8(decoded,8)==string.byte("a") and buffer.readu8(decoded,10)==string.byte("l"))
    "#).unwrap();
}

#[test]
fn xor_bitmap_restores_bgra_channels_bottom_up_rows_and_original_alpha() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new();
    let result = fixture
        .apply(
            root.path(),
            r#"
        local bmp=buffer.create(62)
        buffer.writeu16(bmp,0,0x4d42)
        buffer.writeu32(bmp,10,54);buffer.writeu32(bmp,14,40)
        buffer.writeu32(bmp,18,1);buffer.writeu32(bmp,22,2)
        buffer.writeu16(bmp,26,1);buffer.writeu16(bmp,28,32)
        buffer.copy(bmp,54,buffer.fromstring(string.char(16,32,48,0,1,2,3,0)))
        local original=ctx.image.png(1,2,buffer.fromstring(string.char(10,20,30,40,50,60,70,80)))
        local png=m.xor_bitmap_png(ctx,bmp,original)
        local output=ctx.create("restored.png");output:write(0,png);output:close()
    "#,
        )
        .unwrap();
    let image = image::load_from_memory(&fs::read(result.output.join("restored.png")).unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(image.dimensions(), (1, 2));
    assert_eq!(
        image.as_raw(),
        &[10 ^ 3, 20 ^ 2, 30 ^ 1, 40, 50 ^ 48, 60 ^ 32, 70 ^ 16, 80]
    );
}

#[test]
fn xor_bitmap_refuses_unmatched_original_or_truncated_pixels() {
    for truncated in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::new();
        let body = format!(
            r#"
            local bmp=buffer.create({})
            buffer.writeu16(bmp,0,0x4d42)
            buffer.writeu32(bmp,10,54);buffer.writeu32(bmp,14,40)
            buffer.writeu32(bmp,18,1);buffer.writeu32(bmp,22,2)
            buffer.writeu16(bmp,26,1);buffer.writeu16(bmp,28,32)
            local original=ctx.image.png(1,1,buffer.fromstring(string.char(1,2,3,255)))
            m.xor_bitmap_png(ctx,bmp,original)
        "#,
            if truncated { 61 } else { 62 }
        );
        assert_eq!(
            fixture.apply(root.path(), &body).err().unwrap().kind(),
            PatchErrorKind::Integrity
        );
        assert!(!root.path().join("output").exists());
    }
}

#[test]
fn catalog_uses_length_then_crc_footer_and_preserves_package_seed_privately() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    let mut catalog = vec![0u8; 64];
    u32_at(&mut catalog, 8, 16);
    u32_at(&mut catalog, 12, 1);
    u32_at(&mut catalog, 20, 40);
    catalog[24..40].copy_from_slice(SEED);
    catalog[40..51].copy_from_slice(b"public.mys\0");
    let mut footer = [0u8; 8];
    u32_at(&mut footer, 0, catalog.len() as u32);
    u32_at(&mut footer, 4, crc32fast::hash(&catalog));
    fixture.bytes = b"public wrapper prefix".to_vec();
    fixture.bytes.extend_from_slice(&encrypt(&catalog));
    fixture.bytes.extend_from_slice(&encrypt(&footer));
    fixture
        .apply(
            root.path(),
            r#"
        local entries=m.catalog(ctx,ctx.open("source","public.mys"),seed,substitution)
        assert(#entries==1 and entries[1].name=="public.mys" and entries[1].flags==0)
        assert(buffer.tostring(entries[1].seed)==buffer.tostring(seed))
    "#,
        )
        .unwrap();
}

#[test]
fn static_loader_recovery_transforms_data_without_executing_a_loader() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    let expected = b"STEELBOXpublicloader";
    let mut payload = (expected.len() as u32).to_le_bytes().to_vec();
    payload.extend_from_slice(&[0, expected.len() as u8 - 1]);
    payload.extend_from_slice(expected);
    let payload = encrypt(&payload);
    let mut header = vec![0u8; 496];
    header[368..384].copy_from_slice(SEED);
    u32_at(&mut header, 32, 760);
    u32_at(&mut header, 36, payload.len() as u32);
    let mut wrapper = (0u32..256)
        .map(|i| ((0xf563_f082u32 >> ((i + 1) % 21)).wrapping_sub(i ^ 0x5a)) as u8)
        .collect::<Vec<_>>();
    wrapper.extend_from_slice(&0u32.to_le_bytes());
    wrapper.extend_from_slice(&520359445u32.to_le_bytes());
    wrapper.extend_from_slice(&encrypt(&header[..368]));
    wrapper.extend_from_slice(SEED);
    wrapper.extend_from_slice(&encrypt(&header[384..]));
    wrapper.extend_from_slice(&payload);
    fixture.bytes = wrapper;
    fixture
        .apply(
            root.path(),
            r#"
        local recovered=m.recover_loader(ctx,ctx.open("source","public.mys"),256,0)
        assert(buffer.tostring(recovered.bytes)=="STEELBOXpublicloader" and recovered.trailing==0)
    "#,
        )
        .unwrap();
}
