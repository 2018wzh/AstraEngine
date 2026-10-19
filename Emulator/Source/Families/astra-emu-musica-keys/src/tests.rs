use super::*;
use std::fs;

fn key_file() -> MusicaKeyFile {
    MusicaKeyFile {
        schema: MUSICA_KEY_SCHEMA.into(),
        paz_version: 2,
        index_size_xor: 123,
        rc4_skip_crc: false,
        archive_keys: REQUIRED_ARCHIVE_ROLES
            .into_iter()
            .map(|role| {
                (
                    role.into(),
                    MusicaArchiveKey {
                        index_key_hex: "01aB23CD".into(),
                        data_key_hex: if role == "mov" {
                            String::new()
                        } else {
                            "aB0123cd".into()
                        },
                        type_passwords: BTreeMap::new(),
                        archive_xor: Some(123),
                        video_key_hex: None,
                    },
                )
            })
            .collect(),
        texture_overrides: BTreeMap::new(),
    }
}

fn text(key: &MusicaKeyFile) -> String {
    toml::to_string_pretty(key).unwrap()
}

fn rejected(bytes: &[u8], code: &str) {
    let error = parse_key_file(bytes)
        .err()
        .expect("invalid keys were accepted");
    assert_eq!(error.code, code);
    assert!(!error.to_string().contains("private-secret-sentinel"));
}

#[test]
fn canonical_toml_round_trips_every_role_option() {
    let mut key = key_file();
    key.archive_keys.get_mut("bg").unwrap().type_passwords = BTreeMap::from([
        ("png".into(), "public-image-password".into()),
        ("sc".into(), "public-script-password".into()),
    ]);
    key.archive_keys.get_mut("mov").unwrap().type_passwords = BTreeMap::from([
        ("avi".into(), "public-movie-password".into()),
        ("ogg".into(), "public-audio-password".into()),
    ]);
    key.archive_keys.get_mut("mov").unwrap().video_key_hex = Some("aB".repeat(256));
    key.texture_overrides
        .insert("musica:/bg/BG.png".into(), "hd/BG.png".into());
    let encoded = text(&key);
    let parsed = parse_key_file(encoded.as_bytes()).unwrap();
    assert!(text(&parsed) == encoded);
    assert_eq!(parsed.paz_version, 2);
    assert_eq!(parsed.index_size_xor, 123);
    assert!(!parsed.rc4_skip_crc);
}

#[test]
fn canonical_toml_defaults_only_optional_maps_and_options() {
    let mut table: toml::Table = toml::from_str(&text(&key_file())).unwrap();
    table.remove("texture_overrides");
    for (_, role) in table["archive_keys"].as_table_mut().unwrap().iter_mut() {
        let role = role.as_table_mut().unwrap();
        role.remove("type_passwords");
        role.remove("archive_xor");
        role.remove("video_key_hex");
    }
    let key = parse_key_file(toml::to_string_pretty(&table).unwrap().as_bytes()).unwrap();
    assert!(key.texture_overrides.is_empty());
    assert!(key
        .archive_keys
        .values()
        .all(|role| role.type_passwords.is_empty()
            && role.archive_xor.is_none()
            && role.video_key_hex.is_none()));
}

