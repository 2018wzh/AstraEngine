use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

use astra_core::Hash256;
use astra_emu_family_core::{
    read_private_file, LegacyCoreError, LegacyMountedVfs, LegacyVfsFamilyFactory,
    LegacyVfsMountContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    MinoriLocaleHook, MinoriMountedVfs, MinoriNls, MinoriPazDecryptor, PazArchiveConfig,
    PazRoleScheme, MINORI_FAMILY_OPTIONS_SCHEMA, MINORI_LOCALE_HOOK_ID, MINORI_ORIGINAL_VARIANT_ID,
    REQUIRED_ARCHIVE_ROLES,
};

pub const MINORI_KEY_FILE_SCHEMA: &str = "astra.emu.minori.keys.v1";
pub const MAX_KEY_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MinoriFamilyOptions {
    pub content_variant: String,
    pub locale_hook: String,
    /// FVP-compatible profile selector. The Japanese source currently
    /// requires `shift_jis`; `gbk` and `utf8` are reserved and fail closed at
    /// mount until a verified localized source contract exists.
    pub nls: String,
    pub paz_version: u8,
    pub index_size_xor: u32,
    pub key_file: PathBuf,
    pub archive_roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinoriKeyFile {
    schema: String,
    #[serde(default)]
    type_passwords: MinoriTypePasswords,
    archive_keys: BTreeMap<String, MinoriArchiveKeyFile>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinoriTypePasswords {
    png: Option<String>,
    ogg: Option<String>,
    sc: Option<String>,
    avi: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinoriArchiveKeyFile {
    index_key_hex: String,
    data_key_hex: String,
}

#[derive(Debug, Default)]
pub struct MinoriVfsFamilyFactory;

impl LegacyVfsFamilyFactory for MinoriVfsFamilyFactory {
    fn family_id(&self) -> &str {
        "minori"
    }

    fn family_options_schema_id(&self) -> &str {
        MINORI_FAMILY_OPTIONS_SCHEMA
    }

    fn family_options_schema_hash(&self) -> Hash256 {
        Hash256::from_sha256(MINORI_FAMILY_OPTIONS_SCHEMA.as_bytes())
    }

    fn mount(
        &self,
        context: &LegacyVfsMountContext,
    ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError> {
        if context.family_config.schema_id != MINORI_FAMILY_OPTIONS_SCHEMA
            || context.family_config.schema_hash != self.family_options_schema_hash()
            || context.prefix != "minori:/"
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_MOUNT_PROFILE",
                "Minori mount identity or options schema is invalid",
            ));
        }
        let options: MinoriFamilyOptions = serde_json::from_slice(&context.family_config.payload)
            .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_MOUNT_OPTIONS",
                "Minori family options are invalid",
            )
        })?;
        let nls = MinoriNls::parse(&options.nls).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_NLS_INVALID",
                "Minori profile nls must be shift_jis, gbk, or utf8",
            )
        })?;
        validate_options(&options)?;
        if !nls.is_currently_supported() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_NLS_UNSUPPORTED",
                "the selected Minori text encoding is reserved but not implemented",
            ));
        }
        let locale_hook = MinoriLocaleHook::from_id(&options.locale_hook).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_LOCALE_HOOK",
                "Minori requires the Japanese CP932 locale hook",
            )
        })?;
        validate_original_entrypoint(&context.game_root)?;
        let key_bytes =
            read_private_file(&context.game_root, &options.key_file, MAX_KEY_FILE_BYTES)?;
        let key_text = std::str::from_utf8(&key_bytes).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_KEY_ENCODING",
                "Minori key file must be UTF-8 TOML",
            )
        })?;
        let key_file: MinoriKeyFile = toml::from_str(key_text).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_KEY_PARSE",
                "Minori key file is not valid strict TOML",
            )
        })?;
        let schemes = key_file.into_schemes()?;
        let decryptor = Arc::new(MinoriPazDecryptor::new_with_locale(schemes, locale_hook)?);
        let configs = options
            .archive_roles
            .iter()
            .map(|role| PazArchiveConfig {
                role: role.clone(),
                path: context.game_root.join(format!("{role}.paz")),
                game_root: context.game_root.clone(),
                version: options.paz_version,
                index_size_xor: options.index_size_xor,
            })
            .collect();
        Ok(Arc::new(MinoriMountedVfs::mount(
            context.mount_id.clone(),
            context.prefix.clone(),
            configs,
            decryptor,
            context.profile_hash,
        )?))
    }
}

