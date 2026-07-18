use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use astra_core::Hash256;
use astra_emu_family_core::{LegacyCoreError, LegacyOpaqueFamilyConfig, LegacyVfsMountContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const VFS_MOUNT_PROFILE_SCHEMA: &str = "astra.emu.vfs_mount_profile.v1";
pub const MAX_FAMILY_OPTIONS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyVfsMountProfile {
    pub schema: String,
    pub profile_id: String,
    pub family_id: String,
    pub mount_id: String,
    pub prefix: String,
    pub private_patch: PathBuf,
    pub family_options_schema: String,
    pub family_options: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct LoadedMountProfile {
    pub profile: LegacyVfsMountProfile,
    pub profile_hash: Hash256,
    pub family_config: LegacyOpaqueFamilyConfig,
}

impl LoadedMountProfile {
    pub fn mount_context(
        &self,
        game_root: &Path,
    ) -> Result<LegacyVfsMountContext, LegacyCoreError> {
        if !game_root.is_dir() {
            return Err(invalid(
                "ASTRA_EMU_VFS_GAME_ROOT",
                "game root is not a directory",
            ));
        }
        let game_root = game_root
            .canonicalize()
            .map_err(|_| invalid("ASTRA_EMU_VFS_GAME_ROOT", "game root could not be resolved"))?;
        let private_patch = resolve_relative(&game_root, &self.profile.private_patch)?;
        Ok(LegacyVfsMountContext {
            game_root,
            profile_id: self.profile.profile_id.clone(),
            profile_hash: self.profile_hash,
            mount_id: self.profile.mount_id.clone(),
            prefix: self.profile.prefix.clone(),
            private_patch: Some(private_patch),
            family_config: self.family_config.clone(),
        })
    }
}

pub fn load_mount_profile(path: &Path) -> Result<LoadedMountProfile, LegacyCoreError> {
    let bytes = fs::read(path).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PROFILE_IO",
            "mount profile could not be read",
        )
    })?;
    if bytes.len() > MAX_FAMILY_OPTIONS_BYTES * 2 {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_SIZE",
            "mount profile exceeds its byte budget",
        ));
    }
    let profile: LegacyVfsMountProfile = serde_yaml::from_slice(&bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PROFILE_PARSE",
            "mount profile is not valid strict YAML",
        )
    })?;
    validate_profile(&profile)?;
    let payload = serde_json::to_vec(&profile.family_options).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_OPTIONS",
            "family options could not be canonicalized",
        )
    })?;
    if payload.len() > MAX_FAMILY_OPTIONS_BYTES {
        return Err(invalid(
            "ASTRA_EMU_VFS_OPTIONS_SIZE",
            "family options exceed their byte budget",
        ));
    }
    Ok(LoadedMountProfile {
        profile_hash: Hash256::from_sha256(&bytes),
        family_config: LegacyOpaqueFamilyConfig {
            schema_id: profile.family_options_schema.clone(),
            schema_hash: Hash256::from_sha256(profile.family_options_schema.as_bytes()),
            payload,
        },
        profile,
    })
}

fn validate_profile(profile: &LegacyVfsMountProfile) -> Result<(), LegacyCoreError> {
    if profile.schema != VFS_MOUNT_PROFILE_SCHEMA
        || !safe_symbol(&profile.profile_id)
        || !safe_symbol(&profile.family_id)
        || !safe_symbol(&profile.mount_id)
        || !safe_symbol(&profile.family_options_schema)
        || !profile.prefix.ends_with(":/")
        || profile.prefix[..profile.prefix.len() - 2].contains(['/', '\\', ':'])
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_IDENTITY",
            "mount profile identity is invalid",
        ));
    }
    validate_relative(&profile.private_patch)
}

pub fn validate_relative(path: &Path) -> Result<(), LegacyCoreError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_PATH",
            "profile path must be a normalized relative path",
        ));
    }
    Ok(())
}

fn resolve_relative(root: &Path, relative: &Path) -> Result<PathBuf, LegacyCoreError> {
    validate_relative(relative)?;
    let candidate = root.join(relative);
    if !candidate.is_file() {
        return Err(invalid(
            "ASTRA_EMU_VFS_PRIVATE_PATCH",
            "private patch does not exist",
        ));
    }
    let resolved = candidate.canonicalize().map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PRIVATE_PATCH",
            "private patch could not be resolved",
        )
    })?;
    if !resolved.starts_with(root) {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_PATH",
            "profile path resolves outside the game root",
        ));
    }
    Ok(resolved)
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_profile_rejects_unknown_fields_and_traversal() {
        let root = tempfile::tempdir().unwrap();
        let unknown = root.path().join("unknown.yaml");
        std::fs::write(&unknown, "schema: astra.emu.vfs_mount_profile.v1\nprofile_id: p\nfamily_id: f\nmount_id: m\nprefix: 'f:/'\nprivate_patch: patch.luau\nfamily_options_schema: f.v1\nfamily_options: {}\nextra: true\n").unwrap();
        assert_eq!(
            load_mount_profile(&unknown).unwrap_err().code(),
            "ASTRA_EMU_VFS_PROFILE_PARSE"
        );
        assert_eq!(
            validate_relative(Path::new("../patch.luau"))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_PROFILE_PATH"
        );
        assert_eq!(
            validate_relative(Path::new("/patch.luau"))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_PROFILE_PATH"
        );
    }
}