#[test]
fn canonical_toml_rejects_json_garbro_and_legacy_schema() {
    rejected(br#"{"schema":"astra.emu.musica.profile.v1","roles":{},"secret":"private-secret-sentinel"}"#,
        "ASTRA_EMU_PROFILE_FORMAT");
    rejected(
        &serde_json::to_vec(&key_file()).unwrap(),
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    rejected(
        b"GARbroDB\0private-secret-sentinel",
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    rejected(
        b"\0\x01\0\0\0private-secret-sentinel",
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    for schema in [
        "astra.emu.musica.keys.v1",
        "astra.emu.musica.profile.v1",
        "private-secret-sentinel",
    ] {
        let mut key = key_file();
        key.schema = schema.into();
        rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_SCHEMA");
    }
}

#[test]
fn canonical_toml_requires_all_algorithm_metadata_and_strict_fields() {
    let original: toml::Table = toml::from_str(&text(&key_file())).unwrap();
    for field in [
        "schema",
        "paz_version",
        "index_size_xor",
        "rc4_skip_crc",
        "archive_keys",
    ] {
        let mut table = original.clone();
        table.remove(field);
        rejected(
            toml::to_string_pretty(&table).unwrap().as_bytes(),
            "ASTRA_EMU_PROFILE_FORMAT",
        );
    }
    for field in ["index_key_hex", "data_key_hex"] {
        let mut table = original.clone();
        table["archive_keys"]["bg"]
            .as_table_mut()
            .unwrap()
            .remove(field);
        rejected(
            toml::to_string_pretty(&table).unwrap().as_bytes(),
            "ASTRA_EMU_PROFILE_FORMAT",
        );
    }
    let mut table = original.clone();
    table.insert("private-secret-sentinel".into(), toml::Value::Boolean(true));
    rejected(
        toml::to_string_pretty(&table).unwrap().as_bytes(),
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    let mut table = original.clone();
    table["archive_keys"]["bg"]
        .as_table_mut()
        .unwrap()
        .insert("private-secret-sentinel".into(), toml::Value::Boolean(true));
    rejected(
        toml::to_string_pretty(&table).unwrap().as_bytes(),
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    for (field, value) in [
        ("paz_version", -1),
        ("paz_version", 256),
        ("index_size_xor", -1),
        ("index_size_xor", i64::from(u32::MAX) + 1),
    ] {
        let mut table = original.clone();
        table.insert(field.into(), toml::Value::Integer(value));
        rejected(
            toml::to_string_pretty(&table).unwrap().as_bytes(),
            "ASTRA_EMU_PROFILE_FORMAT",
        );
    }
    rejected(
        format!(
            "schema = \"private-secret-sentinel\"\n{}",
            text(&key_file())
        )
        .as_bytes(),
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    rejected(
        b"schema = \"private-secret-sentinel",
        "ASTRA_EMU_PROFILE_FORMAT",
    );
    rejected(
        b"\xffprivate-secret-sentinel",
        "ASTRA_EMU_MUSICA_KEY_ENCODING",
    );
}

#[test]
fn canonical_toml_rejects_invalid_hex_roles_and_private_bounds() {
    for version in 0..=2 {
        for count in [4, 56] {
            let mut key = key_file();
            key.paz_version = version;
            for role in key.archive_keys.values_mut() {
                role.index_key_hex = "aB".repeat(count);
                role.data_key_hex = "Cd".repeat(count);
            }
            key.validate().unwrap();
        }
    }
    for hex in [
        "0",
        "abcdxyz0",
        "01 02 03",
        "０１２３",
        "private-secret-sentinel",
    ] {
        let mut key = key_file();
        key.archive_keys.get_mut("bg").unwrap().index_key_hex = hex.into();
        rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_KEY_HEX");
    }
    for hex in [String::new(), "00".repeat(3), "00".repeat(57)] {
        let mut key = key_file();
        key.archive_keys.get_mut("bg").unwrap().index_key_hex = hex;
        rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_BLOWFISH_KEY");
    }
    for role in ["bg", "mov"] {
        let mut key = key_file();
        key.archive_keys.get_mut(role).unwrap().data_key_hex = "00".repeat(57);
        rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_BLOWFISH_KEY");
    }
    let mut key = key_file();
    key.archive_keys.get_mut("bg").unwrap().data_key_hex.clear();
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_BLOWFISH_KEY");
    let mut key = key_file();
    key.archive_keys.get_mut("mov").unwrap().data_key_hex = "ab".into();
    key.validate().unwrap(); // mov preserves the existing 0..=56-byte allowance.
    for count in [255, 257] {
        let mut key = key_file();
        key.archive_keys.get_mut("mov").unwrap().video_key_hex = Some("00".repeat(count));
        rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_VIDEO_KEY");
    }
    let mut key = key_file();
    key.archive_keys.get_mut("bg").unwrap().archive_xor = Some(124);
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_XOR");
    let mut key = key_file();
    key.archive_keys
        .get_mut("bg")
        .unwrap()
        .type_passwords
        .insert(
            "private-secret-sentinel".into(),
            "private-secret-sentinel".into(),
        );
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_TYPE_KEY");
    let mut key = key_file();
    key.archive_keys.remove("bg");
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_ROLES");
    let mut key = key_file();
    key.archive_keys.insert(
        "private-secret-sentinel".into(),
        key.archive_keys["bg"].clone(),
    );
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_ROLES");
    let mut key = key_file();
    let bg = key.archive_keys.remove("bg").unwrap();
    key.archive_keys.insert("BG".into(), bg);
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PRIVATE_ROLES");
    let mut key = key_file();
    key.paz_version = 3;
    rejected(text(&key).as_bytes(), "ASTRA_EMU_MUSICA_PROFILE_VERSION");
}

#[test]
fn canonical_toml_validates_texture_paths_and_bounds_without_filesystem() {
    for path in [
        "../BG.png",
        "/BG.png",
        "C:/private-secret-sentinel.png",
        "hd\\BG.png",
        "hd//BG.png",
        "./BG.png",
    ] {
        let mut key = key_file();
        key.texture_overrides
            .insert("musica:/bg/BG.png".into(), path.into());
        rejected(
            text(&key).as_bytes(),
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
        );
    }
    for source in [
        "other:/bg/BG.png".to_owned(),
        "musica:/bg/../BG.png".into(),
        "musica:/".into(),
        "musica://bg/BG.png".into(),
        "musica:/bg/BG.png/".into(),
        "musica:/bg\\BG.png".into(),
        "musica:/bg/a:b.png".into(),
        "musica:/bg/\u{1}.png".into(),
        format!("musica:/bg/{}.png", "a".repeat(512)),
    ] {
        let mut key = key_file();
        key.texture_overrides.insert(source, "hd/BG.png".into());
        rejected(
            text(&key).as_bytes(),
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE",
        );
    }
    for path in ["hd/BG.jpg".into(), format!("hd/{}.png", "a".repeat(512))] {
        let mut key = key_file();
        key.texture_overrides
            .insert("musica:/bg/BG.png".into(), path);
        rejected(
            text(&key).as_bytes(),
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT",
        );
    }
    let mut key = key_file();
    key.texture_overrides = (0..=MAX_TEXTURE_OVERRIDES)
        .map(|index| (format!("musica:/bg/{index}.png"), "hd/BG.png".into()))
        .collect();
    rejected(
        text(&key).as_bytes(),
        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_COUNT",
    );
}

#[test]
fn canonical_key_loader_is_bounded_regular_read_only_and_redacted() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(MUSICA_KEY_FILE);
    let mut bytes = text(&key_file()).into_bytes();
    bytes.resize(MAX_KEY_BYTES as usize, b' ');
    fs::write(&path, &bytes).unwrap();
    load_musica_key_file(&path).unwrap();
    bytes.push(b' ');
    fs::write(&path, &bytes).unwrap();
    let error = load_musica_key_file(&path).err().unwrap();
    assert_eq!(error.code, "ASTRA_EMU_PROFILE_BOUND");
    assert_eq!(fs::metadata(&path).unwrap().len(), MAX_KEY_BYTES + 1);
    assert!(load_musica_key_file(root.path()).is_err());
    let missing = root.path().join("private-secret-sentinel");
    let error = load_musica_key_file(&missing).err().unwrap();
    assert_eq!(error.code, "ASTRA_EMU_PROFILE_MISSING");
    assert!(!error.to_string().contains("private-secret-sentinel"));
    assert!(!missing.exists());
    fs::write(&path, b"index_key_hex = \"private-secret-sentinel").unwrap();
    let error = load_musica_key_file(&path).err().unwrap();
    assert_eq!(error.code, "ASTRA_EMU_PROFILE_FORMAT");
    assert!(!error.to_string().contains("private-secret-sentinel"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn key_file_errors_are_redacted_and_display_code_then_message() {
    for (kind, code) in [
        (ErrorKind::NotFound, "ASTRA_EMU_PROFILE_MISSING"),
        (ErrorKind::PermissionDenied, "ASTRA_EMU_PROFILE_PERMISSION"),
        (ErrorKind::Other, "ASTRA_EMU_PROFILE_OPEN"),
    ] {
        let error = key_io_error(std::io::Error::new(kind, "private-secret-sentinel"));
        assert_eq!(error.code, code);
        assert!(!error.message.contains("private-secret-sentinel"));
        assert_eq!(error.to_string(), format!("{code}: {}", error.message));
    }
    for (code, _) in KEY_FILE_ERRORS {
        assert_eq!(key_file_error(code).unwrap().code, code);
    }
    assert!(key_file_error("ASTRA_EMU_PROFILE_ROOT").is_none());
}
