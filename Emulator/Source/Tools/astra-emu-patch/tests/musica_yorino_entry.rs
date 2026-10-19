use astra_emu_patch::{apply, PatchErrorKind, PatchRequest};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

const HELPER: &str = include_str!("../../../../Patches/common/yorino_entry.luau");
const ACR: &str = include_str!("../../../../Patches/common/musica_acr.luau");
const COPY: &str = include_str!("../../../../Patches/common/copy.luau");
const ROLES: [(&str, &str); 8] = [
    ("bg.paz", "bg"),
    ("bgm.paz", "bgm"),
    ("cnscr.paz", "scr"),
    ("st.paz", "st"),
    ("cnsys.paz", "sys"),
    ("se.paz", "se"),
    ("voice.paz", "voice"),
    ("mov.paz", "mov"),
];

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn source_hash(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .copied()
        .filter(|b| *b != 0)
        .fold(0_u32, |h, b| h.wrapping_mul(131).wrapping_add(u32::from(b)))
        & 0x7fff_ffff
}

fn acr(count: usize, names: bool) -> Vec<u8> {
    let mut bytes = vec![0; 16 + count * 20];
    put32(&mut bytes, 0, count as u32);
    for i in 0..count {
        // Rows 0 and 1 collide with different original identities; all later
        // dialogue rows deliberately duplicate row 0 with a differing target.
        let original: &[u8] = if names {
            b"\xd6\xd0"
        } else if i == 1 {
            &[2, 1]
        } else {
            &[1, 132]
        };
        let target: &[u8] = if names {
            b"\xd6\xd0\xce\xc4"
        } else if i == 0 {
            b"first"
        } else {
            b"\x93\xfa\x96\x7b\x8c\xea\\N{literal}\\a\\v"
        };
        let from = bytes.len();
        bytes.extend_from_slice(original);
        let to = bytes.len();
        bytes.extend_from_slice(target);
        for (field, value) in [
            source_hash(original),
            from as u32,
            original.len() as u32,
            to as u32,
            target.len() as u32,
        ]
        .into_iter()
        .enumerate()
        {
            put32(&mut bytes, 16 + i * 20 + field * 4, value);
        }
    }
    let stored = bytes.len() as u32 - 16;
    put32(&mut bytes, 8, stored);
    put32(&mut bytes, 12, stored);
    bytes
}

