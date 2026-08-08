//! Local read-only cache of the central compatibility database and matching
//! against library works via the VNDB VN id (vID) and release id (rID).
//!
//! VNDB is the single authoritative source for game and version identity:
//! an entry (`CompatibilityCacheEntry`) is keyed by `vn_id` (vID, the game)
//! and `release_id` (rID, the specific version), so compatibility is precise
//! to a particular version of a game. Bangumi is used only for progress
//! tracking and never appears here.
//!
//! Persistence lives here (library DB). Fetching and schema validation live in
//! `astra-emu-metadata`; orchestration lives in the manager.

use rusqlite::{params, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::library::{validate_symbol, Library, LibraryError};

const MAX_NOTES_CHARS: usize = 1024;

/// One cached version-precise compatibility entry, keyed by VNDB VN id (vID)
/// and release id (rID).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilityCacheEntry {
    /// VNDB VN id (vID) — the game-name identity.
    pub vn_id: String,
    /// VNDB release id (rID) — the version identity.
    pub release_id: String,
    pub status: String,
    pub notes: Option<String>,
    pub entry_updated_unix_ms: i64,
    pub fetched_at_unix_ms: i64,
}

/// Singleton sync state for incremental fetches.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilitySyncState {
    pub source_url: String,
    pub response_hash: String,
    pub last_fetched_unix_ms: i64,
    pub diagnostic_code: Option<String>,
}

/// A VNDB release (rID) fetched for a work, used to pin a local installation
/// to a concrete game version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnReleaseRecord {
    pub work_id: String,
    pub release_id: String,
    pub title: Option<String>,
    pub released: Option<String>,
    pub platforms_json: String,
    pub fetched_at_unix_ms: i64,
}

/// A compatibility entry matched to a work through its VNDB identity, precise
/// to the release (rID) the installation was pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilityMatch {
    pub work_id: String,
    pub vn_id: String,
    pub release_id: String,
    pub status: String,
    pub notes: Option<String>,
    pub entry_updated_unix_ms: i64,
}

