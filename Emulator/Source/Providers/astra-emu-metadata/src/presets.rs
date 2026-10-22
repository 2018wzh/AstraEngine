use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::{
    is_bounded_text, is_plugin_id, is_vndb_release_id, is_vndb_vn_id, MAX_ENTRIES,
};
use crate::MetadataError;

pub const PRESET_DATABASE_SCHEMA_VERSION: &str = "astra.emu.presets.v1";
const MAX_SUMMARY_CHARS: usize = 120;
const MAX_CONFIGURATION_ITEMS: usize = 256;
const MAX_CONFIGURATION_VALUE_CHARS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetDatabase {
    #[schemars(regex(pattern = r"^astra\.emu\.presets\.v1$"))]
    pub schema: String,
    pub entries: Vec<Preset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    #[schemars(regex(pattern = r"^v[0-9]{1,63}$"))]
    pub vn_id: String,
    #[schemars(regex(pattern = r"^r[0-9]{1,63}$"))]
    pub release_id: String,
    #[schemars(regex(pattern = r"^[a-z0-9._-]{1,128}$"))]
    pub emulator: String,
    #[schemars(length(min = 1, max = 120))]
    pub summary: String,
    pub configuration: BTreeMap<String, String>,
    #[schemars(regex(pattern = r"^[a-z0-9._-]{1,128}$"))]
    pub patch: Option<String>,
}

impl Preset {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if !is_vndb_vn_id(&self.vn_id) {
            return Err(MetadataError::SchemaMismatch("preset.vn_id"));
        }
        if !is_vndb_release_id(&self.release_id) {
            return Err(MetadataError::SchemaMismatch("preset.release_id"));
        }
        if !is_plugin_id(&self.emulator) {
            return Err(MetadataError::SchemaMismatch("preset.emulator"));
        }
        if !is_bounded_text(&self.summary, 1, MAX_SUMMARY_CHARS) {
            return Err(MetadataError::ResponseBounds("preset.summary"));
        }
        if self.configuration.len() > MAX_CONFIGURATION_ITEMS {
            return Err(MetadataError::ResponseBounds("preset.configuration"));
        }
        for (key, value) in &self.configuration {
            if !is_config_field_id(key) {
                return Err(MetadataError::SchemaMismatch("preset.configuration.key"));
            }
            if !is_bounded_text(value, 0, MAX_CONFIGURATION_VALUE_CHARS) {
                return Err(MetadataError::ResponseBounds("preset.configuration.value"));
            }
        }
        if self
            .patch
            .as_deref()
            .is_some_and(|patch| !is_plugin_id(patch))
        {
            return Err(MetadataError::SchemaMismatch("preset.patch"));
        }
        Ok(())
    }
}

impl PresetDatabase {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.schema != PRESET_DATABASE_SCHEMA_VERSION {
            return Err(MetadataError::SchemaMismatch("presets.schema"));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(MetadataError::ResponseBounds("presets.entries"));
        }
        let mut keys = BTreeSet::new();
        for preset in &self.entries {
            preset.validate()?;
            if !keys.insert((preset.release_id.as_str(), preset.emulator.as_str())) {
                return Err(MetadataError::Duplicate(format!(
                    "{}/{}",
                    preset.release_id, preset.emulator
                )));
            }
        }
        Ok(())
    }

    pub fn preset(&self, release_id: &str, emulator: &str) -> Option<&Preset> {
        self.entries
            .iter()
            .find(|preset| preset.release_id == release_id && preset.emulator == emulator)
    }
}

pub fn parse_preset_database(bytes: &[u8]) -> Result<PresetDatabase, MetadataError> {
    let database: PresetDatabase =
        serde_json::from_slice(bytes).map_err(|_| MetadataError::SchemaMismatch("presets.json"))?;
    database.validate()?;
    Ok(database)
}

fn is_config_field_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(release_id: &str, emulator: &str) -> Preset {
        Preset {
            vn_id: "v17".into(),
            release_id: release_id.into(),
            emulator: emulator.into(),
            summary: "简体中文补丁需要 GBK 脚本编码".into(),
            configuration: BTreeMap::from([("script_encoding".into(), "gbk".into())]),
            patch: Some("musica-eden-cn".into()),
        }
    }

    fn database(entries: Vec<Preset>) -> PresetDatabase {
        PresetDatabase {
            schema: PRESET_DATABASE_SCHEMA_VERSION.into(),
            entries,
        }
    }

    #[test]
    fn round_trips_and_finds_preset() {
        let database = database(vec![preset("r1", "astra.emu.musica"), preset("r1", "rfvp")]);
        let parsed = parse_preset_database(&serde_json::to_vec(&database).unwrap()).unwrap();
        assert_eq!(parsed, database);
        assert!(parsed.preset("r1", "rfvp").is_some());
        assert!(parsed.preset("r2", "rfvp").is_none());
    }

    #[test]
    fn rejects_duplicates_and_invalid_fields() {
        assert!(matches!(
            database(vec![
                preset("r1", "astra.emu.musica"),
                preset("r1", "astra.emu.musica")
            ])
            .validate(),
            Err(MetadataError::Duplicate(_))
        ));
        let mut long = preset("r1", "astra.emu.musica");
        long.summary = "长".repeat(121);
        assert!(database(vec![long]).validate().is_err());
        let mut bad_key = preset("r1", "astra.emu.musica");
        bad_key.configuration.insert("bad key".into(), "1".into());
        assert!(database(vec![bad_key]).validate().is_err());
        let mut bad_release = preset("r1", "astra.emu.musica");
        bad_release.release_id = "12345".into();
        assert!(database(vec![bad_release]).validate().is_err());
    }
}
