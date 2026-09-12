//! Typed startup configuration. Empty game ID denotes core defaults.
use crate::{Library, LibraryError};
use astra_emu_family_api::{resolve_config, ConfigEntry, ConfigField};
use rusqlite::{params, OptionalExtension};

impl Library {
    pub fn family_configuration(
        &self,
        plugin_id: &str,
        game_id: &str,
        schema: &[ConfigField],
    ) -> Result<Vec<ConfigEntry>, LibraryError> {
        let mut values = self.read_family_configuration(plugin_id, "")?;
        // Validate each layer before merging: duplicates/unknowns cannot disappear.
        resolve_config(schema, &values).map_err(LibraryError::FamilyConfiguration)?;
        if !game_id.is_empty() {
            let overrides = self.read_family_configuration(plugin_id, game_id)?;
            resolve_config(schema, &overrides).map_err(LibraryError::FamilyConfiguration)?;
            for entry in overrides {
                values.retain(|value| value.id != entry.id);
                values.push(entry);
            }
        }
        Ok(resolve_config(schema, &values)
            .map_err(LibraryError::FamilyConfiguration)?
            .into_vec())
    }

    fn read_family_configuration(
        &self,
        plugin_id: &str,
        game_id: &str,
    ) -> Result<Vec<ConfigEntry>, LibraryError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT values_json FROM family_configuration WHERE plugin_id=?1 AND game_id=?2",
                params![plugin_id, game_id],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(raw) if raw.len() <= 4 * 1024 * 1024 => {
                serde_json::from_str(&raw).map_err(|_| LibraryError::Settings)
            }
            Some(_) => Err(LibraryError::Settings),
            None => Ok(Vec::new()),
        }
    }

    pub fn save_family_configuration(
        &mut self,
        plugin_id: &str,
        game_id: &str,
        schema: &[ConfigField],
        values: &[ConfigEntry],
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(plugin_id)?;
        if !game_id.is_empty() && self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        resolve_config(schema, values).map_err(LibraryError::FamilyConfiguration)?;
        let raw = serde_json::to_string(values).map_err(|_| LibraryError::Serialization)?;
        self.connection.execute("INSERT INTO family_configuration(plugin_id, game_id, values_json) VALUES(?1, ?2, ?3) ON CONFLICT(plugin_id, game_id) DO UPDATE SET values_json=excluded.values_json", params![plugin_id, game_id, raw])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{ConfigKind, ConfigValue};

    #[test]
    fn persistence_defaults_overrides_reset_and_failed_write_preservation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manager.sqlite");
        let mut library = Library::open(&path).unwrap();
        library
            .add_game(&crate::GameRecord {
                game_id: "game".into(),
                title: "Test".into(),
                user_title: None,
                location: "test-game".into(),
                family_id: Some("test".into()),
                content_fingerprint: None,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let schema = vec![ConfigField {
            id: "gain".into(),
            label: "Gain".into(),
            group: "Audio".into(),
            kind: ConfigKind::Number { min: 0.0, max: 1.0 },
            default: ConfigValue::Number(0.5),
        }];
        let entry = |value| ConfigEntry {
            id: "gain".into(),
            value: ConfigValue::Number(value),
        };
        library
            .save_family_configuration("test.core", "", &schema, &[entry(0.7)])
            .unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "game", &schema)
                .unwrap(),
            vec![entry(0.7)]
        );
        assert!(library
            .save_family_configuration("test.core", "", &schema, &[entry(2.0)])
            .is_err());
        assert_eq!(
            library
                .family_configuration("test.core", "", &schema)
                .unwrap(),
            vec![entry(0.7)]
        );
        library
            .save_family_configuration("test.core", "game", &schema, &[entry(0.2)])
            .unwrap();
        drop(library);
        let mut library = Library::open(&path).unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "game", &schema)
                .unwrap(),
            vec![entry(0.2)]
        );
        library
            .save_family_configuration("test.core", "", &schema, &[])
            .unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "", &schema)
                .unwrap(),
            vec![entry(0.5)]
        );
        library
            .connection
            .execute(
                "UPDATE family_configuration SET values_json=?1 WHERE game_id='game'",
                [serde_json::to_string(&vec![entry(0.1), entry(0.2)]).unwrap()],
            )
            .unwrap();
        assert!(library
            .family_configuration("test.core", "game", &schema)
            .is_err());
        library.remove_game("game").unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "game", &schema)
                .unwrap(),
            vec![entry(0.5)]
        );
    }
}