fn cnbg(count: usize) -> (Vec<u8>, Vec<u32>) {
    let mut bytes = vec![0; 24];
    bytes[..8].copy_from_slice(b"FUYINPAK");
    put32(&mut bytes, 8, 24);
    let mut records = Vec::new();
    let mut crcs = Vec::new();
    for index in 0..count {
        let name = format!("public_{index}.bmp\0");
        let name_at = bytes.len() as u32;
        bytes.extend_from_slice(name.as_bytes());
        let data_at = bytes.len() as u32;
        let mut bmp = vec![0; 72]; // 54-byte header + 16 BGRA bytes + proven two-zero tail.
        bmp[..2].copy_from_slice(b"BM");
        put32(&mut bmp, 2, 72);
        put32(&mut bmp, 10, 54);
        put32(&mut bmp, 14, 40);
        put32(&mut bmp, 18, 2);
        put32(&mut bmp, 22, 2);
        bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&32_u16.to_le_bytes());
        put32(&mut bmp, 34, 18);
        // Bottom-up: blue/white bottom row, red/green top row. Zero alpha is data.
        bmp[54..70].copy_from_slice(&[
            255, 0, 0, 127, 255, 255, 255, 255, 0, 0, 255, 0, 0, 255, 0, 32,
        ]);
        crcs.push(crc32fast::hash(&bmp));
        bytes.extend_from_slice(&bmp);
        records.push([name_at, name.len() as u32, data_at, 72, 72]);
    }
    let index_at = bytes.len() as u32;
    for record in records {
        for value in record {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    put32(&mut bytes, 16, index_at);
    put32(&mut bytes, 20, count as u32 * 20);
    (bytes, crcs)
}

struct Fixture {
    root: PathBuf,
    edition: String,
    snapshots: Vec<(PathBuf, Vec<u8>)>,
}

impl Fixture {
    fn new(
        root: &Path,
        names: usize,
        dialogue: usize,
        images: usize,
        bad_crc: bool,
        missing_key: bool,
    ) -> Self {
        let source = root.join("source");
        let patch = root.join("patch");
        let scripts = root.join("scripts");
        fs::create_dir_all(scripts.join("common")).unwrap();
        fs::create_dir(&source).unwrap();
        fs::create_dir(&patch).unwrap();
        for (name, text) in [
            ("yorino_entry.luau", HELPER),
            ("musica_acr.luau", ACR),
            ("copy.luau", COPY),
        ] {
            fs::write(scripts.join("common").join(name), text).unwrap();
        }
        let mut inputs = Vec::new();
        for (path, role) in ROLES {
            // Opaque public data proves this entry does not parse native PAZ/SC.
            let bytes = format!("public opaque native resource for {role}").into_bytes();
            fs::write(source.join(path), &bytes).unwrap();
            inputs.push(format!(
                "{{path={path:?},role={role:?},size={},sha256=\"{:x}\"}}",
                bytes.len(),
                Sha256::digest(&bytes)
            ));
        }
        for path in ["bg.pazA", "st.pazA"] {
            let bytes = b"public opaque split resource";
            fs::write(source.join(path), bytes).unwrap();
            inputs.push(format!(
                "{{path={path:?},size={},sha256=\"{:x}\"}}",
                bytes.len(),
                Sha256::digest(bytes)
            ));
        }
        let (bitmaps, mut crcs) = cnbg(images);
        for (path, bytes) in [
            ("name.acr", acr(names, true)),
            ("AsyncScript.acr", acr(dialogue, false)),
            ("cnbg.paz", bitmaps),
        ] {
            fs::write(patch.join(path), &bytes).unwrap();
            inputs.push(format!(
                "{{path={path:?},patch=true,size={},sha256=\"{:x}\"}}",
                bytes.len(),
                Sha256::digest(&bytes)
            ));
        }
        if bad_crc {
            *crcs.last_mut().unwrap() ^= 1;
        }
        let mut profile = String::from("schema='astra.emu.musica.keys.v2'\npaz_version=2\n");
        profile.push_str("[texture_overrides]\n'musica:/bg/public_0.png'='existing.png'\n'musica:/sys/unrelated.png'='existing.png'\n");
        for (_, role) in ROLES {
            if !(missing_key && role == "mov") {
                profile.push_str(&format!("[archive_keys.{role}]\n"));
            }
        }
        fs::write(source.join("key.toml"), profile).unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([9, 8, 7, 6]))
            .save(source.join("existing.png"))
            .unwrap();
        fs::write(
            source.join("font.ttf"),
            b"public font copy fixture; not a native font validation",
        )
        .unwrap();
        fs::write(
            source.join("original.exe"),
            b"public forbidden original entry",
        )
        .unwrap();
        fs::write(source.join("legacy.dll"), b"public forbidden DLL").unwrap();
        fs::create_dir(source.join("savedata")).unwrap();
        fs::write(
            source.join("savedata/slot.sav"),
            b"public save must remain private and untouched",
        )
        .unwrap();
        let edition = format!("{{names={names},dialogue={dialogue},width=2,height=2,inputs={{{}}},background_crc32={{{}}},coverage={{status='fifteen_name_use_sites_unlocated',row_index_base=0,unlocated_name_rows={{28,29,30,31,32,33,34,35,36,37,38,39,40,41,42}}}}}}",
            inputs.join(","), crcs.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
        let mut snapshots = Vec::new();
        for dir in [source.clone(), patch.clone(), source.join("savedata")] {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_file() {
                    snapshots.push((path.clone(), fs::read(path).unwrap()));
                }
            }
        }
        Self {
            root: root.to_owned(),
            edition,
            snapshots,
        }
    }

    fn run(&self) -> Result<astra_emu_patch::PatchResult, astra_emu_patch::PatchError> {
        let script = self.root.join("scripts/entry.luau");
        fs::write(&script, format!("return {{id='public-yorino',version=1,title='Public Yorino conversion fixture',apply=function(ctx) return ctx.load('common/yorino_entry.luau').apply(ctx,{}) end}}", self.edition)).unwrap();
        let result = apply(
            PatchRequest {
                script,
                source: self.root.join("source"),
                patch: self.root.join("patch"),
                output: self.root.join("output"),
            },
            Arc::new(AtomicBool::new(false)),
            |_| {},
        );
        for (path, bytes) in &self.snapshots {
            assert_eq!(&fs::read(path).unwrap(), bytes);
        }
        result
    }
}

