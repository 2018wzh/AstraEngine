//! Local metadata records retained by the Manager.

use rusqlite::OptionalExtension;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::library::{
    validate_metadata, validate_provider, validate_remote_id, Library, LibraryError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayTitle {
    pub value: String,
    pub source: DisplayTitleSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTitleSource {
    User,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalIdentityRecord {
    pub game_id: String,
    pub provider: String,
    pub remote_id: String,
    pub provenance: String,
    pub linked_at_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSnapshotState {
    Fresh,
    Stale,
    Failed,
}

impl MetadataSnapshotState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, LibraryError> {
        match value {
            "fresh" => Ok(Self::Fresh),
            "stale" => Ok(Self::Stale),
            "failed" => Ok(Self::Failed),
            _ => Err(LibraryError::InvalidMetadata),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataSnapshotRecord {
    pub game_id: String,
    pub provider: String,
    pub remote_id: String,
    pub metadata_json: String,
    pub fetched_at_unix_ms: i64,
    pub state: MetadataSnapshotState,
}

impl Library {
    pub fn display_title(&self, game_id: &str) -> Result<Option<DisplayTitle>, LibraryError> {
        let Some(game) = self.game(game_id)? else {
            return Ok(None);
        };
        let (value, source) = match game.user_title {
            Some(title) => (title, DisplayTitleSource::User),
            None => (game.title, DisplayTitleSource::Local),
        };
        Ok(Some(DisplayTitle { value, source }))
    }

    pub fn set_external_identity(
        &mut self,
        record: &ExternalIdentityRecord,
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(&record.game_id)?;
        validate_provider(&record.provider)?;
        validate_remote_id(&record.remote_id)?;
        validate_provider(&record.provenance)?;
        if self.game(&record.game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        self.connection.execute(
            "INSERT INTO external_identity(
                game_id, provider, remote_id, provenance, linked_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(game_id, provider) DO UPDATE SET
                remote_id=excluded.remote_id,
                provenance=excluded.provenance,
                linked_at_unix_ms=excluded.linked_at_unix_ms",
            rusqlite::params![
                record.game_id,
                record.provider,
                record.remote_id,
                record.provenance,
                record.linked_at_unix_ms,
            ],
        )?;
        Ok(())
    }

    pub fn external_identities(
        &self,
        game_id: &str,
    ) -> Result<Vec<ExternalIdentityRecord>, LibraryError> {
        crate::library::validate_id(game_id)?;
        let mut statement = self.connection.prepare(
            "SELECT game_id, provider, remote_id, provenance, linked_at_unix_ms
             FROM external_identity WHERE game_id=?1 ORDER BY provider",
        )?;
        let records = statement
            .query_map([game_id], |row| {
                Ok(ExternalIdentityRecord {
                    game_id: row.get(0)?,
                    provider: row.get(1)?,
                    remote_id: row.get(2)?,
                    provenance: row.get(3)?,
                    linked_at_unix_ms: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn remove_external_identity(
        &mut self,
        game_id: &str,
        provider: &str,
    ) -> Result<bool, LibraryError> {
        crate::library::validate_id(game_id)?;
        validate_provider(provider)?;
        Ok(self.connection.execute(
            "DELETE FROM external_identity WHERE game_id=?1 AND provider=?2",
            rusqlite::params![game_id, provider],
        )? > 0)
    }

    pub fn set_metadata_snapshot(
        &mut self,
        record: &MetadataSnapshotRecord,
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(&record.game_id)?;
        validate_provider(&record.provider)?;
        validate_remote_id(&record.remote_id)?;
        validate_metadata(&record.metadata_json)?;
        if serde_json::from_str::<serde_json::Value>(&record.metadata_json).is_err() {
            return Err(LibraryError::InvalidMetadata);
        }
        if self.game(&record.game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        self.connection.execute(
            "INSERT INTO metadata_snapshot(
                game_id, provider, remote_id, metadata_json, fetched_at_unix_ms, state)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(game_id, provider) DO UPDATE SET
                remote_id=excluded.remote_id,
                metadata_json=excluded.metadata_json,
                fetched_at_unix_ms=excluded.fetched_at_unix_ms,
                state=excluded.state",
            rusqlite::params![
                record.game_id,
                record.provider,
                record.remote_id,
                record.metadata_json,
                record.fetched_at_unix_ms,
                record.state.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn metadata_snapshot(
        &self,
        game_id: &str,
        provider: &str,
    ) -> Result<Option<MetadataSnapshotRecord>, LibraryError> {
        crate::library::validate_id(game_id)?;
        validate_provider(provider)?;
        self.connection
            .query_row(
                "SELECT game_id, provider, remote_id, metadata_json,
                        fetched_at_unix_ms, state
                 FROM metadata_snapshot WHERE game_id=?1 AND provider=?2",
                rusqlite::params![game_id, provider],
                |row| {
                    let state: String = row.get(5)?;
                    Ok(MetadataSnapshotRecord {
                        game_id: row.get(0)?,
                        provider: row.get(1)?,
                        remote_id: row.get(2)?,
                        metadata_json: row.get(3)?,
                        fetched_at_unix_ms: row.get(4)?,
                        state: MetadataSnapshotState::parse(&state).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "metadata state",
                                )),
                            )
                        })?,
                    })
                },
            )
            .optional()
            .map_err(LibraryError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GameRecord, Library};

    #[test]
    fn metadata_and_identity_round_trip() {
        let mut library = Library::in_memory().unwrap();
        library
            .add_game(&GameRecord {
                game_id: "game-a".into(),
                title: "Local title".into(),
                user_title: Some("My title".into()),
                location: "selected-location".into(),
                family_id: Some("fvp".into()),
                content_fingerprint: None,
                added_at_unix_ms: 1,
            })
            .unwrap();
        library
            .set_external_identity(&ExternalIdentityRecord {
                game_id: "game-a".into(),
                provider: "vndb".into(),
                remote_id: "v1".into(),
                provenance: "manual".into(),
                linked_at_unix_ms: 2,
            })
            .unwrap();
        assert_eq!(library.external_identities("game-a").unwrap().len(), 1);
        library
            .set_metadata_snapshot(&MetadataSnapshotRecord {
                game_id: "game-a".into(),
                provider: "vndb".into(),
                remote_id: "v1".into(),
                metadata_json: r#"{"title":"Local title"}"#.into(),
                fetched_at_unix_ms: 3,
                state: MetadataSnapshotState::Fresh,
            })
            .unwrap();
        assert_eq!(
            library
                .metadata_snapshot("game-a", "vndb")
                .unwrap()
                .unwrap()
                .state,
            MetadataSnapshotState::Fresh
        );
    }
}