impl MinoriKeyFile {
    fn into_schemes(self) -> Result<BTreeMap<String, PazRoleScheme>, LegacyCoreError> {
        if self.schema != MINORI_KEY_FILE_SCHEMA {
            return Err(invalid(
                "ASTRA_EMU_MINORI_KEY_SCHEMA",
                "Minori key file schema is invalid",
            ));
        }
        let roles = self
            .archive_keys
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if roles != REQUIRED_ARCHIVE_ROLES.into_iter().collect() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_KEY_ROLES",
                "Minori key file does not contain exactly the required archive roles",
            ));
        }
        let type_passwords = [
            ("png", self.type_passwords.png),
            ("ogg", self.type_passwords.ogg),
            ("sc", self.type_passwords.sc),
            ("avi", self.type_passwords.avi),
        ]
        .into_iter()
        .filter_map(|(kind, value)| value.map(|value| (kind.to_owned(), value)))
        .collect::<BTreeMap<_, _>>();
        for password in type_passwords.values() {
            if MinoriLocaleHook::japanese_cp932().encode(password).is_err() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_KEY_CP932",
                    "Minori type password cannot be encoded as CP932",
                ));
            }
        }
        self.archive_keys
            .into_iter()
            .map(|(role, key)| {
                let index_key = decode_hex_key(&key.index_key_hex, false)?;
                let data_key = decode_hex_key(&key.data_key_hex, role != "mov")?;
                if role == "mov" && !key.data_key_hex.is_empty() {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_KEY_MOVIE",
                        "movie archive data_key_hex must be empty",
                    ));
                }
                Ok((
                    role,
                    PazRoleScheme {
                        index_key,
                        data_key,
                        type_passwords: type_passwords.clone(),
                    },
                ))
            })
            .collect()
    }
}

fn decode_hex_key(value: &str, required: bool) -> Result<Vec<u8>, LegacyCoreError> {
    if !value.len().is_multiple_of(2) || value.bytes().any(|byte| !byte.is_ascii_hexdigit()) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_KEY_HEX",
            "Minori key file contains invalid even-length hex",
        ));
    }
    if value.is_empty() && !required {
        return Ok(Vec::new());
    }
    let bytes = hex::decode(value).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_KEY_HEX",
            "Minori key file contains invalid hex",
        )
    })?;
    if !(4..=56).contains(&bytes.len()) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_KEY_LENGTH",
            "Minori Blowfish key length must be between 4 and 56 bytes",
        ));
    }
    Ok(bytes)
}

fn validate_options(options: &MinoriFamilyOptions) -> Result<(), LegacyCoreError> {
    let roles = options
        .archive_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if options.content_variant != MINORI_ORIGINAL_VARIANT_ID
        || options.locale_hook != MINORI_LOCALE_HOOK_ID
        || MinoriNls::parse(&options.nls).is_err()
        || options.paz_version > 2
        || options.key_file.as_os_str().is_empty()
        || options.key_file.is_absolute()
        || options
            .key_file
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || roles.len() != options.archive_roles.len()
        || roles != REQUIRED_ARCHIVE_ROLES.into_iter().collect()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOUNT_OPTIONS",
            "Minori family options violate their contract",
        ));
    }
    Ok(())
}

