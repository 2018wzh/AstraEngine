use crate::{
    CoreError, MusicaMountedVfs, MusicaPazDecryptProvider, PazArchiveConfig, PazRoleScheme,
    REQUIRED_ARCHIVE_ROLES,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub const MUSICA_KEY_SCHEMA: &str = "astra.emu.musica.keys.v2";
pub const MUSICA_KEY_FILE: &str = "key.toml";
const MAX_KEY_BYTES: u64 = 1024 * 1024;
const MAX_TEXTURE_OVERRIDES: usize = 256;
const MAX_TEXTURE_OVERRIDE_URI_BYTES: usize = 512;
const MAX_TEXTURE_OVERRIDE_PATH_BYTES: usize = 512;
const MAX_TEXTURE_OVERRIDE_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) type MusicaTextureOverrides = BTreeMap<String, PathBuf>;

/// Local-private keys. Never log this value or persist it in a public report.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicaKeyFile {
    pub schema: String,
    pub paz_version: u8,
    pub index_size_xor: u32,
    pub rc4_skip_crc: bool,
    pub archive_keys: BTreeMap<String, MusicaArchiveKey>,
    #[serde(default)]
    pub texture_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicaArchiveKey {
    pub index_key_hex: String,
    pub data_key_hex: String,
    #[serde(default)]
    pub type_passwords: BTreeMap<String, String>,
    pub archive_xor: Option<u32>,
    pub video_key_hex: Option<String>,
}