impl Library {
    /// Atomically replace the whole compatibility cache and record the sync
    /// state. Entries with invalid vn_id/release_id symbols are rejected.
    pub fn replace_compatibility_cache(
        &mut self,
        entries: &[CompatibilityCacheEntry],
        sync_state: &CompatibilitySyncState,
    ) -> Result<(), LibraryError> {
        for entry in entries {
            validate_symbol(&entry.vn_id)?;
            validate_symbol(&entry.release_id)?;
            if entry
                .notes
                .as_ref()
                .is_some_and(|notes| notes.chars().count() > MAX_NOTES_CHARS)
            {
                return Err(LibraryError::InvalidSymbol("notes_too_long".into()));
            }
        }
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM compatibility_entry_cache", [])?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO compatibility_entry_cache(
                     vn_id, release_id, status, notes, entry_updated_unix_ms, fetched_at_unix_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for entry in entries {
                insert.execute(params![
                    entry.vn_id,
                    entry.release_id,
                    entry.status,
                    entry.notes,
                    entry.entry_updated_unix_ms,
                    entry.fetched_at_unix_ms,
                ])?;
            }
        }
        tx.execute(
            "INSERT INTO compatibility_sync_state(
                 singleton, source_url, response_hash, last_fetched_unix_ms, diagnostic_code)
             VALUES(1, ?1, ?2, ?3, ?4)
             ON CONFLICT(singleton) DO UPDATE SET
                 source_url=excluded.source_url,
                 response_hash=excluded.response_hash,
                 last_fetched_unix_ms=excluded.last_fetched_unix_ms,
                 diagnostic_code=excluded.diagnostic_code",
            params![
                sync_state.source_url,
                sync_state.response_hash,
                sync_state.last_fetched_unix_ms,
                sync_state.diagnostic_code,
            ],
        )?;
        tx.commit()?;
        tracing::info!(
            event = "astra.emu.compatibility.cache",
            entries = entries.len(),
            response_hash = %sync_state.response_hash
        );
        Ok(())
    }

    /// Read the cached sync state, if any fetch has succeeded before.
    pub fn compatibility_sync_state(&self) -> Result<Option<CompatibilitySyncState>, LibraryError> {
        let row: Option<CompatibilitySyncState> = self
            .connection
            .query_row(
                "SELECT source_url, response_hash, last_fetched_unix_ms, diagnostic_code
                 FROM compatibility_sync_state WHERE singleton=1",
                [],
                |row| {
                    Ok(CompatibilitySyncState {
                        source_url: row.get(0)?,
                        response_hash: row.get(1)?,
                        last_fetched_unix_ms: row.get(2)?,
                        diagnostic_code: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Number of entries currently materialized in the local cache.
    pub fn compatibility_cache_entry_count(&self) -> Result<u32, LibraryError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM compatibility_entry_cache",
            [],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// Record a failed sync diagnostic without disturbing the cached entries.
    /// Updates the singleton row if present, otherwise inserts it.
    pub fn record_compatibility_diagnostic(
        &mut self,
        source_url: &str,
        diagnostic_code: &str,
        now_unix_ms: i64,
    ) -> Result<(), LibraryError> {
        validate_symbol(diagnostic_code)?;
        self.connection.execute(
            "INSERT INTO compatibility_sync_state(
                 singleton, source_url, response_hash, last_fetched_unix_ms, diagnostic_code)
             VALUES(1, ?1, '', ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
                 diagnostic_code=excluded.diagnostic_code,
                 last_fetched_unix_ms=excluded.last_fetched_unix_ms",
            params![source_url, now_unix_ms, diagnostic_code],
        )?;
        tracing::warn!(
            event = "astra.emu.compatibility.diagnostic",
            diagnostic_code = %diagnostic_code
        );
        Ok(())
    }

    /// Persist the VNDB releases (rIDs) fetched for a work, replacing any
    /// previously cached release list for that work.
    pub fn replace_vn_releases(
        &mut self,
        work_id: &str,
        releases: &[VnReleaseRecord],
    ) -> Result<(), LibraryError> {
        validate_symbol(work_id)?;
        for release in releases {
            validate_symbol(&release.work_id)?;
            validate_symbol(&release.release_id)?;
            if release.work_id != work_id {
                return Err(LibraryError::InvalidSymbol(
                    "vn_release_work_id_mismatch".into(),
                ));
            }
            serde_json::from_str::<serde_json::Value>(&release.platforms_json)
                .map_err(|_| LibraryError::InvalidSymbol("vn_release_platforms_json".into()))?;
        }
        let tx = self.connection.transaction()?;
        let existing: Vec<String> = {
            let mut statement = tx.prepare("SELECT release_id FROM vn_release WHERE work_id=?1")?;
            let rows = statement
                .query_map([work_id], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for release_id in existing {
            if !releases
                .iter()
                .any(|release| release.release_id == release_id)
            {
                tx.execute(
                    "DELETE FROM vn_release WHERE work_id=?1 AND release_id=?2",
                    params![work_id, release_id],
                )?;
            }
        }
        {
            let mut insert = tx.prepare(
                "INSERT INTO vn_release(work_id, release_id, title, released, platforms_json, fetched_at_unix_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(work_id, release_id) DO UPDATE SET
                     title=excluded.title, released=excluded.released,
                     platforms_json=excluded.platforms_json,
                     fetched_at_unix_ms=excluded.fetched_at_unix_ms",
            )?;
            for release in releases {
                insert.execute(params![
                    release.work_id,
                    release.release_id,
                    release.title,
                    release.released,
                    release.platforms_json,
                    release.fetched_at_unix_ms,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Releases (rIDs) currently cached for a work, ordered newest-first.
    pub fn vn_releases(&self, work_id: &str) -> Result<Vec<VnReleaseRecord>, LibraryError> {
        validate_symbol(work_id)?;
        let mut statement = self.connection.prepare(
            "SELECT work_id, release_id, title, released, platforms_json, fetched_at_unix_ms
             FROM vn_release WHERE work_id=?1
             ORDER BY COALESCE(released, '') DESC, release_id",
        )?;
        let records = statement
            .query_map([work_id], |row| {
                Ok(VnReleaseRecord {
                    work_id: row.get(0)?,
                    release_id: row.get(1)?,
                    title: row.get(2)?,
                    released: row.get(3)?,
                    platforms_json: row.get(4)?,
                    fetched_at_unix_ms: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// The release (rID) an installation case is pinned to, if any.
    pub fn case_release(&self, case_identity: &str) -> Result<Option<String>, LibraryError> {
        validate_symbol(case_identity)?;
        self.connection
            .query_row(
                "SELECT release_id FROM case_release WHERE case_identity=?1",
                [case_identity],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Pin an installation case to a specific VNDB release (rID).
    pub fn set_case_release(
        &mut self,
        case_identity: &str,
        release_id: &str,
        now_unix_ms: i64,
    ) -> Result<(), LibraryError> {
        validate_symbol(case_identity)?;
        validate_symbol(release_id)?;
        let work: Option<String> = self
            .connection
            .query_row(
                "SELECT work_id FROM library_case WHERE case_identity=?1",
                [case_identity],
                |row| row.get(0),
            )
            .optional()?;
        let Some(work_id) = work else {
            return Err(LibraryError::InvalidSymbol("case_identity".into()));
        };
        let pinned: bool = self
            .connection
            .query_row(
                "SELECT 1 FROM vn_release WHERE work_id=?1 AND release_id=?2",
                params![work_id, release_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !pinned {
            return Err(LibraryError::InvalidSymbol("release_id_not_cached".into()));
        }
        self.connection.execute(
            "INSERT INTO case_release(case_identity, release_id, picked_at_unix_ms)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(case_identity) DO UPDATE SET
                 release_id=excluded.release_id, picked_at_unix_ms=excluded.picked_at_unix_ms",
            params![case_identity, release_id, now_unix_ms],
        )?;
        Ok(())
    }

    /// Match a work's VNDB identity against the compatibility cache, precise
    /// to the release (rID) the installation is pinned to. When no release is
    /// pinned, the most recently updated entry for the vID is returned.
    pub fn compatibility_match(
        &self,
        work_id: &str,
        pinned_release_id: Option<&str>,
    ) -> Result<Option<CompatibilityMatch>, LibraryError> {
        validate_symbol(work_id)?;
        let row = self
            .connection
            .query_row(
                "SELECT e.remote_id, ?2, c.status, c.notes, c.entry_updated_unix_ms
                 FROM external_identity e
                 JOIN compatibility_entry_cache c ON c.vn_id = e.remote_id
                 WHERE e.work_id = ?1 AND e.provider = 'vndb'
                   AND c.release_id = ?2
                 LIMIT 1",
                params![work_id, pinned_release_id.unwrap_or("")],
                |row| {
                    Ok(CompatibilityMatch {
                        work_id: work_id.to_owned(),
                        vn_id: row.get(0)?,
                        release_id: row.get(1)?,
                        status: row.get(2)?,
                        notes: row.get(3)?,
                        entry_updated_unix_ms: row.get(4)?,
                    })
                },
            )
            .optional()?;
        if row.is_some() {
            tracing::trace!(event = "astra.emu.compatibility.match", work_id = %work_id);
            return Ok(row);
        }
        if pinned_release_id.is_some() {
            return Ok(None);
        }
        // Without a pinned release, show the most recently updated entry for
        // the vID as version-agnostic information.
        Ok(self
            .connection
            .query_row(
                "SELECT e.remote_id, c.release_id, c.status, c.notes, c.entry_updated_unix_ms
                 FROM external_identity e
                 JOIN compatibility_entry_cache c ON c.vn_id = e.remote_id
                 WHERE e.work_id = ?1 AND e.provider = 'vndb'
                 ORDER BY c.entry_updated_unix_ms DESC
                 LIMIT 1",
                [work_id],
                |row| {
                    Ok(CompatibilityMatch {
                        work_id: work_id.to_owned(),
                        vn_id: row.get(0)?,
                        release_id: row.get(1)?,
                        status: row.get(2)?,
                        notes: row.get(3)?,
                        entry_updated_unix_ms: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests_support::{link_identity, open_library, seed_case};

    fn sync_state(hash: &str) -> CompatibilitySyncState {
        CompatibilitySyncState {
            source_url: "https://example.com/compatibility.json".into(),
            response_hash: hash.into(),
            last_fetched_unix_ms: 1_700_000_000_500,
            diagnostic_code: None,
        }
    }

    fn entry_updated(
        vn_id: &str,
        release_id: &str,
        status: &str,
        updated: i64,
    ) -> CompatibilityCacheEntry {
        CompatibilityCacheEntry {
            vn_id: vn_id.into(),
            release_id: release_id.into(),
            status: status.into(),
            notes: Some("community note".into()),
            entry_updated_unix_ms: updated,
            fetched_at_unix_ms: 1_700_000_000_500,
        }
    }

    fn entry(vn_id: &str, release_id: &str, status: &str) -> CompatibilityCacheEntry {
        entry_updated(vn_id, release_id, status, 1_700_000_000_000)
    }

    fn release(work_id: &str, release_id: &str, released: &str) -> VnReleaseRecord {
        VnReleaseRecord {
            work_id: work_id.into(),
            release_id: release_id.into(),
            title: Some("Release".into()),
            released: Some(released.into()),
            platforms_json: "[]".into(),
            fetched_at_unix_ms: 1_700_000_000_500,
        }
    }

    #[test]
    fn replace_cache_materializes_entries_and_sync_state() {
        let mut library = open_library();
        library
            .replace_compatibility_cache(
                &[
                    entry("v17", "r123", "perfect"),
                    entry("v17", "r456", "flawed"),
                ],
                &sync_state("hash-1"),
            )
            .unwrap();
        let state = library.compatibility_sync_state().unwrap().unwrap();
        assert_eq!(state.response_hash, "hash-1");
        let count = library.compatibility_cache_entry_count().unwrap();
        assert_eq!(count, 2);
        // Replacing again fully overwrites the cache and updates the singleton.
        library
            .replace_compatibility_cache(
                &[entry("v17", "r999", "unplayable")],
                &sync_state("hash-2"),
            )
            .unwrap();
        let state = library.compatibility_sync_state().unwrap().unwrap();
        assert_eq!(state.response_hash, "hash-2");
        assert_eq!(library.compatibility_cache_entry_count().unwrap(), 1);
    }

    #[test]
    fn match_is_precise_to_pinned_release_rid() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        library
            .replace_compatibility_cache(
                &[
                    entry("v17", "r123", "perfect"),
                    entry("v17", "r456", "boot_only"),
                ],
                &sync_state("hash-1"),
            )
            .unwrap();
        library
            .replace_vn_releases(
                &work_id,
                &[
                    release(&work_id, "r123", "2020-01-01"),
                    release(&work_id, "r456", "2019-01-01"),
                ],
            )
            .unwrap();
        library
            .set_case_release("case-a", "r456", 1_700_000_001_000)
            .unwrap();
        let matched = library
            .compatibility_match(&work_id, Some("r456"))
            .unwrap()
            .unwrap();
        assert_eq!(matched.status, "boot_only");
        assert_eq!(matched.release_id, "r456");
        assert_eq!(matched.vn_id, "v17");
    }

    #[test]
    fn match_falls_back_to_most_recent_without_pinned_release() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        library
            .replace_compatibility_cache(
                &[
                    entry_updated("v17", "r123", "flawed", 1_700_000_000_000),
                    entry_updated("v17", "r456", "perfect", 1_700_000_000_001),
                ],
                &sync_state("hash-1"),
            )
            .unwrap();
        let matched = library
            .compatibility_match(&work_id, None)
            .unwrap()
            .unwrap();
        assert_eq!(matched.status, "perfect");
    }

    #[test]
    fn match_does_not_substitute_another_release_for_a_pin() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        library
            .replace_compatibility_cache(&[entry("v17", "r123", "perfect")], &sync_state("hash-1"))
            .unwrap();
        assert!(library
            .compatibility_match(&work_id, Some("r456"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn replacing_releases_only_mutates_the_requested_work() {
        let mut library = open_library();
        let work_a = seed_case(&mut library, "case-a");
        let work_b = seed_case(&mut library, "case-b");
        library
            .replace_vn_releases(&work_a, &[release(&work_a, "r123", "2020-01-01")])
            .unwrap();
        library
            .replace_vn_releases(&work_b, &[release(&work_b, "r456", "2021-01-01")])
            .unwrap();
        library.replace_vn_releases(&work_a, &[]).unwrap();
        assert!(library.vn_releases(&work_a).unwrap().is_empty());
        assert_eq!(library.vn_releases(&work_b).unwrap()[0].release_id, "r456");
    }

    #[test]
    fn match_returns_none_when_vndb_unlisted() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        library
            .replace_compatibility_cache(&[entry("v99", "r999", "perfect")], &sync_state("h"))
            .unwrap();
        assert!(library
            .compatibility_match(&work_id, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn case_release_requires_cached_release() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        let err = library
            .set_case_release("case-a", "r123", 1_700_000_001_000)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "ASTRA_EMU_LIBRARY_INVALID_SYMBOL: release_id_not_cached"
        );
        library
            .replace_vn_releases(&work_id, &[release(&work_id, "r123", "2020-01-01")])
            .unwrap();
        library
            .set_case_release("case-a", "r123", 1_700_000_001_000)
            .unwrap();
        assert_eq!(
            library.case_release("case-a").unwrap().as_deref(),
            Some("r123")
        );
    }

    #[test]
    fn diagnostic_is_recorded_without_clearing_cache() {
        let mut library = open_library();
        library
            .replace_compatibility_cache(&[entry("v17", "r123", "perfect")], &sync_state("hash-1"))
            .unwrap();
        library
            .record_compatibility_diagnostic(
                "https://example.com/compatibility.json",
                "network",
                1_700_000_001_000,
            )
            .unwrap();
        let state = library.compatibility_sync_state().unwrap().unwrap();
        assert_eq!(state.diagnostic_code.as_deref(), Some("network"));
        assert_eq!(state.response_hash, "hash-1");
    }
}