fn validate_original_entrypoint(game_root: &std::path::Path) -> Result<(), LegacyCoreError> {
    let entrypoint = game_root.join("perseus.exe");
    let metadata = std::fs::symlink_metadata(&entrypoint).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_ORIGINAL_ENTRYPOINT",
            "the Japanese original entrypoint perseus.exe is required",
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_ORIGINAL_ENTRYPOINT",
            "the Minori entrypoint must be a regular non-symlink file",
        ));
    }
    Ok(())
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use astra_emu_family_core::LegacyOpaqueFamilyConfig;

    use super::*;

    fn key_file() -> MinoriKeyFile {
        MinoriKeyFile {
            schema: MINORI_KEY_FILE_SCHEMA.into(),
            type_passwords: MinoriTypePasswords {
                png: Some("画像".into()),
                ogg: None,
                sc: Some("script".into()),
                avi: None,
            },
            archive_keys: REQUIRED_ARCHIVE_ROLES
                .into_iter()
                .map(|role| {
                    (
                        role.into(),
                        MinoriArchiveKeyFile {
                            index_key_hex: "00112233".into(),
                            data_key_hex: if role == "mov" {
                                String::new()
                            } else {
                                "44556677".into()
                            },
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn strict_key_file_accepts_exact_roles_and_cp932_passwords() {
        let schemes = key_file().into_schemes().unwrap();
        assert_eq!(schemes.len(), REQUIRED_ARCHIVE_ROLES.len());
        assert!(schemes["mov"].data_key.is_empty());
        assert_eq!(schemes["scr"].type_passwords["png"], "画像");
    }

    #[test]
    fn strict_key_file_rejects_role_hex_movie_and_cp932_violations() {
        let mut missing = key_file();
        missing.archive_keys.remove("sys");
        assert!(missing.into_schemes().is_err());

        let mut bad_hex = key_file();
        bad_hex.archive_keys.get_mut("scr").unwrap().index_key_hex = "abc".into();
        assert!(bad_hex.into_schemes().is_err());

        let mut movie = key_file();
        movie.archive_keys.get_mut("mov").unwrap().data_key_hex = "00112233".into();
        assert!(movie.into_schemes().is_err());

        let mut password = key_file();
        password.type_passwords.png = Some("🙂".into());
        assert!(password.into_schemes().is_err());
    }

    #[test]
    fn strict_toml_rejects_unknown_fields_and_duplicate_roles() {
        let unknown = "schema = 'astra.emu.minori.keys.v1'\nunknown = true\n[archive_keys]\n";
        assert!(toml::from_str::<MinoriKeyFile>(unknown).is_err());

        let duplicate = "schema = 'astra.emu.minori.keys.v1'\n[archive_keys.scr]\nindex_key_hex='00112233'\ndata_key_hex='44556677'\n[archive_keys.scr]\nindex_key_hex='00112233'\ndata_key_hex='44556677'\n";
        assert!(toml::from_str::<MinoriKeyFile>(duplicate).is_err());
    }

    #[test]
    fn family_options_require_exact_roles_and_safe_key_path() {
        let valid = MinoriFamilyOptions {
            content_variant: MINORI_ORIGINAL_VARIANT_ID.into(),
            locale_hook: MINORI_LOCALE_HOOK_ID.into(),
            nls: crate::MINORI_NLS_SHIFT_JIS.into(),
            paz_version: 2,
            index_size_xor: 0,
            key_file: PathBuf::from("key.toml"),
            archive_roles: REQUIRED_ARCHIVE_ROLES
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };
        assert!(validate_options(&valid).is_ok());

        let mut traversal = valid.clone();
        traversal.key_file = PathBuf::from("../key.toml");
        assert!(validate_options(&traversal).is_err());

        let mut duplicate = valid;
        duplicate.archive_roles[0] = duplicate.archive_roles[1].clone();
        assert!(validate_options(&duplicate).is_err());
    }

    #[test]
    fn family_options_reject_localized_variant_and_non_japanese_hook() {
        let mut options = MinoriFamilyOptions {
            content_variant: MINORI_ORIGINAL_VARIANT_ID.into(),
            locale_hook: MINORI_LOCALE_HOOK_ID.into(),
            nls: crate::MINORI_NLS_SHIFT_JIS.into(),
            paz_version: 2,
            index_size_xor: 0,
            key_file: PathBuf::from("key.toml"),
            archive_roles: REQUIRED_ARCHIVE_ROLES
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };
        options.content_variant = "natsuzora-no-perseus.chs".into();
        assert_eq!(
            validate_options(&options).unwrap_err().code(),
            "ASTRA_EMU_MINORI_MOUNT_OPTIONS"
        );
        options.content_variant = MINORI_ORIGINAL_VARIANT_ID.into();
        options.locale_hook = "astra.emu.minori.locale.gbk.v1".into();
        assert_eq!(
            validate_options(&options).unwrap_err().code(),
            "ASTRA_EMU_MINORI_MOUNT_OPTIONS"
        );
        options.content_variant = MINORI_ORIGINAL_VARIANT_ID.into();
        options.nls = "cp936".into();
        assert_eq!(
            validate_options(&options).unwrap_err().code(),
            "ASTRA_EMU_MINORI_MOUNT_OPTIONS"
        );
    }

    #[test]
    fn family_options_reserve_non_japanese_nls_without_fallback() {
        let options = MinoriFamilyOptions {
            content_variant: MINORI_ORIGINAL_VARIANT_ID.into(),
            locale_hook: MINORI_LOCALE_HOOK_ID.into(),
            nls: "gbk".into(),
            paz_version: 2,
            index_size_xor: 0,
            key_file: PathBuf::from("key.toml"),
            archive_roles: REQUIRED_ARCHIVE_ROLES
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };
        assert!(validate_options(&options).is_ok());
        let parsed = MinoriNls::parse(&options.nls).unwrap();
        assert!(!parsed.is_currently_supported());
        assert_eq!(crate::MINORI_NLS_OPTION, "minori.nls");
    }

    #[test]
    fn mount_blocks_reserved_nls_before_reading_private_game_files() {
        let root = tempfile::tempdir().unwrap();
        let options = MinoriFamilyOptions {
            content_variant: MINORI_ORIGINAL_VARIANT_ID.into(),
            locale_hook: MINORI_LOCALE_HOOK_ID.into(),
            nls: crate::MINORI_NLS_GBK.into(),
            paz_version: 2,
            index_size_xor: 0,
            key_file: PathBuf::from("key.toml"),
            archive_roles: REQUIRED_ARCHIVE_ROLES
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };
        let context = LegacyVfsMountContext {
            game_root: root.path().to_path_buf(),
            profile_id: "profile".into(),
            profile_hash: Hash256::from_sha256(b"profile"),
            mount_id: "mount".into(),
            prefix: "minori:/".into(),
            family_config: LegacyOpaqueFamilyConfig {
                schema_id: MINORI_FAMILY_OPTIONS_SCHEMA.into(),
                schema_hash: Hash256::from_sha256(MINORI_FAMILY_OPTIONS_SCHEMA.as_bytes()),
                payload: serde_json::to_vec(&options).unwrap(),
            },
        };
        let error = MinoriVfsFamilyFactory.mount(&context).err().unwrap();
        assert_eq!(error.code(), "ASTRA_EMU_MINORI_NLS_UNSUPPORTED");
    }

    #[test]
    fn original_entrypoint_must_be_a_regular_file() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            validate_original_entrypoint(root.path())
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_ORIGINAL_ENTRYPOINT"
        );
        std::fs::create_dir(root.path().join("perseus.exe")).unwrap();
        assert_eq!(
            validate_original_entrypoint(root.path())
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_ORIGINAL_ENTRYPOINT"
        );
        std::fs::remove_dir(root.path().join("perseus.exe")).unwrap();
        std::fs::write(root.path().join("perseus.exe"), b"fixture").unwrap();
        assert!(validate_original_entrypoint(root.path()).is_ok());
    }
}
