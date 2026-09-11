use super::*;

impl Library {
    pub fn appearance_settings(&self) -> Result<crate::AppearanceSettings, LibraryError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT settings_json FROM appearance_settings WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let settings: crate::AppearanceSettings = match raw {
            Some(raw) => serde_json::from_str(&raw).map_err(|_| LibraryError::Settings)?,
            None => crate::AppearanceSettings::default(),
        };
        settings.validate().map_err(|_| LibraryError::Settings)?;
        Ok(settings)
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

    pub fn install_verified_plugin(
        &mut self,
        verified: &VerifiedPluginInstall,
    ) -> Result<(), LibraryError> {
        let record = verified.record();
        let descriptor = record.descriptor();
        descriptor
            .validate()
            .map_err(|_| LibraryError::PluginDescriptor)?;
        validate_location(&record.location)?;
        let capabilities =
            serde_json::to_string(&record.capabilities).map_err(|_| LibraryError::Serialization)?;
        let supported_formats = serde_json::to_string(&record.supported_formats)
            .map_err(|_| LibraryError::Serialization)?;
        self.connection.execute(
            "INSERT INTO plugin_installation(
                plugin_id, family_id, location, abi_fingerprint, version,
                capabilities_json, supported_formats_json, installed_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(plugin_id) DO UPDATE SET
                family_id=excluded.family_id,
                location=excluded.location,
                abi_fingerprint=excluded.abi_fingerprint,
                version=excluded.version,
                capabilities_json=excluded.capabilities_json,
                supported_formats_json=excluded.supported_formats_json,
                installed_at_unix_ms=excluded.installed_at_unix_ms",
            params![
                record.plugin_id,
                record.family_id,
                record.location,
                record.abi_fingerprint,
                record.version,
                capabilities,
                supported_formats,
                record.installed_at_unix_ms,
            ],
        )?;
        Ok(())
    }

    pub fn list_installed_plugins(&self) -> Result<Vec<PluginInstallRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT plugin_id, family_id, location, abi_fingerprint, version,
                    capabilities_json, supported_formats_json, installed_at_unix_ms
             FROM plugin_installation ORDER BY plugin_id",
        )?;
        let rows = statement.query_map([], |row| {
            let capabilities_json: String = row.get(5)?;
            let capabilities = serde_json::from_str(&capabilities_json).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "capabilities",
                    )),
                )
            })?;
            let formats_json: String = row.get(6)?;
            let supported_formats = serde_json::from_str(&formats_json).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "supported formats",
                    )),
                )
            })?;
            Ok(PluginInstallRecord {
                plugin_id: row.get(0)?,
                family_id: row.get(1)?,
                location: row.get(2)?,
                abi_fingerprint: row.get(3)?,
                version: row.get(4)?,
                capabilities,
                supported_formats,
                installed_at_unix_ms: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LibraryError::from)
    }

    pub fn remove_plugin(&mut self, plugin_id: &str) -> Result<bool, LibraryError> {
        validate_id(plugin_id)?;
        Ok(self.connection.execute(
            "DELETE FROM plugin_installation WHERE plugin_id=?1",
            [plugin_id],
        )? > 0)
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
            grid_columns: 6,
            accent: "purple".into(),
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
                grid_columns: 0,
                ..appearance.clone()
            };
            assert!(library.save_appearance_settings(&invalid).is_err());
            let mut mapping = crate::default_vn_preset();
            mapping
                .gamepad
                .insert(crate::GamepadInput::South, "unknown_key".into());
            assert!(library.save_input_mapping(&mapping).is_err());
        }
        let library = Library::open(path).unwrap();
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
}
