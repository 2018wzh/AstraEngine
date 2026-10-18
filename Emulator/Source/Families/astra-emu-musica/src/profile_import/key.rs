use crate::{MusicaProfile, MusicaRolePrivateProfile, MUSICA_PROFILE_SCHEMA, REQUIRED_ARCHIVE_ROLES};
use serde::Deserialize;
use std::{collections::{BTreeMap, BTreeSet}, fs::File, io::{ErrorKind, Read}, path::Path};

const KEY_SCHEMA: &str = "astra.emu.musica.keys.v1";
const MAX_KEY_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFile {
    schema: String,
    #[serde(default)]
    type_passwords: BTreeMap<String, String>,
    archive_keys: BTreeMap<String, ArchiveKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveKey {
    index_key_hex: String,
    data_key_hex: String,
}

/// Import the confirmed native keys.v1 TOML schema. Algorithm options were held
/// by the original per-game profile, not key.toml, and must be supplied explicitly.
/// The destination must be a new file in an existing private directory.
pub fn import_key_profile(
    file: &Path,
    paz_version: u8,
    index_size_xor: u32,
    rc4_skip_crc: bool,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    super::ensure_new_output(output)?;
    let source = File::open(file).map_err(|cause| match cause.kind() {
        ErrorKind::NotFound => "ASTRA_EMU_MUSICA_KEY_MISSING: select an existing key.toml",
        ErrorKind::PermissionDenied => "ASTRA_EMU_MUSICA_KEY_PERMISSION: grant this user read access to the selected key file",
        _ => "ASTRA_EMU_MUSICA_KEY_OPEN: selected key file could not be opened",
    })?;
    if !source.metadata()?.is_file() {
        return Err("ASTRA_EMU_MUSICA_KEY_FILE: select a regular key file".into());
    }
    let mut bytes = Vec::new();
    source.take(MAX_KEY_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|_| "ASTRA_EMU_MUSICA_KEY_READ: check access to the selected key file")?;
    if bytes.len() as u64 > MAX_KEY_BYTES {
        return Err("ASTRA_EMU_MUSICA_KEY_BOUND: key file exceeds 64 KiB".into());
    }
    let profile = decode(&bytes, paz_version, index_size_xor, rc4_skip_crc)?;
    super::write_profile(output, &profile)
}

fn decode(bytes: &[u8], paz_version: u8, index_size_xor: u32, rc4_skip_crc: bool)
    -> Result<MusicaProfile, Box<dyn std::error::Error>>
{
    if paz_version > 2 {
        return Err("ASTRA_EMU_MUSICA_PROFILE_VERSION: select PAZ version 0, 1, or 2".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "ASTRA_EMU_MUSICA_KEY_ENCODING: key file must be UTF-8 TOML")?;
    // Never propagate TOML's Display error: it embeds the offending source line.
    let key: KeyFile = toml::from_str(text).map_err(|_| "ASTRA_EMU_MUSICA_KEY_FORMAT: expected strict native keys.v1 TOML")?;
    if key.schema != KEY_SCHEMA {
        return Err("ASTRA_EMU_MUSICA_KEY_SCHEMA: expected astra.emu.musica.keys.v1".into());
    }
    if key.archive_keys.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != REQUIRED_ARCHIVE_ROLES.into_iter().collect()
    {
        return Err("ASTRA_EMU_MUSICA_KEY_ROLES: key file must contain exactly the eight Musica archive roles".into());
    }
    for (kind, password) in &key.type_passwords {
        if !matches!(kind.as_str(), "png" | "ogg" | "sc" | "avi") {
            return Err("ASTRA_EMU_MUSICA_KEY_TYPE: unknown type password field".into());
        }
        if encoding_rs::SHIFT_JIS.encode(password).2 {
            return Err("ASTRA_EMU_MUSICA_KEY_CP932: type passwords must be representable in CP932".into());
        }
    }
    let roles = key.archive_keys.into_iter().map(|(role, value)| {
        if role == "mov" && !value.data_key_hex.is_empty() {
            return Err("ASTRA_EMU_MUSICA_KEY_MOVIE: native movie data key must be empty".into());
        }
        let index_key = hex_key(&value.index_key_hex, false)?;
        let data_key = hex_key(&value.data_key_hex, role == "mov")?;
        Ok((role, MusicaRolePrivateProfile {
            index_key, data_key, type_passwords: key.type_passwords.clone(), archive_xor: None, video_key: None,
        }))
    }).collect::<Result<BTreeMap<_, _>, Box<dyn std::error::Error>>>()?;
    Ok(MusicaProfile {
        schema: MUSICA_PROFILE_SCHEMA.into(), paz_version, index_size_xor, rc4_skip_crc,
        roles, texture_overrides: BTreeMap::new(),
    })
}

fn hex_key(value: &str, empty_allowed: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if value.is_empty() && empty_allowed { return Ok(Vec::new()); }
    if !(8..=112).contains(&value.len()) || !value.len().is_multiple_of(2)
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("ASTRA_EMU_MUSICA_KEY_HEX: keys must contain 4 to 56 bytes of even-length hex".into());
    }
    Ok(value.as_bytes().chunks_exact(2).map(|pair| {
        fn digit(byte: u8) -> u8 { if byte.is_ascii_digit() { byte - b'0' } else { byte.to_ascii_lowercase() - b'a' + 10 } }
        (digit(pair[0]) << 4) | digit(pair[1])
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> String {
        let mut text = format!("schema = '{KEY_SCHEMA}'\n[type_passwords]\nsc = 'public-fixture'\n");
        for role in REQUIRED_ARCHIVE_ROLES {
            let data = if role == "mov" { "" } else { "05060708" };
            text += &format!("[archive_keys.{role}]\nindex_key_hex = '01020304'\ndata_key_hex = '{data}'\n");
        }
        text
    }
    #[test]
    fn import_preserves_source_requires_explicit_options_and_never_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("key.toml");
        let output = root.path().join("private.json");
        let bytes = fixture();
        std::fs::write(&source, &bytes).unwrap();
        import_key_profile(&source, 2, 123, false, &output).unwrap();
        let imported: MusicaProfile = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(imported.paz_version, 2);
        assert_eq!(imported.index_size_xor, 123);
        assert!(!imported.rc4_skip_crc);
        assert_eq!(imported.roles["bg"].index_key, [1, 2, 3, 4]);
        assert!(import_key_profile(&source, 1, 0, true, &output).is_err());
        assert_eq!(std::fs::read_to_string(source).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
    #[test]
    fn rejects_unknown_schema_fields_roles_and_private_parser_diagnostics() {
        let text = fixture();
        for bad in [text.replace(KEY_SCHEMA, "unknown"), text.replace("[archive_keys.bg]", "[archive_keys.wrong]"), format!("{text}\nprivate-secret = [bad\n")] {
            let error = decode(bad.as_bytes(), 2, 0, true).err().unwrap().to_string();
            assert!(!error.contains("private-secret"));
            assert!(!error.contains("public-fixture"));
        }
        assert!(decode(text.as_bytes(), 3, 0, true).is_err());
        assert!(hex_key("123", false).is_err());
        assert!(hex_key("private-secret", false).is_err());
    }
}
