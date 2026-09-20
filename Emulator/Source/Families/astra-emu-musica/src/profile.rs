use crate::{
    CoreError, MusicaMountedVfs, MusicaPazDecryptProvider, PazArchiveConfig, PazRoleScheme,
    REQUIRED_ARCHIVE_ROLES,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MUSICA_PROFILE_SCHEMA: &str = "astra.emu.musica.profile.v1";
pub const MUSICA_PROFILE_FILE: &str = "musica.profile.json";
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
const MAX_TEXTURE_OVERRIDES: usize = 256;
const MAX_TEXTURE_OVERRIDE_URI_BYTES: usize = 512;
const MAX_TEXTURE_OVERRIDE_PATH_BYTES: usize = 512;
const MAX_TEXTURE_OVERRIDE_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) type MusicaTextureOverrides = BTreeMap<String, PathBuf>;

/// Local-private scheme. Never log this value or persist it in a public report.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicaProfile {
    pub schema: String,
    pub paz_version: u8,
    pub index_size_xor: u32,
    pub roles: BTreeMap<String, MusicaRolePrivateProfile>,
    #[serde(default)]
    pub texture_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicaRolePrivateProfile {
    pub index_key: Vec<u8>,
    pub data_key: Vec<u8>,
    pub type_passwords: BTreeMap<String, String>,
    pub archive_xor: Option<u32>,
    pub video_key: Option<Vec<u8>>,
}

/// Opens a bounded private JSON profile and archives owned by the Musica core.
pub fn mount_musica(game_root: &Path, profile_path: &Path) -> Result<MusicaMountedVfs, CoreError> {
    mount_musica_with_texture_overrides(game_root, profile_path).map(|(archive, _)| archive)
}

/// Opens a profile and resolves its explicit pixel replacement paths. The
/// archive remains the authority for logical geometry; this map only carries
/// already-canonicalized replacement files into the Family scene.
pub(crate) fn mount_musica_with_texture_overrides(
    game_root: &Path,
    profile_path: &Path,
) -> Result<(MusicaMountedVfs, MusicaTextureOverrides), CoreError> {
    let (profile, hash, root) = astra_emu_sdk::read_game_profile::<MusicaProfile>(
        game_root,
        profile_path,
        MAX_PROFILE_BYTES,
    )?;
    if profile.paz_version > 2 {
        return Err(invalid(
            "ASTRA_EMU_MUSICA_PROFILE_VERSION",
            "PAZ version is unsupported",
        ));
    }
    let texture_overrides = profile.resolve_texture_overrides(&root)?;
    let configs = REQUIRED_ARCHIVE_ROLES
        .iter()
        .map(|role| PazArchiveConfig {
            role: (*role).into(),
            path: root.join(format!("{role}.paz")),
            game_root: root.clone(),
            version: profile.paz_version,
            index_size_xor: profile.index_size_xor,
        })
        .collect();
    let xor = profile.index_size_xor;
    let decrypt = MusicaPazDecryptProvider::new(hash, profile.into_schemes(xor)?)?;
    MusicaMountedVfs::mount("musica.local", "musica:/", configs, Arc::new(decrypt), hash)
        .map(|archive| (archive, texture_overrides))
}

