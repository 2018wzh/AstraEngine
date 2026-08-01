use astra_core::Hash256;
use rusqlite::{params, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::library::{validate_symbol, Library, LibraryError};

/// One recorded play session for a work. `end_unix_ms` is `None` while the
/// session is still active (the game is running).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlaySessionRecord {
    pub session_id: String,
    pub work_id: String,
    pub case_identity: String,
    pub start_unix_ms: i64,
    pub end_unix_ms: Option<i64>,
    pub duration_ms: i64,
    /// "active" while running, then one of "leave" | "shutdown" | "crash".
    pub ended_by: String,
}

/// Aggregate play statistics for a single work, computed from settled
/// sessions only.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PlayStats {
    pub total_duration_ms: i64,
    pub last_played_unix_ms: Option<i64>,
    pub session_count: u32,
}

/// One row in the "recently played" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecentWorkRecord {
    pub work_id: String,
    pub last_played_unix_ms: i64,
    pub total_duration_ms: i64,
    pub session_count: u32,
}

impl Library {
    /// Record the start of a play session for the case's work and return the
    /// new session id. The case must already exist in `library_case`.
    pub fn start_play_session(
        &mut self,
        case_identity: &str,
        start_unix_ms: i64,
    ) -> Result<String, LibraryError> {
        validate_symbol(case_identity)?;
        let work_id: String = self
            .connection
            .query_row(
                "SELECT work_id FROM library_case WHERE case_identity=?1",
                [case_identity],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| LibraryError::InvalidSymbol("case_not_found".into()))?;
        let existing: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM play_session WHERE work_id=?1",
            [&work_id],
            |row| row.get(0),
        )?;
        let session_id = format!(
            "psess-{}",
            &Hash256::from_sha256(format!("{work_id}\0{start_unix_ms}\0{existing}").as_bytes())
                .to_hex()[..32]
        );
        self.connection.execute(
            "INSERT INTO play_session(session_id, work_id, case_identity, start_unix_ms, ended_by)
             VALUES(?1, ?2, ?3, ?4, 'active')",
            params![session_id, work_id, case_identity, start_unix_ms],
        )?;
        tracing::info!(
            event = "astra.emu.play.session_start",
            work_id = %work_id,
            session_id = %session_id
        );
        Ok(session_id)
    }

    /// Settle an active session. Idempotent: sessions that are already ended
    /// are left untouched. Duration is clamped to be non-negative.
    pub fn end_play_session(
        &mut self,
        session_id: &str,
        end_unix_ms: i64,
        ended_by: &str,
    ) -> Result<(), LibraryError> {
        validate_symbol(session_id)?;
        validate_symbol(ended_by)?;
        let row: Option<(i64, Option<i64>)> = self
            .connection
            .query_row(
                "SELECT start_unix_ms, end_unix_ms FROM play_session WHERE session_id=?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((start_unix_ms, current_end)) = row else {
            return Ok(());
        };
        if current_end.is_some() {
            return Ok(());
        }
        let duration_ms = (end_unix_ms - start_unix_ms).max(0);
        self.connection.execute(
            "UPDATE play_session SET end_unix_ms=?2, duration_ms=?3, ended_by=?4
             WHERE session_id=?1",
            params![session_id, end_unix_ms, duration_ms, ended_by],
        )?;
        tracing::info!(
            event = "astra.emu.play.session_end",
            session_id = %session_id,
            duration_ms = duration_ms,
            ended_by = %ended_by
        );
        Ok(())
    }

    /// Settle any sessions still marked active (for example after a crash).
    /// Active sessions have no reliable end time, so they are closed with zero
    /// duration and `ended_by='crash'`. Returns the number settled.
    pub fn settle_abandoned_sessions(&mut self, now_unix_ms: i64) -> Result<usize, LibraryError> {
        let updated = self.connection.execute(
            "UPDATE play_session SET end_unix_ms=?1, duration_ms=0, ended_by='crash'
             WHERE end_unix_ms IS NULL",
            [now_unix_ms],
        )?;
        if updated > 0 {
            tracing::warn!(event = "astra.emu.play.settle_abandoned", count = updated);
        }
        Ok(updated)
    }

    /// Aggregate play statistics for a work, considering settled sessions only.
    pub fn play_stats(&self, work_id: &str) -> Result<PlayStats, LibraryError> {
        validate_symbol(work_id)?;
        let (total, last, count): (i64, Option<i64>, u32) = self.connection.query_row(
            "SELECT COALESCE(SUM(duration_ms), 0), MAX(start_unix_ms), COUNT(*)
             FROM play_session WHERE work_id=?1 AND end_unix_ms IS NOT NULL",
            [work_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(PlayStats {
            total_duration_ms: total,
            last_played_unix_ms: last,
            session_count: count,
        })
    }

    /// Recently played works, most recent first, limited to `limit` rows.
    pub fn recent_works(&self, limit: u32) -> Result<Vec<RecentWorkRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT work_id, MAX(start_unix_ms) AS last_played,
                    COALESCE(SUM(duration_ms), 0) AS total, COUNT(*) AS sessions
             FROM play_session WHERE end_unix_ms IS NOT NULL
             GROUP BY work_id ORDER BY last_played DESC, work_id LIMIT ?1",
        )?;
        let records = statement
            .query_map([i64::from(limit)], |row| {
                Ok(RecentWorkRecord {
                    work_id: row.get(0)?,
                    last_played_unix_ms: row.get(1)?,
                    total_duration_ms: row.get(2)?,
                    session_count: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// Full session history for a work, most recent first.
    pub fn session_history(&self, work_id: &str) -> Result<Vec<PlaySessionRecord>, LibraryError> {
        validate_symbol(work_id)?;
        let mut statement = self.connection.prepare(
            "SELECT session_id, work_id, case_identity, start_unix_ms, end_unix_ms,
                    duration_ms, ended_by
             FROM play_session WHERE work_id=?1 ORDER BY start_unix_ms DESC, session_id",
        )?;
        let records = statement
            .query_map([work_id], |row| {
                Ok(PlaySessionRecord {
                    session_id: row.get(0)?,
                    work_id: row.get(1)?,
                    case_identity: row.get(2)?,
                    start_unix_ms: row.get(3)?,
                    end_unix_ms: row.get(4)?,
                    duration_ms: row.get(5)?,
                    ended_by: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }
}
