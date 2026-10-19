use astra_emu_musica_keys::{MusicaArchiveKey, MusicaKeyFile, MUSICA_KEY_SCHEMA};
use serde::{
    de::{MapAccess, Visitor},
    Deserialize, Deserializer,
};
use std::{collections::BTreeMap, fmt, fs::File, io::Read, marker::PhantomData, path::Path};

const PROFILE_SCHEMA: &str = "astra.emu.musica.profile.v1";
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

// The retired JSON DTOs belong to this converter alone. Never derive Debug:
// these values contain private keys and passwords.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProfile {
    schema: String,
    paz_version: u8,
    index_size_xor: u32,
    #[serde(default = "legacy_rc4_skip_crc")]
    rc4_skip_crc: bool,
    #[serde(deserialize_with = "unique_map")]
    roles: BTreeMap<String, LegacyRole>,
    #[serde(default, deserialize_with = "unique_map")]
    texture_overrides: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRole {
    index_key: Vec<u8>,
    data_key: Vec<u8>,
    #[serde(deserialize_with = "unique_map")]
    type_passwords: BTreeMap<String, String>,
    archive_xor: Option<u32>,
    video_key: Option<Vec<u8>>,
}

// profile.v1 defined an omitted rc4_skip_crc as true. Preserve that schema's
// behavior here; production keys.v2 requires the field explicitly.
fn legacy_rc4_skip_crc() -> bool {
    true
}

fn unique_map<'de, D, V>(deserializer: D) -> Result<BTreeMap<String, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct UniqueMap<V>(PhantomData<V>);
    impl<'de, V: Deserialize<'de>> Visitor<'de> for UniqueMap<V> {
        type Value = BTreeMap<String, V>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an object with unique keys")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, V>()? {
                if values.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate private map key"));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(UniqueMap(PhantomData))
}

pub fn import_json_profile(file: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    super::ensure_new_output(output)?;
    let source = File::open(file)
        .map_err(|_| "ASTRA_EMU_PROFILE_OPEN: check access to the selected JSON profile")?;
    if !source
        .metadata()
        .map_err(|_| "ASTRA_EMU_PROFILE_METADATA")?
        .is_file()
    {
        return Err("ASTRA_EMU_PROFILE_FILE: select a regular JSON profile file".into());
    }
    let mut bytes = Vec::new();
    source
        .take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "ASTRA_EMU_PROFILE_READ: check access to the selected JSON profile")?;
    super::write_profile(output, &decode(&bytes)?)
}

