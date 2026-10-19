//! A consistent, bulk-read projection of the library for Manager consumers.

use std::collections::BTreeMap;

use super::{game_from_row, GameRecord, Library, LibraryError};
use crate::{
    metadata::{external_identity_from_row, metadata_snapshot_from_row},
    play::play_stats_from_row,
    ExternalIdentityRecord, MetadataSnapshotRecord, PlayStats,
};

/// Library records read from one database snapshot, without provider JSON parsing.
#[derive(Debug)]
pub struct LibrarySnapshot {
    /// Games not removed from the library, in [`Library::list_games`] order.
    /// Removed games are read through [`Library::excluded_games`].
    pub games: Vec<GameRecord>,
    /// Completed-session totals for every game, including games with no sessions.
    pub play_stats: BTreeMap<String, PlayStats>,
    /// Linked identities ordered by provider within each game.
    pub identities: BTreeMap<String, Vec<ExternalIdentityRecord>>,
    /// Raw metadata keyed by `(game_id, provider)`, retaining its stored state.
    pub metadata: BTreeMap<(String, String), MetadataSnapshotRecord>,
}

impl Library {
    /// Read games, completed-session totals, identities, and metadata consistently.
    ///
    /// Four bulk queries share a deferred read transaction. Sessions are aggregated
    /// independently of identity and metadata rows, so providers cannot multiply
    /// play durations or counts. Active sessions are excluded, as in `play_stats`.
    pub fn library_snapshot(&self) -> Result<LibrarySnapshot, LibraryError> {
        let transaction = self.connection.unchecked_transaction()?;
        let games = {
            let mut statement = transaction.prepare(
                "SELECT game_id, title, user_title, location, family_id,
                        content_fingerprint, added_at_unix_ms
                 FROM library_game
                 WHERE game_id NOT IN (SELECT game_id FROM excluded_game)
                 ORDER BY title COLLATE NOCASE, game_id",
            )?;
            let games = statement
                .query_map([], game_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            games
        };
        let play_stats = {
            let mut statement = transaction.prepare(
                "SELECT COALESCE(SUM(session.duration_ms), 0),
                        MAX(session.start_unix_ms), COUNT(session.session_id), game.game_id
                 FROM library_game AS game
                 LEFT JOIN play_session AS session
                    ON session.game_id=game.game_id AND session.end_unix_ms IS NOT NULL
                 GROUP BY game.game_id",
            )?;
            let play_stats = statement
                .query_map([], |row| Ok((row.get(3)?, play_stats_from_row(row)?)))?
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            play_stats
        };
        let identities = {
            let mut statement = transaction.prepare(
                "SELECT game_id, provider, remote_id, provenance, linked_at_unix_ms
                 FROM external_identity ORDER BY game_id, provider",
            )?;
            let mut identities: BTreeMap<String, Vec<ExternalIdentityRecord>> = BTreeMap::new();
            for record in statement.query_map([], external_identity_from_row)? {
                let record = record?;
                if let Some(records) = identities.get_mut(record.game_id.as_str()) {
                    records.push(record);
                } else {
                    identities.insert(record.game_id.clone(), vec![record]);
                }
            }
            identities
        };
        let metadata = {
            let mut statement = transaction.prepare(
                "SELECT game_id, provider, remote_id, metadata_json, fetched_at_unix_ms, state
                 FROM metadata_snapshot",
            )?;
            let mut metadata = BTreeMap::new();
            for record in statement.query_map([], metadata_snapshot_from_row)? {
                let record = record?;
                metadata.insert((record.game_id.clone(), record.provider.clone()), record);
            }
            metadata
        };
        transaction.commit()?;
        Ok(LibrarySnapshot {
            games,
            play_stats,
            identities,
            metadata,
        })
    }
}

#[cfg(test)]
#[path = "library_snapshot_tests.rs"]
mod tests;
