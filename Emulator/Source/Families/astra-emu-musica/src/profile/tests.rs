use super::*;
use astra_core::Hash256;
use astra_emu_musica_keys::{MusicaArchiveKey, MUSICA_KEY_FILE, MUSICA_KEY_SCHEMA};
use std::fs::File;

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

#[test]
fn validated_keys_decode_into_paz_role_schemes() {
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
    key.validate().unwrap();
    let schemes = into_schemes(&key);
    assert_eq!(schemes["bg"].index_key, [1, 0xab, 0x23, 0xcd]);
    assert_eq!(schemes["bg"].data_key, [0xab, 1, 0x23, 0xcd]);
    assert!(schemes["mov"].data_key.is_empty());
    assert_eq!(schemes["mov"].video_key, Some([0xab; 256]));
    assert_eq!(schemes["mov"].archive_xor, Some(123));
    assert_eq!(
        schemes["mov"].type_passwords["avi"],
        "public-movie-password"
    );
    assert_eq!(schemes["bg"].type_passwords["png"], "public-image-password");
    assert!(!schemes["bg"].rc4_skip_crc);
    MusicaPazDecryptProvider::new(Hash256::from_sha256(b"public-test"), schemes).unwrap();
}

#[test]
fn canonical_toml_mount_preserves_raw_hash_decryption_and_path_confinement() {
    let root = tempfile::tempdir().unwrap();
    crate::test_fixture::game(root.path(), b".end\r\n");
    let path = root.path().join(MUSICA_KEY_FILE);
    let original = fs::read(&path).unwrap();
    let archive = mount_musica(root.path(), Path::new(MUSICA_KEY_FILE)).unwrap();
    assert_eq!(
        archive.manifest().private_profile_hash,
        Hash256::from_sha256(&original)
    );
    assert_eq!(
        archive.manifest().mount_profile_hash,
        Hash256::from_sha256(&original)
    );
    assert_eq!(
        archive.manifest().entries.len(),
        REQUIRED_ARCHIVE_ROLES.len()
    );
    assert_eq!(
        archive
            .read_range("musica:/scr/test.sc", 0, 6)
            .unwrap()
            .bytes
            .as_slice(),
        b".end\r\n"
    );
    let mut changed = original.clone();
    changed.extend_from_slice(b"\n# public formatting change\n");
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), &changed).unwrap();
    let external = mount_musica(root.path(), outside.path()).unwrap();
    assert_eq!(
        external.manifest().mount_profile_hash,
        Hash256::from_sha256(&changed)
    );
    assert_ne!(
        archive.manifest().mount_profile_hash,
        external.manifest().mount_profile_hash
    );
    assert_eq!(
        mount_musica(root.path(), Path::new("../key.toml"))
            .err()
            .unwrap()
            .code(),
        "ASTRA_EMU_PROFILE_PATH"
    );
    fs::write(&path, b"{\"schema\":\"astra.emu.musica.profile.v1\"}").unwrap();
    assert_eq!(
        mount_musica(root.path(), &path).err().unwrap().code(),
        "ASTRA_EMU_PROFILE_FORMAT"
    );
    assert_eq!(fs::read(outside.path()).unwrap(), changed);
}

#[test]
fn canonical_toml_texture_resolution_preserves_file_bounds() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("hd")).unwrap();
    let path = root.path().join("hd/BG.png");
    fs::write(&path, [1, 2, 3]).unwrap();
    let mut key = key_file();
    key.texture_overrides
        .insert("musica:/bg/BG.png".into(), "hd/BG.png".into());
    key.validate().unwrap();
    let resolved = resolve_texture_overrides(&key, root.path()).unwrap();
    assert_eq!(resolved["musica:/bg/BG.png"], path.canonicalize().unwrap());
    for size in [0, MAX_TEXTURE_OVERRIDE_BYTES + 1] {
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(size)
            .unwrap();
        assert_eq!(
            resolve_texture_overrides(&key, root.path())
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_BOUND"
        );
    }
    key.texture_overrides
        .insert("musica:/bg/BG.png".into(), "hd/missing.png".into());
    assert_eq!(
        resolve_texture_overrides(&key, root.path())
            .err()
            .unwrap()
            .code(),
        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH"
    );
}

#[test]
fn canonical_key_errors_preserve_codes_and_do_not_echo_private_details() {
    for code in [
        "ASTRA_EMU_PROFILE_MISSING",
        "ASTRA_EMU_PROFILE_PERMISSION",
        "ASTRA_EMU_PROFILE_OPEN",
        "ASTRA_EMU_PROFILE_FILE",
        "ASTRA_EMU_PROFILE_READ",
        "ASTRA_EMU_PROFILE_BOUND",
        "ASTRA_EMU_PROFILE_PATH",
    ] {
        let error = configured_key_error(CoreError::invalid(code, "private-secret-sentinel"));
        assert_eq!(error.code(), code);
        assert!(!error.message().contains("private-secret-sentinel"));
    }
    let root_error = CoreError::invalid("ASTRA_EMU_PROFILE_ROOT", "game directory is unavailable");
    assert_eq!(configured_key_error(root_error.clone()), root_error);
    let root = tempfile::tempdir().unwrap();
    let error = mount_musica(root.path(), Path::new(MUSICA_KEY_FILE))
        .err()
        .unwrap();
    assert_eq!(error.code(), "ASTRA_EMU_PROFILE_MISSING");
}
