use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{ErrorKind, Read},
    path::{Component, Path},
};

pub const MUSICA_KEY_SCHEMA: &str = "astra.emu.musica.keys.v2";
pub const MUSICA_KEY_FILE: &str = "key.toml";
pub const MAX_KEY_BYTES: u64 = 1024 * 1024;
pub const MAX_TEXTURE_OVERRIDES: usize = 256;
pub const MAX_TEXTURE_OVERRIDE_URI_BYTES: usize = 512;
pub const MAX_TEXTURE_OVERRIDE_PATH_BYTES: usize = 512;
pub const MAX_TEXTURE_OVERRIDE_BYTES: u64 = 64 * 1024 * 1024;
pub const REQUIRED_ARCHIVE_ROLES: [&str; 8] =
    ["bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"];

const TEXTURE_SOURCE_PREFIX: &str = "musica:/";

const KEY_FILE_ERRORS: [(&str, &str); 7] = [
    (
        "ASTRA_EMU_PROFILE_MISSING",
        "key.toml is missing; select an existing key.toml in game configuration",
    ),
    (
        "ASTRA_EMU_PROFILE_PERMISSION",
        "key.toml access was denied; grant read access or select a readable private copy",
    ),
    (
        "ASTRA_EMU_PROFILE_OPEN",
        "key.toml could not be opened; check the selected file and storage availability",
    ),
    (
        "ASTRA_EMU_PROFILE_FILE",
        "select an existing regular key.toml file",
    ),
    (
        "ASTRA_EMU_PROFILE_READ",
        "key.toml read failed; check file access and storage availability",
    ),
    (
        "ASTRA_EMU_PROFILE_BOUND",
        "key.toml exceeds 1 MiB; select the private key file rather than an archive or scheme database",
    ),
    (
        "ASTRA_EMU_PROFILE_PATH",
        "select key.toml using an explicit absolute path or a safe game-relative path confined to the game directory",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyFileError {
    pub code: &'static str,
    pub message: &'static str,
}

impl fmt::Display for KeyFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for KeyFileError {}

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
pub fn load_musica_key_file(path: &Path) -> Result<MusicaKeyFile, KeyFileError> {
    let mut file = File::open(path).map_err(key_io_error)?;
    if !file.metadata().map_err(key_io_error)?.is_file() {
        return Err(file_error("ASTRA_EMU_PROFILE_FILE"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_KEY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| file_error("ASTRA_EMU_PROFILE_READ"))?;
    parse_key_file(&bytes)
}

pub fn parse_key_file(bytes: &[u8]) -> Result<MusicaKeyFile, KeyFileError> {
    if bytes.len() as u64 > MAX_KEY_BYTES {
        return Err(file_error("ASTRA_EMU_PROFILE_BOUND"));
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

/// Returns the redacted key-file error for a profile access code, if `code` is one.
pub fn key_file_error(code: &str) -> Option<KeyFileError> {
    KEY_FILE_ERRORS
        .iter()
        .find(|(known, _)| *known == code)
        .map(|&(code, message)| KeyFileError { code, message })
}

fn file_error(code: &'static str) -> KeyFileError {
    key_file_error(code).expect("key file error code must be selected internally")
}

fn key_io_error(cause: std::io::Error) -> KeyFileError {
    file_error(match cause.kind() {
        ErrorKind::NotFound => "ASTRA_EMU_PROFILE_MISSING",
        ErrorKind::PermissionDenied => "ASTRA_EMU_PROFILE_PERMISSION",
        _ => "ASTRA_EMU_PROFILE_OPEN",
    })
}

impl MusicaKeyFile {
    /// Checks private key metadata and resource path syntax without filesystem IO.
    pub fn validate(&self) -> Result<(), KeyFileError> {
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

    fn validate_texture_overrides(&self) -> Result<(), KeyFileError> {
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
            if !valid_texture_source(source) {
                return Err(invalid(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE",
                    "texture override source URI is invalid",
                ));
            }
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
}

// Same rules as the SDK archive URI check for the Musica mount prefix.
fn valid_texture_source(uri: &str) -> bool {
    let Some(relative) = uri.strip_prefix(TEXTURE_SOURCE_PREFIX) else {
        return false;
    };
    !relative.is_empty()
        && !relative.starts_with('/')
        && !relative.ends_with('/')
        && !relative.contains(':')
        && !uri.contains('\\')
        && !relative
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        && !uri.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
}

fn validate_hex(
    hex: &str,
    min_bytes: usize,
    max_bytes: usize,
    bound_code: &'static str,
) -> Result<(), KeyFileError> {
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

fn invalid(code: &'static str, message: &'static str) -> KeyFileError {
    KeyFileError { code, message }
}

#[cfg(test)]
mod tests;