#[test]
fn all_28402_rows_and_40_backgrounds_survive_with_native_aliases_and_specific_coverage_gate() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), 44, 28358, 40, false, false);
    let result = fixture.run().unwrap();
    let config = &result.profile.configuration;
    assert_eq!(config["scr_archive"], "cnscr.paz");
    assert_eq!(config["sys_archive"], "cnsys.paz");
    assert_eq!(config["translation_mode"], "overlay");
    assert_eq!(config["script_encoding"], "gbk");
    assert_eq!(config["font_path"], "font.ttf");
    let table: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("yorino-translations.json")).unwrap())
            .unwrap();
    assert_eq!(table["version"], 2);
    assert_eq!(table["hash_collision_policy"], "first_record");
    assert_eq!(table["source_encoding"], "gbk");
    assert_eq!(table["names"].as_array().unwrap().len(), 44);
    assert_eq!(table["dialogue"].as_array().unwrap().len(), 28358);
    assert_eq!(table["names"][0]["text"], "中文");
    assert_eq!(table["names"][43]["text"], "中文");
    assert_eq!(table["dialogue"][0]["text"], "first");
    assert_eq!(table["dialogue"][1]["text"], "日本語\\N{literal}\\a\\v");
    assert_eq!(table["dialogue"][28357]["text"], "日本語\\N{literal}\\a\\v");
    assert_eq!(
        table["dialogue"][0]["source_hash"],
        table["dialogue"][1]["source_hash"]
    );
    assert_ne!(
        table["dialogue"][0]["source_sha256"],
        table["dialogue"][1]["source_sha256"]
    );
    let coverage: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("yorino-coverage.json")).unwrap())
            .unwrap();
    assert_eq!(coverage["converted_verified"], false);
    assert_eq!(
        coverage["native_endpoint_coverage"]["unlocated_name_rows"]
            .as_array()
            .unwrap()
            .len(),
        15
    );
    assert_eq!(coverage["record_audit"]["different_identity_collisions"], 1);
    let profile: toml::Value =
        toml::from_str(&fs::read_to_string(result.output.join("key.toml")).unwrap()).unwrap();
    assert_eq!(
        profile["texture_overrides"]["musica:/sys/unrelated.png"].as_str(),
        Some("existing.png")
    );
    assert_eq!(
        profile["texture_overrides"]["musica:/bg/public_0.png"].as_str(),
        Some("yorino-backgrounds/public_0.png")
    );
    for i in 0..40 {
        let image = image::open(
            result
                .output
                .join(format!("yorino-backgrounds/public_{i}.png")),
        )
        .unwrap()
        .to_rgba8();
        assert_eq!(
            image.as_raw(),
            &[255, 0, 0, 0, 0, 255, 0, 32, 0, 0, 255, 127, 255, 255, 255, 255]
        );
    }
    for (path, _) in ROLES {
        assert_eq!(
            fs::read(result.output.join(path)).unwrap(),
            fs::read(root.path().join("source").join(path)).unwrap()
        );
    }
    for forbidden in [
        "original.exe",
        "legacy.dll",
        "savedata",
        "name.acr",
        "AsyncScript.acr",
        "cnbg.paz",
        "scr.paz",
        "sys.paz",
    ] {
        assert!(!result.output.join(forbidden).exists());
    }
    assert!(result.output.join("existing.png").is_file());
    assert!(result.output.join("bg.pazA").is_file());
    assert!(result.output.join("st.pazA").is_file());
}

#[test]
fn final_background_crc_failure_rolls_back_all_copied_archives_and_generated_images() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), 3, 5, 3, true, false);
    assert_eq!(
        fixture.run().err().unwrap().kind(),
        PatchErrorKind::Integrity
    );
    assert!(!root.path().join("output").exists());
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("staging")));
}

#[test]
fn missing_eighth_key_role_never_publishes_partial_output() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), 3, 5, 2, false, true);
    assert_eq!(
        fixture.run().err().unwrap().kind(),
        PatchErrorKind::Integrity
    );
    assert!(!root.path().join("output").exists());
}

#[test]
fn changed_pinned_source_fingerprint_is_rejected_without_touching_any_input() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(root.path(), 3, 5, 2, false, false);
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(b"public opaque native resource for bg")
    );
    fixture.edition = fixture.edition.replacen(&fingerprint, &"0".repeat(64), 1);
    assert_eq!(
        fixture.run().err().unwrap().kind(),
        PatchErrorKind::UnsupportedInput
    );
    assert!(!root.path().join("output").exists());
}