/// Reads and validates a bounded UTF-8 key.toml without opening a game or device.
pub fn load_musica_key_file(path: &Path) -> Result<MusicaKeyFile, CoreError> {
    let mut file = File::open(path).map_err(key_io_error)?;
    if !file.metadata().map_err(key_io_error)?.is_file() {
        return Err(key_file_error("ASTRA_EMU_PROFILE_FILE"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_KEY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| key_file_error("ASTRA_EMU_PROFILE_READ"))?;
    parse_key_file(&bytes)
}

fn parse_key_file(bytes: &[u8]) -> Result<MusicaKeyFile, CoreError> {
    if bytes.len() as u64 > MAX_KEY_BYTES {
        return Err(key_file_error("ASTRA_EMU_PROFILE_BOUND"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MUSICA_KEY_ENCODING",
            "key.toml must be UTF-8 TOML; select or convert the private key file",
        )
    })?;
    // TOML diagnostics include source lines, which can contain private keys.
    let key: MusicaKeyFile = toml::from_str(text).map_err(|_| {
        invalid("ASTRA_EMU_PROFILE_FORMAT", "select a canonical keys.v2 key.toml with explicit PAZ version, index XOR and RC4 CRC-skip options; convert legacy files with the Musica developer CLI")
    })?;
    key.validate()?;
    Ok(key)
}

fn key_io_error(cause: std::io::Error) -> CoreError {
    key_file_error(match cause.kind() {
        ErrorKind::NotFound => "ASTRA_EMU_PROFILE_MISSING",
        ErrorKind::PermissionDenied => "ASTRA_EMU_PROFILE_PERMISSION",
        _ => "ASTRA_EMU_PROFILE_OPEN",
    })
}

fn key_file_error(code: &'static str) -> CoreError {
    let message = match code {
        "ASTRA_EMU_PROFILE_MISSING" => {
            "key.toml is missing; select an existing key.toml in game configuration"
        }
        "ASTRA_EMU_PROFILE_PERMISSION" => {
            "key.toml access was denied; grant read access or select a readable private copy"
        }
        "ASTRA_EMU_PROFILE_OPEN" => {
            "key.toml could not be opened; check the selected file and storage availability"
        }
        "ASTRA_EMU_PROFILE_FILE" => "select an existing regular key.toml file",
        "ASTRA_EMU_PROFILE_READ" => {
            "key.toml read failed; check file access and storage availability"
        }
        "ASTRA_EMU_PROFILE_BOUND" => {
            "key.toml exceeds 1 MiB; select the private key file rather than an archive or scheme database"
        }
        "ASTRA_EMU_PROFILE_PATH" => {
            "select key.toml using an explicit absolute path or a safe game-relative path confined to the game directory"
        }
        _ => unreachable!("key file error code must be selected internally"),
    };
    invalid(code, message)
}

fn configured_key_error(error: CoreError) -> CoreError {
    match error.code() {
        "ASTRA_EMU_PROFILE_MISSING" => key_file_error("ASTRA_EMU_PROFILE_MISSING"),
        "ASTRA_EMU_PROFILE_PERMISSION" => key_file_error("ASTRA_EMU_PROFILE_PERMISSION"),
        "ASTRA_EMU_PROFILE_OPEN" => key_file_error("ASTRA_EMU_PROFILE_OPEN"),
        "ASTRA_EMU_PROFILE_FILE" => key_file_error("ASTRA_EMU_PROFILE_FILE"),
        "ASTRA_EMU_PROFILE_READ" => key_file_error("ASTRA_EMU_PROFILE_READ"),
        "ASTRA_EMU_PROFILE_BOUND" => key_file_error("ASTRA_EMU_PROFILE_BOUND"),
        "ASTRA_EMU_PROFILE_PATH" => key_file_error("ASTRA_EMU_PROFILE_PATH"),
        _ => error,
    }
}

/// Opens canonical key.toml and archives owned by the Musica core.
pub fn mount_musica(game_root: &Path, profile_path: &Path) -> Result<MusicaMountedVfs, CoreError> {
    mount_musica_with_texture_overrides(game_root, profile_path).map(|(archive, _)| archive)
}

/// Resource paths remain confined to the game even for an external key.toml.
pub(crate) fn mount_musica_with_texture_overrides(
    game_root: &Path,
    profile_path: &Path,
) -> Result<(MusicaMountedVfs, MusicaTextureOverrides), CoreError> {
    let (bytes, hash, root) =
        astra_emu_sdk::read_configured_profile_bytes(game_root, profile_path, MAX_KEY_BYTES)
            .map_err(configured_key_error)?;
    let key = parse_key_file(&bytes)?;
    let texture_overrides = key.resolve_texture_overrides(&root)?;
    let configs = REQUIRED_ARCHIVE_ROLES
        .iter()
        .map(|role| PazArchiveConfig {
            role: (*role).into(),
            path: root.join(format!("{role}.paz")),
            game_root: root.clone(),
            version: key.paz_version,
            index_size_xor: key.index_size_xor,
        })
        .collect();
    let decrypt = MusicaPazDecryptProvider::new(hash, key.into_schemes())?;
    MusicaMountedVfs::mount("musica.local", "musica:/", configs, Arc::new(decrypt), hash)
        .map(|archive| (archive, texture_overrides))
}

impl MusicaKeyFile {
    /// Checks private key metadata and resource path syntax without filesystem IO.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.schema != MUSICA_KEY_SCHEMA {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_PRIVATE_SCHEMA",
                "select a keys.v2 key.toml; convert legacy schemas with the Musica developer CLI",
            ));
        }
        if self.paz_version > 2 {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_PROFILE_VERSION",
                "key.toml must explicitly select supported PAZ version 0, 1, or 2",
            ));
        }
        if self.archive_keys.len() != REQUIRED_ARCHIVE_ROLES.len()
            || REQUIRED_ARCHIVE_ROLES
                .iter()
                .any(|role| !self.archive_keys.contains_key(*role))
        {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_PRIVATE_ROLES",
                "Musica keys must contain exactly the required archive roles",
            ));
        }
        for (role, key) in &self.archive_keys {
            validate_hex(&key.index_key_hex, 4, 56, "ASTRA_EMU_MUSICA_BLOWFISH_KEY")?;
            validate_hex(
                &key.data_key_hex,
                if role == "mov" { 0 } else { 4 },
                56,
                "ASTRA_EMU_MUSICA_BLOWFISH_KEY",
            )?;
            if key
                .archive_xor
                .is_some_and(|xor| xor != self.index_size_xor)
            {
                return Err(invalid(
                    "ASTRA_EMU_MUSICA_PRIVATE_XOR",
                    "Musica archive XOR does not match mount options",
                ));
            }
            if let Some(video_key) = &key.video_key_hex {
                validate_hex(video_key, 256, 256, "ASTRA_EMU_MUSICA_PRIVATE_VIDEO_KEY")?;
            }
            if key
                .type_passwords
                .keys()
                .any(|kind| !matches!(kind.as_str(), "png" | "ogg" | "sc" | "avi"))
            {
                return Err(invalid(
                    "ASTRA_EMU_MUSICA_PRIVATE_TYPE_KEY",
                    "Musica keys contain an unknown type key",
                ));
            }
        }
        self.validate_texture_overrides()
    }

    fn validate_texture_overrides(&self) -> Result<(), CoreError> {
        if self.texture_overrides.len() > MAX_TEXTURE_OVERRIDES {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_COUNT",
                "too many texture overrides are configured",
            ));
        }
        for (source, replacement) in &self.texture_overrides {
            if source.len() > MAX_TEXTURE_OVERRIDE_URI_BYTES {
                return Err(invalid(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE",
                    "texture override source URI is too long",
                ));
            }
            astra_emu_sdk::validate_archive_uri("musica:/", source).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE",
                    "texture override source URI is invalid",
                )
            })?;
            let path = Path::new(replacement);
            if replacement.len() > MAX_TEXTURE_OVERRIDE_PATH_BYTES
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            {
                return Err(invalid(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT",
                    "texture replacements must be PNG files",
                ));
            }
            if replacement.is_empty()
                || replacement.contains(['\\', ':', '\0'])
                || path.is_absolute()
                || replacement
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || path
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(invalid(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
                    "texture replacement path is unsafe",
                ));
            }
        }
        Ok(())
    }

    fn resolve_texture_overrides(&self, root: &Path) -> Result<MusicaTextureOverrides, CoreError> {
        self.texture_overrides
            .iter()
            .map(|(source, replacement)| {
                let path = astra_emu_sdk::resolve_game_file(root, replacement).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
                        "texture replacement path is missing or unsafe",
                    )
                })?;
                let metadata = fs::metadata(&path).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
                        "texture replacement path is missing or unsafe",
                    )
                })?;
                if metadata.len() == 0 || metadata.len() > MAX_TEXTURE_OVERRIDE_BYTES {
                    return Err(invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_BOUND",
                        "texture replacement exceeds the supported size",
                    ));
                }
                Ok((source.clone(), path))
            })
            .collect()
    }

    // Only called after validation; decode each private key once for decryption.
    fn into_schemes(self) -> BTreeMap<String, PazRoleScheme> {
        self.archive_keys
            .into_iter()
            .map(|(role, key)| {
                let video_key = key.video_key_hex.map(|hex| {
                    std::array::from_fn(|index| {
                        decode_hex_byte(&hex.as_bytes()[index * 2..index * 2 + 2])
                    })
                });
                (
                    role,
                    PazRoleScheme {
                        index_key: decode_hex(&key.index_key_hex),
                        data_key: decode_hex(&key.data_key_hex),
                        rc4_skip_crc: self.rc4_skip_crc,
                        type_passwords: key.type_passwords,
                        archive_xor: key.archive_xor,
                        video_key,
                    },
                )
            })
            .collect()
    }
}

fn validate_hex(
    hex: &str,
    min_bytes: usize,
    max_bytes: usize,
    bound_code: &'static str,
) -> Result<(), CoreError> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            "ASTRA_EMU_MUSICA_KEY_HEX",
            "Musica keys must contain even-length hexadecimal",
        ));
    }
    if !(min_bytes..=max_bytes).contains(&(hex.len() / 2)) {
        return Err(invalid(bound_code, "Musica key length is unsupported"));
    }
    Ok(())
}

fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| decode_hex_byte(pair))
        .collect()
}

fn decode_hex_byte(pair: &[u8]) -> u8 {
    let nibble = |byte: u8| {
        if byte.is_ascii_digit() {
            byte - b'0'
        } else {
            byte.to_ascii_lowercase() - b'a' + 10
        }
    };
    nibble(pair[0]) << 4 | nibble(pair[1])
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
