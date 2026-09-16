use crate::{CmvsArchive, CmvsSchemeProfile};
use astra_emu_sdk::{read_game_profile, resolve_game_file, CoreError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const CMVS_PROFILE_SCHEMA: &str = "astra.emu.cmvs.profile.v1";
pub const CMVS_PROFILE_FILE: &str = "cmvs.profile.json";

/// Local private configuration, never a log or package payload.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CmvsProfile {
    pub schema: String,
    pub scheme: CmvsSchemeProfile,
    pub archives: BTreeMap<String, String>,
    pub loose_files: BTreeMap<String, String>,
}

pub fn mount_cmvs(game_root: &Path, profile_path: &Path) -> Result<CmvsArchive, CoreError> {
    let (profile, hash, root) =
        read_game_profile::<CmvsProfile>(game_root, profile_path, 1024 * 1024)?;
    profile.validate()?;
    let resolve = |files: BTreeMap<String, String>| {
        files
            .into_iter()
            .map(|(role, path)| Ok((role, resolve_game_file(&root, &path)?)))
            .collect::<Result<Vec<_>, CoreError>>()
    };
    let archives = resolve(profile.archives)?;
    let loose = resolve(profile.loose_files)?;
    let mut sources = BTreeSet::new();
    for (_, path) in archives.iter().chain(&loose) {
        if !sources.insert(path) {
            return Err(CoreError::invalid(
                "ASTRA_EMU_CMVS_SOURCE_DUPLICATE",
                "game source is configured more than once",
            ));
        }
    }
    CmvsArchive::mount_with_cache(
        "cmvs.local".into(),
        "cmvs:/".into(),
        archives,
        loose,
        profile.scheme,
        hash,
        hash,
        None,
    )
}

impl CmvsProfile {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.schema != CMVS_PROFILE_SCHEMA
            || self.archives.is_empty()
            || self.archives.len() > 256
            || self.loose_files.len() > 256
            || self
                .archives
                .keys()
                .chain(self.loose_files.keys())
                .any(|role| !astra_core::is_safe_symbol(role))
        {
            return Err(CoreError::invalid(
                "ASTRA_EMU_CMVS_PROFILE",
                "CMVS profile is invalid",
            ));
        }
        self.scheme
            .validate()
            .map_err(|code| CoreError::invalid(code, "CMVS scheme is invalid"))?;
        if self.scheme.version != 5 {
            return Err(CoreError::invalid(
                "ASTRA_EMU_CMVS_CPZ_VERSION",
                "archive mounting currently requires CPZ5",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_scheme_is_rejected_before_opening_resources() {
        let root = tempfile::tempdir().unwrap();
        let profile = serde_json::json!({
            "schema": CMVS_PROFILE_SCHEMA,
            "archives": {"script":"missing.cpz"}, "loose_files": {},
            "scheme": {
                "version":6, "cpz5_secret":vec![0;24], "md5_variant":"A",
                "decoder_factor":0, "entry_init_key":0, "entry_sub_key":0,
                "entry_tail_key":0, "entry_key_pos":0, "index_seed":0,
                "index_addend":0, "index_subtrahend":0, "dir_key_addend":[0,0,0,0]
            }
        });
        let bytes = serde_json::to_vec(&profile).unwrap();
        let path = root.path().join(CMVS_PROFILE_FILE);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            mount_cmvs(root.path(), &path).err().unwrap().code(),
            "ASTRA_EMU_CMVS_CPZ_VERSION"
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