fn decode(bytes: &[u8]) -> Result<MusicaKeyFile, Box<dyn std::error::Error>> {
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err("ASTRA_EMU_PROFILE_BOUND: JSON profile exceeds 1 MiB".into());
    }
    // JSON diagnostics can contain private field names and values; discard them.
    let profile: LegacyProfile = serde_json::from_slice(bytes)
        .map_err(|_| "ASTRA_EMU_PROFILE_FORMAT: expected strict profile.v1 JSON")?;
    if profile.schema != PROFILE_SCHEMA {
        return Err("ASTRA_EMU_PROFILE_SCHEMA: expected astra.emu.musica.profile.v1".into());
    }
    let key = MusicaKeyFile {
        schema: MUSICA_KEY_SCHEMA.into(),
        paz_version: profile.paz_version,
        index_size_xor: profile.index_size_xor,
        rc4_skip_crc: profile.rc4_skip_crc,
        archive_keys: profile
            .roles
            .into_iter()
            .map(|(role, value)| {
                (
                    role,
                    MusicaArchiveKey {
                        index_key_hex: super::encode_hex(&value.index_key),
                        data_key_hex: super::encode_hex(&value.data_key),
                        type_passwords: value.type_passwords,
                        archive_xor: value.archive_xor,
                        video_key_hex: value.video_key.map(|bytes| super::encode_hex(&bytes)),
                    },
                )
            })
            .collect(),
        texture_overrides: profile.texture_overrides,
    };
    key.validate()?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_musica_keys::{load_musica_key_file, REQUIRED_ARCHIVE_ROLES};
    use serde_json::{json, Value};

    fn fixture() -> Value {
        let roles = REQUIRED_ARCHIVE_ROLES.into_iter().enumerate().map(|(index, role)| {
            (role.to_owned(), json!({
                "index_key": [1, 2, 3, 4],
                "data_key": if role == "mov" { vec![] } else { vec![5, 6, 7, 8] },
                "type_passwords": {"sc": format!("public-fixture-{index}"), "png": "画像"},
                "archive_xor": 123,
                "video_key": if role == "mov" { Some((0..=255).collect::<Vec<u8>>()) } else { None },
            }))
        }).collect::<BTreeMap<_, _>>();
        json!({
            "schema": PROFILE_SCHEMA,
            "paz_version": 2,
            "index_size_xor": 123,
            "rc4_skip_crc": false,
            "roles": roles,
            "texture_overrides": {"musica:/bg/public.png": "overrides/public.png"},
        })
    }

    #[test]
    fn conversion_preserves_every_option_without_game_access() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("legacy.json");
        let output = root.path().join("key.toml");
        let original = serde_json::to_vec(&fixture()).unwrap();
        std::fs::write(&source, &original).unwrap();
        import_json_profile(&source, &output).unwrap();
        let imported = load_musica_key_file(&output).unwrap();
        assert_eq!(imported.schema, MUSICA_KEY_SCHEMA);
        assert_eq!(imported.paz_version, 2);
        assert_eq!(imported.index_size_xor, 123);
        assert!(!imported.rc4_skip_crc);
        for (index, role) in REQUIRED_ARCHIVE_ROLES.into_iter().enumerate() {
            let key = &imported.archive_keys[role];
            assert_eq!(key.index_key_hex, "01020304");
            assert_eq!(
                key.data_key_hex,
                if role == "mov" { "" } else { "05060708" }
            );
            assert_eq!(key.type_passwords["sc"], format!("public-fixture-{index}"));
            assert_eq!(key.type_passwords["png"], "画像");
            assert_eq!(key.archive_xor, Some(123));
            assert_eq!(
                key.video_key_hex.as_deref(),
                if role == "mov" {
                    Some(super::super::encode_hex(&(0..=255).collect::<Vec<u8>>()))
                } else {
                    None
                }
                .as_deref()
            );
        }
        assert_eq!(
            imported.texture_overrides["musica:/bg/public.png"],
            "overrides/public.png"
        );
        assert!(import_json_profile(&source, &output).is_err());
        assert!(import_json_profile(&source, &source).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn preserves_historical_absent_field_semantics() {
        let mut value = fixture();
        value.as_object_mut().unwrap().remove("rc4_skip_crc");
        value.as_object_mut().unwrap().remove("texture_overrides");
        for role in value["roles"].as_object_mut().unwrap().values_mut() {
            role.as_object_mut().unwrap().remove("archive_xor");
            role.as_object_mut().unwrap().remove("video_key");
        }
        let key = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(key.rc4_skip_crc);
        assert!(key.texture_overrides.is_empty());
        assert!(key
            .archive_keys
            .values()
            .all(|role| role.archive_xor.is_none() && role.video_key_hex.is_none()));
    }

    #[test]
    fn rejects_unknown_missing_duplicate_and_invalid_private_fields() {
        let original = fixture();
        let mut invalid = Vec::new();
        let mut value = original.clone();
        value["schema"] = json!("private-secret");
        invalid.push(serde_json::to_vec(&value).unwrap());
        for field in ["schema", "paz_version", "index_size_xor", "roles"] {
            let mut value = original.clone();
            value.as_object_mut().unwrap().remove(field);
            invalid.push(serde_json::to_vec(&value).unwrap());
        }
        for field in ["private-secret", "index_key"] {
            let mut value = original.clone();
            value["roles"]["bg"][field] = json!("private-secret");
            invalid.push(serde_json::to_vec(&value).unwrap());
        }
        let text = serde_json::to_string(&original).unwrap();
        invalid.push(
            text.replacen(
                "\"schema\":",
                "\"schema\":\"private-secret\",\"schema\":",
                1,
            )
            .into_bytes(),
        );
        invalid.push(
            text.replace(
                "\"type_passwords\":{",
                "\"type_passwords\":{\"sc\":\"private-secret\",",
            )
            .into_bytes(),
        );
        invalid.push(
            text.replace(
                "\"texture_overrides\":{",
                "\"texture_overrides\":{\"musica:/bg/public.png\":\"private-secret\",",
            )
            .into_bytes(),
        );
        invalid.push(text.replace("\"roles\":{", "\"roles\":{\"bg\":{\"index_key\":[1,2,3,4],\"data_key\":[5,6,7,8],\"type_passwords\":{}},").into_bytes());
        for bytes in invalid {
            let error = decode(&bytes).err().unwrap().to_string();
            assert!(!error.contains("private-secret"));
            assert!(!error.contains("public-fixture"));
        }
        assert!(decode(&vec![b' '; MAX_PROFILE_BYTES as usize + 1]).is_err());
        assert!(decode(b"{\"private-secret\":").is_err());
    }
}
