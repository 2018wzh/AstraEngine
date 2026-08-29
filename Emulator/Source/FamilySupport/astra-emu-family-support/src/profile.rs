use std::{
    fs,
    path::{Component, Path},
};

use astra_core::{is_safe_symbol as safe_symbol, Hash256};
use astra_emu_family_core::{LegacyCoreError, LegacyOpaqueFamilyConfig, LegacyVfsMountContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical launch-profile schema for all AstraEMU family adapters.
pub const FAMILY_LAUNCH_PROFILE_SCHEMA: &str = "astra.emu.family_launch_profile.v1";
pub const MAX_FAMILY_OPTIONS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyFamilyLaunchProfile {
    pub schema: String,
    pub profile_id: String,
    pub family_id: String,
    pub mount_id: String,
    pub prefix: String,
    pub runtime: LegacyRuntimeLaunch,
    pub family_options_schema: String,
    pub family_options: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyRuntimeLaunch {
    pub entry_uri: String,
    pub launch_mode: String,
}

#[derive(Debug, Clone)]
pub struct LoadedLaunchProfile {
    pub profile: LegacyFamilyLaunchProfile,
    pub profile_hash: Hash256,
    pub family_config: LegacyOpaqueFamilyConfig,
}

impl LoadedLaunchProfile {
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
        Ok(LegacyVfsMountContext {
            game_root,
            profile_id: self.profile.profile_id.clone(),
            profile_hash: self.profile_hash,
            mount_id: self.profile.mount_id.clone(),
            prefix: self.profile.prefix.clone(),
            family_config: self.family_config.clone(),
        })
    }
}

pub fn load_launch_profile(path: &Path) -> Result<LoadedLaunchProfile, LegacyCoreError> {
    let bytes = fs::read(path).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PROFILE_IO",
            "launch profile could not be read",
        )
    })?;
    if bytes.is_empty() || bytes.len() > MAX_FAMILY_OPTIONS_BYTES * 2 {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_SIZE",
            "launch profile exceeds its byte budget",
        ));
    }
    let profile: LegacyFamilyLaunchProfile = serde_yaml::from_slice(&bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PROFILE_PARSE",
            "launch profile is not valid strict YAML",
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
    Ok(LoadedLaunchProfile {
        profile_hash: Hash256::from_sha256(&bytes),
        family_config: LegacyOpaqueFamilyConfig {
            schema_id: profile.family_options_schema.clone(),
            schema_hash: Hash256::from_sha256(profile.family_options_schema.as_bytes()),
            payload,
        },
        profile,
    })
}

fn validate_profile(profile: &LegacyFamilyLaunchProfile) -> Result<(), LegacyCoreError> {
    if profile.schema != FAMILY_LAUNCH_PROFILE_SCHEMA
        || !safe_symbol(&profile.profile_id)
        || !safe_symbol(&profile.family_id)
        || !safe_symbol(&profile.mount_id)
        || !safe_symbol(&profile.family_options_schema)
        || profile.prefix != format!("{}:/", profile.family_id)
        || !safe_symbol(&profile.runtime.launch_mode)
        || !matches!(profile.runtime.launch_mode.as_str(), "direct" | "title")
        || profile.runtime.entry_uri.is_empty()
        || profile.runtime.entry_uri.len() > 4096
        || !profile.runtime.entry_uri.starts_with(&profile.prefix)
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_IDENTITY",
            "launch profile identity is invalid",
        ));
    }
    let entry_path = &profile.runtime.entry_uri[profile.prefix.len()..];
    if entry_path.is_empty()
        || entry_path.contains(['\\', '\0', ':'])
        || entry_path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_PROFILE_ENTRY",
            "runtime entry URI is invalid",
        ));
    }
    Ok(())
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

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_yaml(extra: &str) -> String {
        format!(
            "schema: astra.emu.family_launch_profile.v1\nprofile_id: p\nfamily_id: fvp\nmount_id: m\nprefix: 'fvp:/'\nruntime:\n  entry_uri: 'fvp:/scr/main'\n  launch_mode: direct\nfamily_options_schema: fixture.options.v1\nfamily_options: {{}}\n{extra}"
        )
    }

    #[test]
    fn strict_profile_rejects_old_patch_and_unknown_fields() {
        let root = tempfile::tempdir().unwrap();
        let unknown = root.path().join("unknown.yaml");
        fs::write(
            &unknown,
            profile_yaml("private_patch: patch.luau\nextra: true\n"),
        )
        .unwrap();
        assert_eq!(
            load_launch_profile(&unknown).unwrap_err().code(),
            "ASTRA_EMU_VFS_PROFILE_PARSE"
        );
    }

    #[test]
    fn launch_profile_loads_and_validates_entry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("profile.yaml");
        fs::write(&path, profile_yaml("")).unwrap();
        let loaded = load_launch_profile(&path).unwrap();
        assert_eq!(loaded.profile.runtime.entry_uri, "fvp:/scr/main");
        assert!(loaded.mount_context(root.path()).is_ok());
    }

    #[test]
    fn relative_paths_reject_traversal_and_absolute_paths() {
        assert_eq!(
            validate_relative(Path::new("../key.toml"))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_PROFILE_PATH"
        );
        assert_eq!(
            validate_relative(Path::new("/key.toml"))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_PROFILE_PATH"
        );
    }
}
