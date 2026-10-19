use crate::{
    CoreError, MusicaMountedVfs, MusicaPazDecryptProvider, PazArchiveConfig, PazRoleScheme,
};
use astra_emu_musica_keys::{
    key_file_error, parse_key_file, KeyFileError, MusicaKeyFile, MAX_KEY_BYTES,
    MAX_TEXTURE_OVERRIDE_BYTES, REQUIRED_ARCHIVE_ROLES,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type MusicaTextureOverrides = BTreeMap<String, PathBuf>;

fn core_error(e: KeyFileError) -> CoreError {
    CoreError::invalid(e.code, e.message)
}

fn configured_key_error(error: CoreError) -> CoreError {
    key_file_error(error.code()).map_or(error, core_error)
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
    mount_musica_with_resources(
        game_root,
        profile_path,
        &crate::MusicaResourcePaths::default(),
    )
}

/// Mount standard PAZ readers with explicit physical script/system aliases.
pub fn mount_musica_with_resources(
    game_root: &Path,
    profile_path: &Path,
    resources: &crate::MusicaResourcePaths,
) -> Result<(MusicaMountedVfs, MusicaTextureOverrides), CoreError> {
    let (bytes, hash, root) =
        astra_emu_sdk::read_configured_profile_bytes(game_root, profile_path, MAX_KEY_BYTES)
            .map_err(configured_key_error)?;
    let key = parse_key_file(&bytes).map_err(core_error)?;
    let texture_overrides = resolve_texture_overrides(&key, &root)?;
    let configs = REQUIRED_ARCHIVE_ROLES
        .iter()
        .map(|role| {
            let alias = match *role {
                "scr" => resources.scr_archive.as_str(),
                "sys" => resources.sys_archive.as_str(),
                _ => "",
            };
            let path = if alias.is_empty() {
                root.join(format!("{role}.paz"))
            } else {
                crate::resources::resource_file(&root, alias)?
            };
            Ok(PazArchiveConfig {
                role: (*role).into(),
                path,
                game_root: root.clone(),
                version: key.paz_version,
                index_size_xor: key.index_size_xor,
            })
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    let decrypt = MusicaPazDecryptProvider::new(hash, into_schemes(&key))?;
    let mut archive =
        MusicaMountedVfs::mount("musica.local", "musica:/", configs, Arc::new(decrypt), hash)?;
    archive.set_script_directory(&root, &resources.script_directory)?;
    Ok((archive, texture_overrides))
}

pub(crate) fn resolve_texture_overrides(
    key: &MusicaKeyFile,
    root: &Path,
) -> Result<MusicaTextureOverrides, CoreError> {
    key.texture_overrides
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
fn into_schemes(key_file: &MusicaKeyFile) -> BTreeMap<String, PazRoleScheme> {
    key_file
        .archive_keys
        .iter()
        .map(|(role, key)| {
            let video_key = key.video_key_hex.as_ref().map(|hex| {
                std::array::from_fn(|index| {
                    decode_hex_byte(&hex.as_bytes()[index * 2..index * 2 + 2])
                })
            });
            (
                role.clone(),
                PazRoleScheme {
                    index_key: decode_hex(&key.index_key_hex),
                    data_key: decode_hex(&key.data_key_hex),
                    rc4_skip_crc: key_file.rc4_skip_crc,
                    type_passwords: key.type_passwords.clone(),
                    archive_xor: key.archive_xor,
                    video_key,
                },
            )
        })
        .collect()
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
