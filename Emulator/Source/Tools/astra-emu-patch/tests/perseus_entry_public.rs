use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use blowfish::{
    cipher::{BlockCipherEncrypt, KeyInit},
    Blowfish,
};
use flate2::{write::ZlibEncoder, Compression};
use md5::{Digest, Md5};
use rc4::{Rc4, StreamCipher};
use sha2::Sha256;
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const PIPELINE: &str = include_str!("../../../../Metadata/patches/common/perseus.luau");
const MOLEBOX: &str = include_str!("../../../../Metadata/patches/common/molebox.luau");
const BINARY: &str = include_str!("../../../../Metadata/patches/common/binary.luau");
const COPY: &str = include_str!("../../../../Metadata/patches/common/copy.luau");
const SCHEDULE: [usize; 60] = [
    11, 12, 13, 14, 8, 9, 10, 4, 5, 6, 7, 1, 2, 3, 12, 13, 14, 0, 9, 10, 11, 5, 6, 7, 8, 2, 3, 4,
    13, 14, 0, 1, 10, 11, 12, 6, 7, 8, 9, 3, 4, 5, 14, 0, 1, 2, 11, 12, 13, 7, 8, 9, 10, 4, 5, 6,
    0, 1, 2, 3,
];
const SEED: [u8; 16] = *b"public-mys-seed!";
const CATALOG_LABEL: &[u8] = b"public-perseus-catalog";
const SYS_KEY: &[u8] = b"public-sys-data-key";

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |mut crc, byte| {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xa001 } else { 0 };
        }
        crc
    })
}
fn cipher(bytes: &[u8], seed: &[u8], cbc: bool) -> Vec<u8> {
    let key = SCHEDULE.map(|i| seed[i]);
    let s = |v: u8| v ^ 0x5a;
    let mut output = bytes.to_vec();
    let mut previous = [0u8; 8];
    for block in output.as_chunks_mut::<8>().0 {
        let mut p = *block;
        if cbc {
            for i in 0..8 {
                p[i] ^= previous[i];
            }
        }
        for i in 0..4 {
            p[i + 4] ^= s(p[i] ^ key[56 + i]);
        }
        for round in (0..8).rev() {
            let k = round * 7;
            p[0] ^= s(p[7] ^ key[k + 6]);
            p[3] ^= s(p[6] ^ key[k + 5]);
            p[2] ^= s(p[4] ^ p[5]);
            p[1] ^= s(p[4] ^ key[k + 4]);
            p[7] ^= s(p[3] ^ key[k + 3]);
            p[6] ^= s(p[2] ^ key[k + 2]);
            p[5] ^= s(p[1] ^ key[k + 1]);
            p[4] ^= s(p[0] ^ key[k]);
        }
        block.copy_from_slice(&p);
        previous = p;
    }
    output
}
fn bitmap(rgba: &[u8], top_down: bool) -> Vec<u8> {
    let height = rgba.len() / 4;
    let mut bytes = vec![0u8; 54 + rgba.len()];
    bytes[..2].copy_from_slice(b"BM");
    put32(&mut bytes, 10, 54);
    put32(&mut bytes, 14, 40);
    put32(&mut bytes, 18, 1);
    put32(
        &mut bytes,
        22,
        if top_down {
            -(height as i32) as u32
        } else {
            height as u32
        },
    );
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
    for (y, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let row = if top_down { y } else { height - y - 1 };
        bytes[54 + row * 4..58 + row * 4]
            .copy_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    bytes
}
fn replacement_bitmap(rgba: &[u8], top_down: bool, tail: bool) -> Vec<u8> {
    let mut bytes = bitmap(rgba, top_down);
    if tail {
        bytes.extend_from_slice(&[0, 0]);
        put32(&mut bytes, 34, (rgba.len() + 2) as u32);
    }
    let size = bytes.len() as u32;
    put32(&mut bytes, 2, size);
    bytes
}
fn png(rgba: &[u8]) -> Vec<u8> {
    let image = image::RgbaImage::from_raw(1, (rgba.len() / 4) as u32, rgba.to_vec()).unwrap();
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Member {
    name: &'static str,
    role: &'static str,
    bytes: Vec<u8>,
}
struct Fixture {
    members: Vec<Member>,
    bad_crc: bool,
    bad_marker: bool,
    bad_span: bool,
    bad_identity: bool,
    bad_original: bool,
    bad_dimensions: bool,
    top_down: bool,
    packed_original: bool,
}
impl Fixture {
    fn new() -> Self {
        let mut scene = vec![b' '; 49159];
        scene[..17].copy_from_slice(b"#include part.sc\n");
        scene[49155..].copy_from_slice(&[0xc4, 0xe3, 0xba, 0xc3]);
        Self {
            members: vec![
                Member {
                    name: "scene.sc",
                    role: "musica_script",
                    bytes: scene,
                },
                Member {
                    name: "part.sc",
                    role: "musica_script",
                    bytes: b"#label public\n".to_vec(),
                },
                Member {
                    name: "ui.png",
                    role: "bmp_xor",
                    bytes: bitmap(&[1, 2, 3, 0, 16, 32, 48, 0], false),
                },
                Member {
                    name: "bg.png",
                    role: "bmp",
                    bytes: replacement_bitmap(&[11, 22, 33, 0, 44, 55, 66, 0], false, true),
                },
                Member {
                    name: "1.ttf",
                    role: "font",
                    bytes: b"public independent font one".to_vec(),
                },
                Member {
                    name: "2.ttf",
                    role: "font",
                    bytes: b"public independent font two".to_vec(),
                },
                Member {
                    name: "op.ass",
                    role: "ass",
                    bytes: b"[Script Info]\nPlayResX: 1280\nPlayResY: 720\n".to_vec(),
                },
            ],
            bad_crc: false,
            bad_marker: false,
            bad_span: false,
            bad_identity: false,
            bad_original: false,
            bad_dimensions: false,
            top_down: false,
            packed_original: false,
        }
    }
    fn package(&self) -> (Vec<u8>, usize) {
        let mut bytes = vec![0u8; 4];
        let mut index = Vec::new();
        let mut offsets = Vec::new();
        let mut blocks = 0;
        for (member_index, member) in self.members.iter().enumerate() {
            offsets.push(index.len() as u32);
            let count = member.bytes.len().div_ceil(49152);
            let start = index.len();
            index.resize(start + 52 + count * 12, 0);
            let name: Vec<u8> = member
                .name
                .to_uppercase()
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            index[start + 20..start + 36].copy_from_slice(&Md5::digest(name));
            put32(&mut index, start + 40, 7);
            put32(&mut index, start + 44, member.bytes.len() as u32);
            put32(&mut index, start + 48, count as u32);
            for (i, block) in member.bytes.chunks(49152).enumerate() {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(block).unwrap();
                let coded = cipher(&encoder.finish().unwrap(), &SEED, true);
                let at = start + 52 + i * 12;
                index[at..at + 2].copy_from_slice(&(coded.len() as u16).to_le_bytes());
                index[at + 2..at + 4].copy_from_slice(&(block.len() as u16).to_le_bytes());
                index[at + 4..at + 6].copy_from_slice(&6u16.to_le_bytes());
                let corrupt = self.bad_crc && member_index == self.members.len() - 1;
                index[at + 6..at + 8]
                    .copy_from_slice(&(crc16(block) ^ u16::from(corrupt)).to_le_bytes());
                put32(&mut index, at + 8, (bytes.len() - 4) as u32);
                bytes.extend(coded);
                blocks += 1;
            }
        }
        let root = index.len();
        index.resize(root + 52 + offsets.len() * 4, 0);
        put32(&mut index, root + 40, 0x8000_0000);
        put32(&mut index, root + 48, offsets.len() as u32);
        for (i, offset) in offsets.iter().enumerate() {
            put32(&mut index, root + 52 + i * 4, *offset);
        }
        let lookup = index.len();
        for offset in &offsets {
            index.extend_from_slice(&offset.to_le_bytes());
        }
        let header_offset = bytes.len() + index.len();
        let mut header = vec![0u8; 48];
        put32(&mut header, 4, index.len() as u32);
        put32(&mut header, 8, root as u32);
        put32(&mut header, 12, header_offset as u32 - 4);
        put32(&mut header, 16, crc32fast::hash(&index));
        put32(&mut header, 20, lookup as u32);
        put32(&mut header, 24, offsets.len() as u32);
        header[40..].copy_from_slice(b"STELPACK");
        bytes.extend(cipher(&index, &SEED, true));
        bytes.extend(cipher(&header, &SEED, true));
        put32(&mut bytes, 0, header_offset as u32);
        (bytes, blocks)
    }
    fn wrapper(&self) -> Vec<u8> {
        let wrapper_seed = *b"public-wrap-seed";
        let mut loader = vec![0u8; 512];
        loader[..8].copy_from_slice(b"STEELBOX");
        for i in 0..256 {
            loader[64 + i] = i as u8 ^ 0x5a;
        }
        loader[400..400 + CATALOG_LABEL.len()].copy_from_slice(CATALOG_LABEL);
        let mut coded = (loader.len() as u32).to_le_bytes().to_vec();
        for chunk in loader.chunks(256) {
            coded.extend([0, (chunk.len() - 1) as u8]);
            coded.extend(chunk);
        }
        let mut header = vec![0u8; 496];
        put32(&mut header, 32, 2048);
        put32(&mut header, 36, coded.len() as u32);
        header[368..384].copy_from_slice(&wrapper_seed);
        let mut wrapper = vec![0u8; 2048];
        put32(&mut wrapper, 64, if self.bad_marker { 1 } else { 0 });
        put32(&mut wrapper, 68, 520359445);
        wrapper[72..440].copy_from_slice(&cipher(&header[..368], &wrapper_seed, true));
        wrapper[440..456].copy_from_slice(&wrapper_seed);
        wrapper[456..568].copy_from_slice(&cipher(&header[384..], &wrapper_seed, true));
        for i in 0..256 {
            wrapper[600 + i] =
                ((0xf563f082u32 >> ((i + 1) % 21)) as u8).wrapping_sub(i as u8 ^ 0x5a);
        }
        wrapper.extend(cipher(&coded, &wrapper_seed, true));
        let catalog_seed = Md5::digest(CATALOG_LABEL);
        let mut catalog = vec![0u8; 56];
        put32(&mut catalog, 8, 16);
        put32(&mut catalog, 12, 1);
        put32(&mut catalog, 20, 40);
        catalog[24..40].copy_from_slice(&SEED);
        catalog[40..51].copy_from_slice(b"public.mys\0");
        let mut trailer = (catalog.len() as u32).to_le_bytes().to_vec();
        trailer.extend(crc32fast::hash(&catalog).to_le_bytes());
        wrapper.extend(cipher(&catalog, &catalog_seed, true));
        wrapper.extend(cipher(&trailer, &catalog_seed, false));
        wrapper
    }
    fn apply(
        &mut self,
        root: &Path,
        cancel_after_member: bool,
    ) -> Result<astra_emu_patch::PatchResult, astra_emu_patch::PatchError> {
        if self.top_down {
            self.members[3].bytes =
                replacement_bitmap(&[11, 22, 33, 0, 44, 55, 66, 0], true, false);
        }
        if self.bad_dimensions {
            put32(&mut self.members[3].bytes, 22, i32::MAX as u32);
        }
        let source = root.join("source");
        let scripts = root.join("scripts");
        fs::create_dir_all(source.join("media")).unwrap();
        fs::create_dir_all(scripts.join("common")).unwrap();
        for (name, module) in [
            ("perseus", PIPELINE),
            ("molebox", MOLEBOX),
            ("binary", BINARY),
            ("copy", COPY),
        ] {
            fs::write(scripts.join(format!("common/{name}.luau")), module).unwrap();
        }
        let (package, blocks) = self.package();
        let wrapper = self.wrapper();
        let original = png(&[10, 20, 30, 40, 50, 60, 70, 80]);
        let material = format!("ui.png {:08X} public-password", original.len());
        let skip = (crc32fast::hash(material.as_bytes()) >> 12) & 0xff;
        let mut sys_bytes = if self.packed_original {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&original).unwrap();
            encoder.finish().unwrap()
        } else {
            original.clone()
        };
        let stored = sys_bytes.len();
        sys_bytes.resize(sys_bytes.len().next_multiple_of(8), 0);
        if !self.packed_original {
            let mut stream = Rc4::new_from_slice(material.as_bytes()).unwrap();
            stream.apply_keystream(&mut vec![0u8; skip as usize]);
            stream.apply_keystream(&mut sys_bytes);
        }
        let blowfish: Blowfish = Blowfish::new_from_slice(SYS_KEY).unwrap();
        for block in sys_bytes.as_chunks_mut::<8>().0 {
            block[..4].reverse();
            block[4..].reverse();
            blowfish.encrypt_block((&mut *block).into());
            block[..4].reverse();
            block[4..].reverse();
        }
        for byte in &mut sys_bytes {
            *byte ^= 0x87;
        }
        let aligned = sys_bytes.len();
        let mut sys = vec![0u8; 8];
        sys.extend(sys_bytes);
        let key = format!("schema = 'astra.emu.musica.keys.v2'\npaz_version = 2\nrc4_skip_crc = true\n[archive_keys.sys]\ndata_key_hex = '{}'\n[archive_keys.sys.type_passwords]\npng = 'public-password'\n[texture_overrides]\n'musica:/sys/unchanged.png' = 'independent.png'\n", hex(SYS_KEY));
        fs::write(source.join("key.toml"), &key).unwrap();
        fs::write(source.join("public.exe"), &wrapper).unwrap();
        fs::write(source.join("public.mys"), &package).unwrap();
        fs::write(source.join("sys.paz"), &sys).unwrap();
        fs::write(source.join("independent.png"), &original).unwrap();
        fs::write(source.join("media/op.avi"), b"independent movie").unwrap();
        fs::write(source.join("bg.pazA"), b"independent split bank").unwrap();
        fs::write(source.join("old-save.dat"), b"never copy").unwrap();
        let mut members = String::new();
        for (i, member) in self.members.iter().enumerate() {
            let hash = if self.bad_identity && i == 1 {
                "0".repeat(64)
            } else {
                digest(&member.bytes)
            };
            members.push_str(&format!(
                "{{path={:?},role={:?},size={},sha256={:?}}},",
                member.name,
                member.role,
                member.bytes.len(),
                hash
            ));
        }
        let script = scripts.join("public.luau");
        fs::write(&script, format!(r#"return {{id="public-perseus",version=1,title="Public full legacy conversion",apply=function(ctx)
            return ctx.load("common/perseus.luau").apply(ctx, {{
                loader="public.exe",loader_sha256={wrapper_hash:?},marker=64,source_substitution=600,label=400,inner_substitution=64,
                package="public.mys",package_sha256={package_hash:?},sys_sha256={sys_hash:?},sys_xor_byte=135,blocks={blocks},
                roles={{musica_script=2,bmp_xor=1,bmp=1,font=2,ass=1}}, members={{{members}}},
                originals={{["ui.png"]={{name="ui.png",offset={offset},stored_size={stored},aligned_size={aligned},unpacked_size={unpacked},packed={packed},sha256={original_hash:?}}}}},
                fonts={{"1.ttf","2.ttf"}},tracks={{{{resource="op.avi",ass="op.ass"}}}},
                files={{"sys.paz","bg.pazA","media/op.avi"}},
            }})
        end}}"#, wrapper_hash=digest(&wrapper), package_hash=digest(&package), sys_hash=digest(&sys),
            offset=if self.bad_span { sys.len() + 8 } else { 8 }, unpacked=original.len(), packed=self.packed_original,
            original_hash=if self.bad_original { "0".repeat(64) } else { digest(&original) })).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let callback_flag = cancelled.clone();
        let result = apply(
            PatchRequest {
                script,
                source: source.clone(),
                patch: source.clone(),
                output: root.join("output"),
            },
            cancelled,
            move |progress| {
                if cancel_after_member && progress.phase == "Restoring checked Perseus resources" {
                    callback_flag.store(true, Ordering::Release);
                }
            },
        );
        assert_eq!(fs::read(source.join("public.exe")).unwrap(), wrapper);
        assert_eq!(fs::read(source.join("public.mys")).unwrap(), package);
        assert_eq!(fs::read(source.join("sys.paz")).unwrap(), sys);
        assert_eq!(fs::read_to_string(source.join("key.toml")).unwrap(), key);
        result
    }
}

