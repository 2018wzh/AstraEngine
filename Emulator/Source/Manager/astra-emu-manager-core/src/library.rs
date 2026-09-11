//! Durable Manager data for games, play records, metadata, and settings.
//!
//! This database is deliberately independent from a running Family. Opening
//! a database with an old or unknown schema explicitly rebuilds the Manager
//! tables; native game save files live below the game location and are never
//! touched by this code.

use std::{path::Path, time::Duration};

use astra_emu_translation_openai_compatible::TranslationProfile;
use rusqlite::{params, Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    family::{FamilyCapability, FamilyPluginDescriptor},
    input_mapping::InputMapping,
    work_settings::GameSettings,
};

#[path = "library_settings.rs"]
mod settings;

const SCHEMA_VERSION: i64 = 3;
const MAX_TITLE_CHARS: usize = 1_024;
const MAX_LOCATION_BYTES: usize = 4_096;
const MAX_METADATA_BYTES: usize = 1_048_576;

const REQUIRED_TABLES: [&str; 9] = [
    "appearance_settings",
    "library_game",
    "external_identity",
    "metadata_snapshot",
    "play_session",
    "manager_settings",
    "game_settings",
    "translation_profile",
    "plugin_installation",
];

const SCHEMA_SQL: &str = r#"
CREATE TABLE appearance_settings (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    settings_json TEXT NOT NULL
);
CREATE TABLE library_game (
    game_id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    user_title TEXT,
    location TEXT NOT NULL UNIQUE,
    family_id TEXT,
    content_fingerprint TEXT,
    added_at_unix_ms INTEGER NOT NULL
);
CREATE INDEX library_game_title ON library_game(title, game_id);
CREATE TABLE external_identity (
    game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    provenance TEXT NOT NULL,
    linked_at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY(game_id, provider),
    UNIQUE(provider, remote_id)
);
CREATE TABLE metadata_snapshot (
    game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    fetched_at_unix_ms INTEGER NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY(game_id, provider)
);
CREATE TABLE play_session (
    session_id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    start_unix_ms INTEGER NOT NULL,
    end_unix_ms INTEGER,
    duration_ms INTEGER NOT NULL DEFAULT 0 CHECK(duration_ms >= 0),
    ended_by TEXT NOT NULL DEFAULT 'active'
);
CREATE INDEX play_session_game_start ON play_session(game_id, start_unix_ms);
CREATE UNIQUE INDEX one_active_play_session ON play_session((1)
) WHERE end_unix_ms IS NULL;
CREATE TABLE manager_settings (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    input_mapping_json TEXT NOT NULL,
    filter_settings_json TEXT NOT NULL
);
CREATE TABLE game_settings (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    settings_json TEXT NOT NULL
);
CREATE TABLE translation_profile (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    profile_json TEXT NOT NULL
);
CREATE TABLE plugin_installation (
    plugin_id TEXT PRIMARY KEY NOT NULL,
    family_id TEXT NOT NULL,
    location TEXT NOT NULL UNIQUE,
    abi_fingerprint TEXT NOT NULL,
    version TEXT NOT NULL,
    capabilities_json TEXT NOT NULL,
    supported_formats_json TEXT NOT NULL,
    installed_at_unix_ms INTEGER NOT NULL
);
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameRecord {
    pub game_id: String,
    pub title: String,
    pub user_title: Option<String>,
    pub location: String,
    pub family_id: Option<String>,
    pub content_fingerprint: Option<String>,
    pub added_at_unix_ms: i64,
}

