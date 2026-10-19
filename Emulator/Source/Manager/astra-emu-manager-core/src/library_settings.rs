use super::*;

fn migrate_appearance(raw: &str) -> Option<crate::AppearanceSettings> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Previous {
        theme_dark: bool,
        theme_mode: String,
        accent: String,
        grid_columns: i32,
        density: String,
        view_mode: String,
    }
    let previous: Previous = serde_json::from_str(raw).ok()?;
    if !matches!(previous.accent.as_str(), "blue" | "purple")
        || !(1..=8).contains(&previous.grid_columns)
    {
        return None;
    }
    let _ = previous.theme_dark;
    let settings = crate::AppearanceSettings {
        theme_mode: previous.theme_mode,
        density: previous.density,
        view_mode: previous.view_mode,
        ..Default::default()
    };
    settings.validate().ok()?;
    Some(settings)
}

impl Library {
    pub fn appearance_settings(&mut self) -> Result<crate::AppearanceSettings, LibraryError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT settings_json FROM appearance_settings WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(raw) => match serde_json::from_str::<crate::AppearanceSettings>(&raw) {
                Ok(settings) if settings.validate().is_ok() => Ok(settings),
                _ => {
                    if let Some(settings) = migrate_appearance(&raw) {
                        self.save_appearance_settings(&settings)?;
                        return Ok(settings);
                    }
                    tracing::warn!(event = "astra.emu.manager.appearance_reset");
                    let defaults = crate::AppearanceSettings::default();
                    self.save_appearance_settings(&defaults)?;
                    Ok(defaults)
                }
            },
            None => Ok(crate::AppearanceSettings::default()),
        }
    }

    pub fn save_appearance_settings(
        &mut self,
        settings: &crate::AppearanceSettings,
    ) -> Result<(), LibraryError> {
        settings.validate().map_err(|_| LibraryError::Settings)?;
        let json = serde_json::to_string(settings).map_err(|_| LibraryError::Serialization)?;
        self.connection.execute("INSERT INTO appearance_settings(singleton, settings_json) VALUES(1, ?1) ON CONFLICT(singleton) DO UPDATE SET settings_json=excluded.settings_json", [json])?;
        Ok(())
    }
    pub fn save_input_mapping(&mut self, mapping: &InputMapping) -> Result<(), LibraryError> {
        mapping.validate().map_err(|_| LibraryError::Settings)?;
        let mapping_json =
            serde_json::to_string(mapping).map_err(|_| LibraryError::Serialization)?;
        let filter = serde_json::to_string(&self.filter_settings()?.unwrap_or_default())
            .map_err(|_| LibraryError::Serialization)?;
        self.connection.execute(
            "INSERT INTO manager_settings(singleton, input_mapping_json, filter_settings_json)
             VALUES(1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
                input_mapping_json=excluded.input_mapping_json",
            params![mapping_json, filter],
        )?;
        Ok(())
    }

    pub fn load_input_mapping(&self) -> Result<Option<InputMapping>, LibraryError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT input_mapping_json FROM manager_settings WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| {
            let mapping: InputMapping =
                serde_json::from_str(&value).map_err(|_| LibraryError::Settings)?;
            mapping.validate().map_err(|_| LibraryError::Settings)?;
            Ok(mapping)
        })
        .transpose()
    }

    pub fn save_filter_settings(
        &mut self,
        settings: &crate::FilterSettings,
    ) -> Result<(), LibraryError> {
        settings.validate().map_err(|_| LibraryError::Settings)?;
        let encoded = serde_json::to_string(settings).map_err(|_| LibraryError::Serialization)?;
        let mapping = self
            .load_input_mapping()?
            .unwrap_or_else(crate::default_vn_preset);
        let mapping_json =
            serde_json::to_string(&mapping).map_err(|_| LibraryError::Serialization)?;
        self.connection.execute(
            "INSERT INTO manager_settings(singleton, input_mapping_json, filter_settings_json)
             VALUES(1, ?1, ?2) ON CONFLICT(singleton) DO UPDATE SET filter_settings_json=excluded.filter_settings_json",
            params![mapping_json, encoded],
        )?;
        Ok(())
    }

    pub fn filter_settings(&self) -> Result<Option<crate::FilterSettings>, LibraryError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT filter_settings_json FROM manager_settings WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|raw| {
            let settings: crate::FilterSettings =
                serde_json::from_str(&raw).map_err(|_| LibraryError::Settings)?;
            settings.validate().map_err(|_| LibraryError::Settings)?;
            Ok(settings)
        })
        .transpose()
    }

    pub fn game_settings(&self, game_id: &str) -> Result<Option<GameSettings>, LibraryError> {
        validate_id(game_id)?;
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT settings_json FROM game_settings WHERE game_id=?1",
                [game_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| {
            let settings: GameSettings =
                serde_json::from_str(&value).map_err(|_| LibraryError::Settings)?;
            settings.validate().map_err(|_| LibraryError::Settings)?;
            Ok(settings)
        })
        .transpose()
    }

    pub fn set_game_settings(
        &mut self,
        game_id: &str,
        settings: &GameSettings,
    ) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        if self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        settings.validate().map_err(|_| LibraryError::Settings)?;
        let encoded = serde_json::to_string(settings).map_err(|_| LibraryError::Serialization)?;
        self.connection.execute(
            "INSERT INTO game_settings(game_id, settings_json) VALUES(?1, ?2)
             ON CONFLICT(game_id) DO UPDATE SET settings_json=excluded.settings_json",
            params![game_id, encoded],
        )?;
        Ok(())
    }

    pub fn clear_game_settings(&mut self, game_id: &str) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        self.connection
            .execute("DELETE FROM game_settings WHERE game_id=?1", [game_id])?;
        Ok(())
    }

    pub fn set_translation_profile(
        &mut self,
        profile: &TranslationProfile,
    ) -> Result<(), LibraryError> {
        profile.validate().map_err(|_| LibraryError::Settings)?;
        let encoded = serde_json::to_string(profile).map_err(|_| LibraryError::Serialization)?;
        self.connection.execute(
            "INSERT INTO translation_profile(singleton, profile_json) VALUES(1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET profile_json=excluded.profile_json",
            [encoded],
        )?;
        Ok(())
    }

    pub fn translation_profile(&self) -> Result<Option<TranslationProfile>, LibraryError> {
        let encoded: Option<String> = self
            .connection
            .query_row(
                "SELECT profile_json FROM translation_profile WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        encoded
            .map(|value| serde_json::from_str(&value).map_err(|_| LibraryError::Settings))
            .transpose()
    }

    pub fn clear_translation_profile(&mut self) -> Result<(), LibraryError> {
        self.connection
            .execute("DELETE FROM translation_profile WHERE singleton=1", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_reopen_and_reject_invalid_updates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.db");
        let appearance = crate::AppearanceSettings {
            cover_size: "small".into(),
            reduce_motion: true,
            ..Default::default()
        };
        let filter = crate::FilterSettings {
            configuration: crate::FilterConfiguration {
                preset: crate::FilterPreset::Scale,
                scale: 2.0,
                ..Default::default()
            },
            source: None,
        };
        {
            let mut library = Library::open(&path).unwrap();
            library.save_appearance_settings(&appearance).unwrap();
            library.save_filter_settings(&filter).unwrap();
            library
                .save_input_mapping(&crate::default_vn_preset())
                .unwrap();
            let mut invalid = filter.clone();
            invalid.configuration.scale = f32::NAN;
            assert!(library.save_filter_settings(&invalid).is_err());
            let invalid = crate::AppearanceSettings {
                cover_size: "huge".into(),
                ..appearance.clone()
            };
            assert!(library.save_appearance_settings(&invalid).is_err());
            let mut mapping = crate::default_vn_preset();
            mapping
                .gamepad
                .insert(crate::GamepadInput::South, "unknown_key".into());
            assert!(library.save_input_mapping(&mapping).is_err());
        }
        let mut library = Library::open(path).unwrap();
        assert_eq!(library.appearance_settings().unwrap(), appearance);
        assert_eq!(library.filter_settings().unwrap(), Some(filter));
        assert_eq!(
            library.load_input_mapping().unwrap(),
            Some(crate::default_vn_preset())
        );
    }

    #[test]
    fn failed_settings_write_preserves_previous_filter() {
        let mut library = Library::in_memory().unwrap();
        let previous = crate::FilterSettings::default();
        library.save_filter_settings(&previous).unwrap();
        library
            .connection
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        let mut next = previous.clone();
        next.configuration.preset = crate::FilterPreset::Sharpen;
        assert!(library.save_filter_settings(&next).is_err());
        assert_eq!(library.filter_settings().unwrap(), Some(previous));
    }

    #[test]
    fn previous_appearance_preserves_shared_preferences_after_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.db");
        {
            let mut library = Library::open(&path).unwrap();
            library.connection.execute(
                "INSERT INTO appearance_settings(singleton, settings_json) VALUES(1, ?1)",
                [r#"{"theme_dark":true,"theme_mode":"dark","accent":"purple","grid_columns":4,"density":"compact","view_mode":"list"}"#],
            ).unwrap();
            let migrated = library.appearance_settings().unwrap();
            assert_eq!(migrated.theme_mode, "dark");
            assert_eq!(migrated.density, "compact");
            assert_eq!(migrated.view_mode, "list");
        }
        let mut reopened = Library::open(path).unwrap();
        let settings = reopened.appearance_settings().unwrap();
        assert_eq!(settings.theme_mode, "dark");
        assert_eq!(settings.density, "compact");
        assert_eq!(settings.view_mode, "list");
    }

    #[test]
    fn invalid_appearance_json_resets_to_defaults() {
        let mut library = Library::in_memory().unwrap();
        library
            .connection
            .execute(
                "INSERT INTO appearance_settings(singleton, settings_json) VALUES(1, ?1)",
                [r#"{"theme_dark":true}"#],
            )
            .unwrap();
        let settings = library.appearance_settings().unwrap();
        assert_eq!(settings, crate::AppearanceSettings::default());
        let stored: String = library
            .connection
            .query_row(
                "SELECT settings_json FROM appearance_settings WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stored,
            serde_json::to_string(&crate::AppearanceSettings::default()).unwrap()
        );
    }
}