#[test]
fn recovered_loader_catalog_and_every_member_produce_independent_native_resources() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    let result = fixture.apply(root.path(), false).unwrap();
    let output = &result.output;
    for member in fixture
        .members
        .iter()
        .filter(|m| matches!(m.role, "musica_script" | "font" | "ass"))
    {
        let directory = match member.role {
            "musica_script" => "scripts",
            "font" => "fonts",
            _ => "subtitles",
        };
        assert_eq!(
            fs::read(output.join(format!("localized/perseus/{directory}/{}", member.name)))
                .unwrap(),
            member.bytes
        );
    }
    let ui = image::open(output.join("localized/perseus/textures/ui.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(ui.as_raw(), &[11, 22, 29, 40, 34, 28, 118, 80]);
    let bg = image::open(output.join("localized/perseus/textures/bg.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(bg.as_raw(), &[11, 22, 33, 0, 44, 55, 66, 0]);
    let key: toml::Value =
        toml::from_str(&fs::read_to_string(output.join("key.toml")).unwrap()).unwrap();
    assert_eq!(
        key["texture_overrides"]["musica:/sys/ui.png"]
            .as_str()
            .unwrap(),
        "localized/perseus/textures/ui.png"
    );
    assert_eq!(
        key["texture_overrides"]["musica:/bg/bg.png"]
            .as_str()
            .unwrap(),
        "localized/perseus/textures/bg.png"
    );
    let subtitles: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("localized/perseus/subtitles.json")).unwrap())
            .unwrap();
    assert_eq!(subtitles["tracks"][0]["resource"], "op.avi");
    assert_eq!(subtitles["tracks"][0]["fonts"].as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read(output.join("bg.pazA")).unwrap(),
        b"independent split bank"
    );
    assert_eq!(
        fs::read(output.join("media/op.avi")).unwrap(),
        b"independent movie"
    );
    assert_eq!(
        fs::read(output.join("independent.png")).unwrap(),
        fs::read(root.path().join("source/independent.png")).unwrap()
    );
    assert!(!output.join("old-save.dat").exists());
    assert!(!output.join("public.exe").exists());
    assert!(!output.join("public.mys").exists());
    assert!(!output.join("localized/perseus/scripts/noTitle.sc").exists());
}

#[test]
fn top_down_full_replacement_keeps_original_row_order_and_zero_alpha() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    fixture.top_down = true;
    let result = fixture.apply(root.path(), false).unwrap();
    assert_eq!(
        image::open(result.output.join("localized/perseus/textures/bg.png"))
            .unwrap()
            .to_rgba8()
            .as_raw(),
        &[11, 22, 33, 0, 44, 55, 66, 0]
    );
}