impl GameRecord {
    pub fn display_title(&self) -> &str {
        self.user_title.as_deref().unwrap_or(&self.title)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginInstallRecord {
    pub plugin_id: String,
    pub family_id: String,
    pub location: String,
    pub abi_fingerprint: String,
    pub version: String,
    pub capabilities: Vec<FamilyCapability>,
    pub supported_formats: Vec<String>,
    pub installed_at_unix_ms: i64,
}

impl PluginInstallRecord {
    pub fn descriptor(&self) -> FamilyPluginDescriptor {
        FamilyPluginDescriptor {
            family_id: self.family_id.clone(),
            plugin_id: self.plugin_id.clone(),
            abi_fingerprint: self.abi_fingerprint.clone(),
            version: self.version.clone(),
            capabilities: self.capabilities.clone(),
            supported_formats: self.supported_formats.clone(),
        }
    }
}

/// A database write token that can only be created by code which has already
/// checked a real Family module's ABI layout and descriptor. The public record
/// remains serializable for display/reporting, but cannot be passed to the
/// persistence API as self-reported plugin metadata.
#[derive(Debug, Clone)]
pub struct VerifiedPluginInstall {
    record: PluginInstallRecord,
}

impl VerifiedPluginInstall {
    #[allow(dead_code)]
    pub fn from_verified_descriptor(
        descriptor: FamilyPluginDescriptor,
        location: String,
        installed_at_unix_ms: i64,
    ) -> Result<Self, LibraryError> {
        descriptor
            .validate()
            .map_err(|_| LibraryError::PluginDescriptor)?;
        validate_location(&location)?;
        Ok(Self {
            record: PluginInstallRecord {
                plugin_id: descriptor.plugin_id.clone(),
                family_id: descriptor.family_id.clone(),
                location,
                abi_fingerprint: descriptor.abi_fingerprint.clone(),
                version: descriptor.version.clone(),
                capabilities: descriptor.capabilities.clone(),
                supported_formats: descriptor.supported_formats.clone(),
                installed_at_unix_ms,
            },
        })
    }

    pub fn record(&self) -> &PluginInstallRecord {
        &self.record
    }
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("ASTRA_EMU_LIBRARY_SQLITE: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("ASTRA_EMU_LIBRARY_SCHEMA_CORRUPT")]
    SchemaCorrupt,
    #[error("ASTRA_EMU_LIBRARY_INVALID_ID")]
    InvalidId,
    #[error("ASTRA_EMU_LIBRARY_INVALID_TITLE")]
    InvalidTitle,
    #[error("ASTRA_EMU_LIBRARY_INVALID_LOCATION")]
    InvalidLocation,
    #[error("ASTRA_EMU_LIBRARY_INVALID_METADATA")]
    InvalidMetadata,
    #[error("ASTRA_EMU_LIBRARY_GAME_NOT_FOUND")]
    GameNotFound,
    #[error("ASTRA_EMU_LIBRARY_ACTIVE_SESSION")]
    ActiveSession,
    #[error("ASTRA_EMU_LIBRARY_SERIALIZATION")]
    Serialization,
    #[error("ASTRA_EMU_LIBRARY_SETTINGS")]
    Settings,
    #[error("ASTRA_EMU_LIBRARY_PLUGIN_DESCRIPTOR")]
    PluginDescriptor,
}

pub struct Library {
    pub(crate) connection: Connection,
}

impl Library {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LibraryError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self, LibraryError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, LibraryError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let mut library = Self { connection };
        library.ensure_schema()?;
        Ok(library)
    }

    fn ensure_schema(&mut self) -> Result<(), LibraryError> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        let table_count = self.user_table_count()?;
        if version == SCHEMA_VERSION {
            if table_count != REQUIRED_TABLES.len() || !self.required_tables_present()? {
                return Err(LibraryError::SchemaCorrupt);
            }
            return Ok(());
        }

        if version != 0 || table_count != 0 {
            self.rebuild_manager_tables()?;
            tracing::warn!(
                event = "astra.emu.library.rebuilt",
                previous_schema_version = version,
            );
        }
        self.connection.execute_batch(SCHEMA_SQL)?;
        self.connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    fn user_table_count(&self) -> Result<usize, LibraryError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        usize::try_from(count).map_err(|_| LibraryError::SchemaCorrupt)
    }

    fn required_tables_present(&self) -> Result<bool, LibraryError> {
        for table in REQUIRED_TABLES {
            let present: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )?;
            if !present {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn rebuild_manager_tables(&mut self) -> Result<(), LibraryError> {
        self.connection.execute_batch("PRAGMA foreign_keys=OFF;")?;
        let tables = {
            let mut statement = self.connection.prepare(
                "SELECT name FROM sqlite_master
                 WHERE type='table'
                   AND name NOT LIKE 'sqlite_%'",
            )?;
            let records = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        for table in tables {
            let escaped = table.replace('"', "\"\"");
            self.connection
                .execute_batch(&format!("DROP TABLE IF EXISTS \"{escaped}\";"))?;
        }
        self.connection.execute_batch(
            "DROP INDEX IF EXISTS library_game_title;
             DROP INDEX IF EXISTS play_session_game_start;
             DROP INDEX IF EXISTS one_active_play_session;
             PRAGMA user_version=0;
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(())
    }

    pub fn add_game(&mut self, game: &GameRecord) -> Result<(), LibraryError> {
        validate_game(game)?;
        self.connection.execute(
            "INSERT INTO library_game(
                game_id, title, user_title, location, family_id,
                content_fingerprint, added_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(game_id) DO UPDATE SET
                title=excluded.title,
                user_title=excluded.user_title,
                location=excluded.location,
                family_id=excluded.family_id,
                content_fingerprint=excluded.content_fingerprint",
            params![
                game.game_id,
                game.title,
                game.user_title,
                game.location,
                game.family_id,
                game.content_fingerprint,
                game.added_at_unix_ms,
            ],
        )?;
        Ok(())
    }

    pub fn list_games(&self) -> Result<Vec<GameRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT game_id, title, user_title, location, family_id,
                    content_fingerprint, added_at_unix_ms
             FROM library_game ORDER BY title COLLATE NOCASE, game_id",
        )?;
        let records = statement
            .query_map([], game_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn game(&self, game_id: &str) -> Result<Option<GameRecord>, LibraryError> {
        validate_id(game_id)?;
        self.connection
            .query_row(
                "SELECT game_id, title, user_title, location, family_id,
                        content_fingerprint, added_at_unix_ms
                 FROM library_game WHERE game_id=?1",
                [game_id],
                game_from_row,
            )
            .optional()
            .map_err(LibraryError::from)
    }

    pub fn remove_game(&mut self, game_id: &str) -> Result<bool, LibraryError> {
        validate_id(game_id)?;
        Ok(self
            .connection
            .execute("DELETE FROM library_game WHERE game_id=?1", [game_id])?
            > 0)
    }

    pub fn set_game_user_title(
        &mut self,
        game_id: &str,
        user_title: Option<&str>,
    ) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        if let Some(title) = user_title {
            validate_title(title)?;
        }
        let changed = self.connection.execute(
            "UPDATE library_game SET user_title=?2 WHERE game_id=?1",
            params![game_id, user_title],
        )?;
        if changed == 0 {
            return Err(LibraryError::GameNotFound);
        }
        Ok(())
    }

    pub fn set_game_family(
        &mut self,
        game_id: &str,
        family_id: Option<&str>,
    ) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        if let Some(family_id) = family_id {
            validate_id(family_id)?;
        }
        let changed = self.connection.execute(
            "UPDATE library_game SET family_id=?2 WHERE game_id=?1",
            params![game_id, family_id],
        )?;
        if changed == 0 {
            return Err(LibraryError::GameNotFound);
        }
        Ok(())
    }
}

fn game_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameRecord> {
    Ok(GameRecord {
        game_id: row.get(0)?,
        title: row.get(1)?,
        user_title: row.get(2)?,
        location: row.get(3)?,
        family_id: row.get(4)?,
        content_fingerprint: row.get(5)?,
        added_at_unix_ms: row.get(6)?,
    })
}

fn validate_game(game: &GameRecord) -> Result<(), LibraryError> {
    validate_id(&game.game_id)?;
    validate_title(&game.title)?;
    if let Some(user_title) = &game.user_title {
        validate_title(user_title)?;
    }
    validate_location(&game.location)?;
    if let Some(family_id) = &game.family_id {
        validate_id(family_id)?;
    }
    if game
        .content_fingerprint
        .as_deref()
        .is_some_and(|value| value.is_empty() || value.len() > 256 || value.contains('\0'))
    {
        return Err(LibraryError::InvalidId);
    }
    Ok(())
}

pub(crate) fn validate_id(value: &str) -> Result<(), LibraryError> {
    if !is_safe_identifier(value) {
        return Err(LibraryError::InvalidId);
    }
    Ok(())
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

fn validate_title(value: &str) -> Result<(), LibraryError> {
    if value.trim().is_empty() || value.chars().count() > MAX_TITLE_CHARS || value.contains('\0') {
        return Err(LibraryError::InvalidTitle);
    }
    Ok(())
}

fn validate_location(value: &str) -> Result<(), LibraryError> {
    if value.is_empty() || value.len() > MAX_LOCATION_BYTES || value.contains('\0') {
        return Err(LibraryError::InvalidLocation);
    }
    Ok(())
}

pub(crate) fn validate_metadata(value: &str) -> Result<(), LibraryError> {
    if value.is_empty() || value.len() > MAX_METADATA_BYTES || value.contains('\0') {
        return Err(LibraryError::InvalidMetadata);
    }
    Ok(())
}

pub(crate) fn validate_provider(value: &str) -> Result<(), LibraryError> {
    validate_id(value)
}

pub(crate) fn validate_remote_id(value: &str) -> Result<(), LibraryError> {
    if value.is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(LibraryError::InvalidId);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family::{
        FamilyCapability, FamilyPluginDescriptor, INDEPENDENT_FAMILY_ABI_FINGERPRINT,
    };
    use crate::input_mapping::default_vn_preset;

    fn game(id: &str, location: &str) -> GameRecord {
        GameRecord {
            game_id: id.into(),
            title: id.into(),
            user_title: None,
            location: location.into(),
            family_id: Some("fvp".into()),
            content_fingerprint: None,
            added_at_unix_ms: 1,
        }
    }

    #[test]
    fn new_schema_stores_games_and_manager_settings() {
        let mut library = Library::in_memory().unwrap();
        assert!(library.list_games().unwrap().is_empty());
        library
            .add_game(&game("game-a", "user-selected-location"))
            .unwrap();
        assert_eq!(
            library.game("game-a").unwrap().unwrap().display_title(),
            "game-a"
        );
        library.save_input_mapping(&default_vn_preset()).unwrap();
        assert!(library.load_input_mapping().unwrap().is_some());
        library
            .save_filter_settings(&crate::FilterSettings::default())
            .unwrap();
        assert_eq!(
            library.filter_settings().unwrap(),
            Some(crate::FilterSettings::default())
        );
    }

    #[test]
    fn old_schema_is_rebuilt_without_migration() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE source_grant(source_id TEXT); PRAGMA user_version=11;")
            .unwrap();
        let library = Library::from_connection(connection).unwrap();
        assert_eq!(library.user_table_count().unwrap(), REQUIRED_TABLES.len());
        assert!(library.list_games().unwrap().is_empty());
    }

    #[test]
    fn duplicate_locations_are_rejected_and_native_location_is_untouched() {
        let mut library = Library::in_memory().unwrap();
        library.add_game(&game("game-a", "same-location")).unwrap();
        assert!(library.add_game(&game("game-b", "same-location")).is_err());
        assert!(library.remove_game("game-a").unwrap());
        assert!(library.game("game-a").unwrap().is_none());
    }

    #[test]
    fn only_verified_descriptor_can_be_persisted() {
        let mut library = Library::in_memory().unwrap();
        let descriptor = FamilyPluginDescriptor {
            family_id: "fvp".into(),
            plugin_id: "astra.emu.fvp".into(),
            abi_fingerprint: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            version: "1.0.0".into(),
            capabilities: vec![FamilyCapability::CpuFrame],
            supported_formats: vec!["fvp.hcb".into()],
        };
        let verified = VerifiedPluginInstall::from_verified_descriptor(
            descriptor,
            "selected-plugin.dll".into(),
            10,
        )
        .unwrap();
        library.install_verified_plugin(&verified).unwrap();
        let records = library.list_installed_plugins().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].abi_fingerprint,
            INDEPENDENT_FAMILY_ABI_FINGERPRINT
        );
        assert_eq!(records[0].supported_formats, vec!["fvp.hcb"]);
    }
}
