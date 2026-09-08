//! Local play history. A play session belongs to the Manager process and does
//! not alter the native save files owned by a Family.

use rusqlite::OptionalExtension;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::library::{validate_id, Library, LibraryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaySessionEndReason {
    Leave,
    Shutdown,
    Crash,
}

impl PlaySessionEndReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Leave => "leave",
            Self::Shutdown => "shutdown",
            Self::Crash => "crash",
        }
    }

    fn parse(value: &str) -> Result<Self, LibraryError> {
        match value {
            "leave" => Ok(Self::Leave),
            "shutdown" => Ok(Self::Shutdown),
            "crash" => Ok(Self::Crash),
            _ => Err(LibraryError::Settings),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlaySessionRecord {
    pub session_id: String,
    pub game_id: String,
    pub start_unix_ms: i64,
    pub end_unix_ms: Option<i64>,
    pub duration_ms: i64,
    pub ended_by: Option<PlaySessionEndReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PlayStats {
    pub total_duration_ms: i64,
    pub last_played_unix_ms: Option<i64>,
    pub session_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecentGameRecord {
    pub game_id: String,
    pub last_played_unix_ms: i64,
    pub total_duration_ms: i64,
    pub session_count: u32,
}

impl Library {
    pub fn start_play_session(
        &mut self,
        game_id: &str,
        start_unix_ms: i64,
    ) -> Result<String, LibraryError> {
        validate_id(game_id)?;
        if self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        let active: Option<String> = self
            .connection
            .query_row(
                "SELECT game_id FROM play_session WHERE end_unix_ms IS NULL LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if active.is_some() {
            return Err(LibraryError::ActiveSession);
        }
        let session_id = format!("play-{}", Uuid::new_v4().simple());
        self.connection.execute(
            "INSERT INTO play_session(
                session_id, game_id, start_unix_ms, ended_by)
             VALUES(?1, ?2, ?3, 'active')",
            rusqlite::params![session_id, game_id, start_unix_ms],
        )?;
        tracing::info!(event = "astra.emu.play.session_started");
        Ok(session_id)
    }

    pub fn active_play_session(&self) -> Result<Option<PlaySessionRecord>, LibraryError> {
        self.connection
            .query_row(
                "SELECT session_id, game_id, start_unix_ms, end_unix_ms,
                        duration_ms, ended_by
                 FROM play_session WHERE end_unix_ms IS NULL LIMIT 1",
                [],
                play_session_from_row,
            )
            .optional()
            .map_err(LibraryError::from)
    }

    pub fn end_play_session(
        &mut self,
        session_id: &str,
        end_unix_ms: i64,
        reason: PlaySessionEndReason,
    ) -> Result<(), LibraryError> {
        validate_id(session_id)?;
        let start: Option<i64> = self
            .connection
            .query_row(
                "SELECT start_unix_ms FROM play_session
                 WHERE session_id=?1 AND end_unix_ms IS NULL",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(start_unix_ms) = start else {
            return Ok(());
        };
        let duration_ms = end_unix_ms.saturating_sub(start_unix_ms).max(0);
        self.connection.execute(
            "UPDATE play_session SET end_unix_ms=?2, duration_ms=?3, ended_by=?4
             WHERE session_id=?1 AND end_unix_ms IS NULL",
            rusqlite::params![session_id, end_unix_ms, duration_ms, reason.as_str()],
        )?;
        tracing::info!(
            event = "astra.emu.play.session_ended",
            duration_ms,
            reason = reason.as_str()
        );
        Ok(())
    }

    pub fn settle_abandoned_sessions(&mut self, end_unix_ms: i64) -> Result<usize, LibraryError> {
        let count = self.connection.execute(
            "UPDATE play_session SET end_unix_ms=?1, duration_ms=0, ended_by='crash'
             WHERE end_unix_ms IS NULL",
            [end_unix_ms],
        )?;
        if count > 0 {
            tracing::warn!(event = "astra.emu.play.sessions_settled", count);
        }
        Ok(count)
    }

    pub fn play_stats(&self, game_id: &str) -> Result<PlayStats, LibraryError> {
        validate_id(game_id)?;
        let (total, last, count): (i64, Option<i64>, u32) = self.connection.query_row(
            "SELECT COALESCE(SUM(duration_ms), 0), MAX(start_unix_ms), COUNT(*)
             FROM play_session WHERE game_id=?1 AND end_unix_ms IS NOT NULL",
            [game_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(PlayStats {
            total_duration_ms: total,
            last_played_unix_ms: last,
            session_count: count,
        })
    }

    pub fn recent_games(&self, limit: u32) -> Result<Vec<RecentGameRecord>, LibraryError> {
        let limit = i64::from(limit);
        let mut statement = self.connection.prepare(
            "SELECT game_id, MAX(start_unix_ms) AS last_played,
                    COALESCE(SUM(duration_ms), 0) AS total, COUNT(*) AS sessions
             FROM play_session WHERE end_unix_ms IS NOT NULL
             GROUP BY game_id ORDER BY last_played DESC, game_id LIMIT ?1",
        )?;
        let records = statement
            .query_map([limit], |row| {
                Ok(RecentGameRecord {
                    game_id: row.get(0)?,
                    last_played_unix_ms: row.get(1)?,
                    total_duration_ms: row.get(2)?,
                    session_count: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn session_history(&self, game_id: &str) -> Result<Vec<PlaySessionRecord>, LibraryError> {
        validate_id(game_id)?;
        let mut statement = self.connection.prepare(
            "SELECT session_id, game_id, start_unix_ms, end_unix_ms,
                    duration_ms, ended_by
             FROM play_session WHERE game_id=?1
             ORDER BY start_unix_ms DESC, session_id",
        )?;
        let records = statement
            .query_map([game_id], play_session_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }
}

fn play_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaySessionRecord> {
    let end_unix_ms: Option<i64> = row.get(3)?;
    let ended_by: String = row.get(5)?;
    let ended_by = if end_unix_ms.is_none() {
        None
    } else {
        Some(PlaySessionEndReason::parse(&ended_by).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "play reason",
                )),
            )
        })?)
    };
    Ok(PlaySessionRecord {
        session_id: row.get(0)?,
        game_id: row.get(1)?,
        start_unix_ms: row.get(2)?,
        end_unix_ms,
        duration_ms: row.get(4)?,
        ended_by,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GameRecord, Library};

    fn seeded_library() -> Library {
        let mut library = Library::in_memory().unwrap();
        library
            .add_game(&GameRecord {
                game_id: "game-a".into(),
                title: "Game A".into(),
                user_title: None,
                location: "location-a".into(),
                family_id: Some("fvp".into()),
                content_fingerprint: None,
                added_at_unix_ms: 1,
            })
            .unwrap();
        library
    }

    #[test]
    fn only_one_manager_session_can_be_active() {
        let mut library = seeded_library();
        let first = library.start_play_session("game-a", 1_000).unwrap();
        assert!(matches!(
            library.start_play_session("game-a", 2_000),
            Err(LibraryError::ActiveSession)
        ));
        library
            .end_play_session(&first, 6_000, PlaySessionEndReason::Leave)
            .unwrap();
        assert_eq!(
            library.play_stats("game-a").unwrap().total_duration_ms,
            5_000
        );
        assert!(library.start_play_session("game-a", 7_000).is_ok());
    }

    #[test]
    fn abandoned_session_is_settled_as_crash() {
        let mut library = seeded_library();
        library.start_play_session("game-a", 1_000).unwrap();
        assert_eq!(library.settle_abandoned_sessions(2_000).unwrap(), 1);
        let history = library.session_history("game-a").unwrap();
        assert_eq!(history[0].ended_by, Some(PlaySessionEndReason::Crash));
        assert_eq!(history[0].duration_ms, 0);
    }
}
