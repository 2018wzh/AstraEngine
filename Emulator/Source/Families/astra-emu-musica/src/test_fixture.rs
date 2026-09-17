use crate::{
    MusicaProfile, MusicaRolePrivateProfile, MUSICA_PROFILE_FILE, MUSICA_PROFILE_SCHEMA,
    REQUIRED_ARCHIVE_ROLES,
};
use blowfish::{
    cipher::{BlockCipherEncrypt, KeyInit},
    Blowfish,
};
use std::{collections::BTreeMap, io::Cursor, path::Path};
const KEY: &[u8] = b"public-fixture-key";
fn encrypt(bytes: &[u8]) -> Vec<u8> {
    let cipher: Blowfish = Blowfish::new_from_slice(KEY).unwrap();
    let mut bytes = bytes.to_vec();
    for chunk in bytes.as_chunks_mut::<8>().0 {
        chunk[..4].reverse();
        chunk[4..].reverse();
        cipher.encrypt_block((&mut *chunk).into());
        chunk[..4].reverse();
        chunk[4..].reverse();
    }
    bytes
}
fn archive(role: &str, name: &str, payload: &[u8]) -> Vec<u8> {
    let aligned = payload.len().div_ceil(8) * 8;
    let mut index = 1u32.to_le_bytes().to_vec();
    if role == "mov" {
        index.extend(0u8..=255);
    }
    index.extend_from_slice(name.as_bytes());
    index.push(0);
    let offset = index.len();
    index.extend_from_slice(&[0; 8]);
    index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    index.extend_from_slice(&(aligned as u32).to_le_bytes());
    index.extend_from_slice(&0u32.to_le_bytes());
    index.resize(index.len().div_ceil(8) * 8, 0);
    let end = 4 + index.len();
    index[offset..offset + 8].copy_from_slice(&(end as u64).to_le_bytes());
    let mut bytes = (index.len() as u32).to_le_bytes().to_vec();
    bytes.extend(encrypt(&index));
    let mut data = payload.to_vec();
    data.resize(aligned, 0);
    if role != "mov" {
        data = encrypt(&data);
    }
    bytes.extend(data);
    bytes
}
pub(crate) fn game(root: &Path, script: &[u8]) {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([25, 100, 220, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let wave = wave();
    for role in REQUIRED_ARCHIVE_ROLES {
        let (name, payload) = match role {
            "scr" => ("test.sc", script),
            "bg" => ("BG.png", png.get_ref().as_slice()),
            "bgm" => ("tone.ogg", wave.as_slice()),
            _ => ("fixture.bin", &b"fixture\0"[..]),
        };
        std::fs::write(
            root.join(format!("{role}.paz")),
            archive(role, name, payload),
        )
        .unwrap();
    }
    let profile = MusicaProfile {
        schema: MUSICA_PROFILE_SCHEMA.into(),
        paz_version: 0,
        index_size_xor: 0,
        roles: REQUIRED_ARCHIVE_ROLES
            .iter()
            .map(|role| {
                (
                    (*role).into(),
                    MusicaRolePrivateProfile {
                        index_key: KEY.to_vec(),
                        data_key: if *role == "mov" { vec![] } else { KEY.to_vec() },
                        type_passwords: BTreeMap::new(),
                        archive_xor: None,
                        video_key: None,
                    },
                )
            })
            .collect(),
    };
    std::fs::write(
        root.join(MUSICA_PROFILE_FILE),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
}
pub(crate) fn wave() -> Vec<u8> {
    let samples = (0..4800)
        .map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 16000.0) as i16)
        .collect::<Vec<_>>();
    let len = samples.len() as u32 * 2;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(48000u32.to_le_bytes());
    out.extend(96000u32.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(len.to_le_bytes());
    for s in samples {
        out.extend(s.to_le_bytes());
    }
    out
}

pub(crate) fn stand(root: &Path, png: &[u8]) {
    std::fs::write(root.join("st.paz"), archive("st", "Stand.png", png)).unwrap();
}

pub(crate) fn asset(root: &Path, role: &str, name: &str, bytes: &[u8]) {
    std::fs::write(root.join(format!("{role}.paz")), archive(role, name, bytes)).unwrap();
}
