use crate::{
    MinoriError, MinoriMountedVfs, MinoriPazDecryptProvider, PazArchiveConfig, PazRoleScheme,
    REQUIRED_ARCHIVE_ROLES,
};
use astra_core::Hash256;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
    sync::Arc,
};

pub const MINORI_PROFILE_SCHEMA: &str = "astra.emu.minori.profile.v1";
pub const MINORI_PROFILE_FILE: &str = "minori.profile.json";
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

/// Local-private scheme. Never log this value or persist it in a public report.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinoriProfile {
    pub schema: String,
    pub paz_version: u8,
    pub index_size_xor: u32,
    pub roles: BTreeMap<String, MinoriRolePrivateProfile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinoriRolePrivateProfile {
    pub index_key: Vec<u8>,
    pub data_key: Vec<u8>,
    pub type_passwords: BTreeMap<String, String>,
    pub archive_xor: Option<u32>,
    pub video_key: Option<Vec<u8>>,
}

/// Opens a bounded private JSON profile and archives owned by the Minori core.
pub fn mount_minori(
    game_root: &Path,
    profile_path: &Path,
) -> Result<MinoriMountedVfs, MinoriError> {
    let root = game_root.canonicalize().map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GAME_ROOT",
            "game directory cannot be opened",
        )
    })?;
    let path = profile_path.canonicalize().map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PROFILE_OPEN",
            "private profile cannot be opened",
        )
    })?;
    if !path.starts_with(&root) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_PROFILE_PATH",
            "private profile must be inside the game directory",
        ));
    }
    let file = File::open(path).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PROFILE_OPEN",
            "private profile cannot be opened",
        )
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_PROFILE_READ",
                "private profile cannot be read",
            )
        })?;
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_PROFILE_BOUND",
            "private profile exceeds the size limit",
        ));
    }
    let profile: MinoriProfile = serde_json::from_slice(&bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PROFILE_FORMAT",
            "private profile JSON is invalid",
        )
    })?;
    if profile.paz_version > 2 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_PROFILE_VERSION",
            "PAZ version is unsupported",
        ));
    }
    let hash = Hash256::from_sha256(&bytes);
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
    let decrypt = MinoriPazDecryptProvider::new(hash, profile.into_schemes(xor)?)?;
    MinoriMountedVfs::mount("minori.local", "minori:/", configs, Arc::new(decrypt), hash)
}

impl MinoriProfile {
    fn into_schemes(
        self,
        expected_xor: u32,
    ) -> Result<BTreeMap<String, PazRoleScheme>, MinoriError> {
        if self.schema != MINORI_PROFILE_SCHEMA {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PRIVATE_SCHEMA",
                "Minori private payload schema is invalid",
            ));
        }
        let roles = self
            .roles
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if roles != REQUIRED_ARCHIVE_ROLES.into_iter().collect() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PRIVATE_ROLES",
                "Minori private payload does not contain the required archive roles",
            ));
        }
        self.roles
            .into_iter()
            .map(|(role, value)| {
                if value.archive_xor.is_some_and(|xor| xor != expected_xor) {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_PRIVATE_XOR",
                        "Minori private archive XOR does not match mount options",
                    ));
                }
                let video_key = value
                    .video_key
                    .map(|key| {
                        key.try_into().map_err(|_| {
                            invalid(
                                "ASTRA_EMU_MINORI_PRIVATE_VIDEO_KEY",
                                "Minori video key must contain exactly 256 bytes",
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
                        "ASTRA_EMU_MINORI_PRIVATE_TYPE_KEY",
                        "Minori private payload contains an unknown type key",
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

fn invalid(code: &'static str, message: &'static str) -> MinoriError {
    MinoriError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_rejects_external_or_oversized_input_without_creating_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            mount_minori(root.path(), outside.path())
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_MINORI_PROFILE_PATH"
        );
        let file = root.path().join(MINORI_PROFILE_FILE);
        std::fs::write(&file, vec![0; MAX_PROFILE_BYTES as usize + 1]).unwrap();
        assert_eq!(
            mount_minori(root.path(), &file).err().unwrap().code(),
            "ASTRA_EMU_MINORI_PROFILE_BOUND"
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
                    MinoriRolePrivateProfile {
                        index_key: vec![1; 16],
                        data_key: vec![2; 16],
                        type_passwords: BTreeMap::new(),
                        archive_xor: None,
                        video_key: None,
                    },
                )
            })
            .collect();
        let mut profile = MinoriProfile {
            schema: MINORI_PROFILE_SCHEMA.into(),
            paz_version: 0,
            index_size_xor: 0,
            roles,
        };
        profile.clone().into_schemes(0).unwrap();
        profile.roles.get_mut("mov").unwrap().video_key = Some(vec![0; 255]);
        assert_eq!(
            profile.clone().into_schemes(0).err().unwrap().code(),
            "ASTRA_EMU_MINORI_PRIVATE_VIDEO_KEY"
        );
        profile.roles.remove("bg");
        assert_eq!(
            profile.into_schemes(0).err().unwrap().code(),
            "ASTRA_EMU_MINORI_PRIVATE_ROLES"
        );
    }
}