impl MusicaProfile {
    fn resolve_texture_overrides(&self, root: &Path) -> Result<MusicaTextureOverrides, CoreError> {
        if self.texture_overrides.len() > MAX_TEXTURE_OVERRIDES {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_COUNT",
                "too many texture overrides are configured",
            ));
        }
        self.texture_overrides
            .iter()
            .map(|(source, replacement)| {
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
                if replacement.len() > MAX_TEXTURE_OVERRIDE_PATH_BYTES
                    || !Path::new(replacement)
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
                {
                    return Err(invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT",
                        "texture replacements must be PNG files",
                    ));
                }
                let path = astra_emu_sdk::resolve_game_file(root, replacement).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
                        "texture replacement path is missing or unsafe",
                    )
                })?;
                let size = fs::metadata(&path).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
                        "texture replacement path is missing or unsafe",
                    )
                })?;
                if size.len() == 0 || size.len() > MAX_TEXTURE_OVERRIDE_BYTES {
                    return Err(invalid(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_BOUND",
                        "texture replacement exceeds the supported size",
                    ));
                }
                Ok((source.clone(), path))
            })
            .collect()
    }

    fn into_schemes(self, expected_xor: u32) -> Result<BTreeMap<String, PazRoleScheme>, CoreError> {
        if self.schema != MUSICA_PROFILE_SCHEMA {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_PRIVATE_SCHEMA",
                "Musica private payload schema is invalid",
            ));
        }
        let roles = self
            .roles
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if roles != REQUIRED_ARCHIVE_ROLES.into_iter().collect() {
            return Err(invalid(
                "ASTRA_EMU_MUSICA_PRIVATE_ROLES",
                "Musica private payload does not contain the required archive roles",
            ));
        }
        self.roles
            .into_iter()
            .map(|(role, value)| {
                if value.archive_xor.is_some_and(|xor| xor != expected_xor) {
                    return Err(invalid(
                        "ASTRA_EMU_MUSICA_PRIVATE_XOR",
                        "Musica private archive XOR does not match mount options",
                    ));
                }
                let video_key = value
                    .video_key
                    .map(|key| {
                        key.try_into().map_err(|_| {
                            invalid(
                                "ASTRA_EMU_MUSICA_PRIVATE_VIDEO_KEY",
                                "Musica video key must contain exactly 256 bytes",
                            )
                        })
                    })
                    .transpose()?;
                if value
                    .type_passwords
                    .keys()
                    .any(|key| !matches!(key.as_str(), "png" | "ogg" | "sc" | "avi"))
                {
                    return Err(invalid(
                        "ASTRA_EMU_MUSICA_PRIVATE_TYPE_KEY",
                        "Musica private payload contains an unknown type key",
                    ));
                }
                Ok((
                    role,
                    PazRoleScheme {
                        index_key: value.index_key,
                        data_key: value.data_key,
                        type_passwords: value.type_passwords,
                        archive_xor: value.archive_xor,
                        video_key,
                    },
                ))
            })
            .collect()
    }
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_rejects_external_or_oversized_input_without_creating_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            mount_musica(root.path(), outside.path())
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_PROFILE_PATH"
        );
        let file = root.path().join(MUSICA_PROFILE_FILE);
        std::fs::write(&file, vec![0; MAX_PROFILE_BYTES as usize + 1]).unwrap();
        assert_eq!(
            mount_musica(root.path(), &file).err().unwrap().code(),
            "ASTRA_EMU_PROFILE_BOUND"
        );
        assert_eq!(
            std::fs::metadata(file).unwrap().len(),
            MAX_PROFILE_BYTES + 1
        );
    }
    #[test]
    fn role_schema_requires_exact_roles_and_valid_video_key() {
        let roles = REQUIRED_ARCHIVE_ROLES
            .iter()
            .map(|role| {
                (
                    (*role).to_owned(),
                    MusicaRolePrivateProfile {
                        index_key: vec![1; 16],
                        data_key: vec![2; 16],
                        type_passwords: BTreeMap::new(),
                        archive_xor: None,
                        video_key: None,
                    },
                )
            })
            .collect();
        let mut profile = MusicaProfile {
            schema: MUSICA_PROFILE_SCHEMA.into(),
            paz_version: 0,
            index_size_xor: 0,
            roles,
            texture_overrides: BTreeMap::new(),
        };
        profile.clone().into_schemes(0).unwrap();
        profile.roles.get_mut("mov").unwrap().video_key = Some(vec![0; 255]);
        assert_eq!(
            profile.clone().into_schemes(0).err().unwrap().code(),
            "ASTRA_EMU_MUSICA_PRIVATE_VIDEO_KEY"
        );
        profile.roles.remove("bg");
        assert_eq!(
            profile.into_schemes(0).err().unwrap().code(),
            "ASTRA_EMU_MUSICA_PRIVATE_ROLES"
        );
    }

    #[test]
    fn texture_override_profile_resolves_safe_png_paths_and_rejects_invalid_values() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("hd")).unwrap();
        std::fs::write(root.path().join("hd/BG.png"), [1, 2, 3]).unwrap();
        let mut profile = MusicaProfile {
            schema: MUSICA_PROFILE_SCHEMA.into(),
            paz_version: 0,
            index_size_xor: 0,
            roles: BTreeMap::new(),
            texture_overrides: BTreeMap::from([("musica:/bg/BG.png".into(), "hd/BG.png".into())]),
        };
        let resolved = profile.resolve_texture_overrides(root.path()).unwrap();
        assert!(resolved
            .get("musica:/bg/BG.png")
            .is_some_and(|path| path.ends_with("hd/BG.png")));
        profile
            .texture_overrides
            .insert("musica:/bg/BG.png".into(), "../BG.png".into());
        assert_eq!(
            profile
                .resolve_texture_overrides(root.path())
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH"
        );
        profile
            .texture_overrides
            .insert("musica:/bg/BG.png".into(), "hd/BG.jpg".into());
        assert_eq!(
            profile
                .resolve_texture_overrides(root.path())
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT"
        );
    }
}