#[test]
fn packed_native_original_slice_is_inflated_before_xor_without_filename_crypto() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    fixture.packed_original = true;
    let result = fixture.apply(root.path(), false).unwrap();
    assert_eq!(
        image::open(result.output.join("localized/perseus/textures/ui.png"))
            .unwrap()
            .to_rgba8()
            .as_raw(),
        &[11, 22, 29, 40, 34, 28, 118, 80]
    );
}

#[test]
fn late_crc_identity_slice_and_dimension_failures_never_publish_partial_output() {
    for (case, expected) in [
        (0, PatchErrorKind::Integrity),
        (1, PatchErrorKind::Integrity),
        (2, PatchErrorKind::Bounds),
        (3, PatchErrorKind::Integrity),
        (4, PatchErrorKind::Bounds),
        (5, PatchErrorKind::UnsupportedInput),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = Fixture::new();
        match case {
            0 => fixture.bad_crc = true,
            1 => fixture.bad_identity = true,
            2 => fixture.bad_span = true,
            3 => fixture.bad_original = true,
            4 => fixture.bad_dimensions = true,
            _ => fixture.bad_marker = true,
        }
        let error = fixture.apply(root.path(), false).err().unwrap();
        assert_eq!(error.kind(), expected, "case {case}");
        assert!(!root.path().join("output").exists());
    }
}

#[test]
fn cancellation_after_first_checked_member_rolls_back_all_staged_resources() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new();
    assert_eq!(
        fixture.apply(root.path(), true).err().unwrap().kind(),
        PatchErrorKind::Cancelled
    );
    assert!(!root.path().join("output").exists());
}
