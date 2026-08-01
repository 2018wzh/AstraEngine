//! Local read-only cache of the central compatibility database and matching
//! against library works via `external_identity(provider, remote_id)`.
//!
//! Persistence lives here (library DB). Fetching and schema validation live in
//! `astra-emu-metadata`; orchestration lives in the manager.

use rusqlite::{params, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::library::{validate_symbol, Library, LibraryError};

const MAX_NOTES_CHARS: usize = 1024;

/// One cached compatibility entry, keyed by provider identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilityCacheEntry {
    pub provider: String,
    pub remote_id: String,
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

/// A compatibility cache entry matched to a work through its external
/// identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilityMatch {
    pub work_id: String,
    pub provider: String,
    pub remote_id: String,
    pub status: String,
    pub notes: Option<String>,
    pub entry_updated_unix_ms: i64,
}

impl Library {
    /// Atomically replace the whole compatibility cache and record the sync
    /// state. Entries with invalid provider/remote_id symbols are rejected.
    pub fn replace_compatibility_cache(
        &mut self,
        entries: &[CompatibilityCacheEntry],
        sync_state: &CompatibilitySyncState,
    ) -> Result<(), LibraryError> {
        for entry in entries {
            validate_symbol(&entry.provider)?;
            validate_symbol(&entry.remote_id)?;
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
                     provider, remote_id, status, notes, entry_updated_unix_ms, fetched_at_unix_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for entry in entries {
                insert.execute(params![
                    entry.provider,
                    entry.remote_id,
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

    /// Match a work's external identities against the compatibility cache.
    /// Prefers bangumi over vndb, then the most recently updated entry.
    /// Returns `None` when the work has no cached compatibility record.
    pub fn compatibility_match(
        &self,
        work_id: &str,
    ) -> Result<Option<CompatibilityMatch>, LibraryError> {
        validate_symbol(work_id)?;
        let row: Option<CompatibilityMatch> = self
            .connection
            .query_row(
                "SELECT c.provider, c.remote_id, c.status, c.notes, c.entry_updated_unix_ms
                 FROM compatibility_entry_cache c
                 JOIN external_identity e
                   ON e.provider = c.provider AND e.remote_id = c.remote_id
                 WHERE e.work_id = ?1
                 ORDER BY CASE c.provider WHEN 'bangumi' THEN 0 WHEN 'vndb' THEN 1 ELSE 2 END,
                          c.entry_updated_unix_ms DESC
                 LIMIT 1",
                [work_id],
                |row| {
                    Ok(CompatibilityMatch {
                        work_id: work_id.to_owned(),
                        provider: row.get(0)?,
                        remote_id: row.get(1)?,
                        status: row.get(2)?,
                        notes: row.get(3)?,
                        entry_updated_unix_ms: row.get(4)?,
                    })
                },
            )
            .optional()?;
        if row.is_some() {
            tracing::trace!(event = "astra.emu.compatibility.match", work_id = %work_id);
        }
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests_support::{link_identity, open_library, seed_case};

    fn entry(provider: &str, remote_id: &str, status: &str) -> CompatibilityCacheEntry {
        CompatibilityCacheEntry {
            provider: provider.into(),
            remote_id: remote_id.into(),
            status: status.into(),
            notes: Some("community note".into()),
            entry_updated_unix_ms: 1_700_000_000_000,
            fetched_at_unix_ms: 1_700_000_000_500,
        }
    }

    fn sync_state(hash: &str) -> CompatibilitySyncState {
        CompatibilitySyncState {
            source_url: "https://example.com/compatibility.json".into(),
            response_hash: hash.into(),
            last_fetched_unix_ms: 1_700_000_000_500,
            diagnostic_code: None,
        }
    }

    #[test]
    fn replace_cache_materializes_entries_and_sync_state() {
        let mut library = open_library();
        library
            .replace_compatibility_cache(
                &[
                    entry("vndb", "v17", "perfect"),
                    entry("bangumi", "12345", "flawed"),
                ],
                &sync_state("hash-1"),
            )
            .unwrap();
        let state = library.compatibility_sync_state().unwrap().unwrap();
        assert_eq!(state.response_hash, "hash-1");
        let count: i64 = library
            .connection
            .query_row(
                "SELECT COUNT(*) FROM compatibility_entry_cache",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        // Replacing again fully overwrites the cache and updates the singleton.
        library
            .replace_compatibility_cache(
                &[entry("vndb", "v99", "unplayable")],
                &sync_state("hash-2"),
            )
            .unwrap();
        let state = library.compatibility_sync_state().unwrap().unwrap();
        assert_eq!(state.response_hash, "hash-2");
        let count: i64 = library
            .connection
            .query_row(
                "SELECT COUNT(*) FROM compatibility_entry_cache",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn match_prefers_bangumi_over_vndb() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        link_identity(&library, &work_id, "bangumi", "12345");
        library
            .replace_compatibility_cache(
                &[
                    entry("vndb", "v17", "perfect"),
                    entry("bangumi", "12345", "boot_only"),
                ],
                &sync_state("hash-1"),
            )
            .unwrap();
        let matched = library.compatibility_match(&work_id).unwrap().unwrap();
        assert_eq!(matched.provider, "bangumi");
        assert_eq!(matched.status, "boot_only");
        assert_eq!(matched.work_id, work_id);
    }

    #[test]
    fn match_returns_none_when_unlisted() {
        let mut library = open_library();
        let work_id = seed_case(&mut library, "case-a");
        link_identity(&library, &work_id, "vndb", "v17");
        library
            .replace_compatibility_cache(&[entry("vndb", "v999", "perfect")], &sync_state("h"))
            .unwrap();
        assert!(library.compatibility_match(&work_id).unwrap().is_none());
    }

    #[test]
    fn diagnostic_is_recorded_without_clearing_cache() {
        let mut library = open_library();
        library
            .replace_compatibility_cache(&[entry("vndb", "v17", "perfect")], &sync_state("hash-1"))
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
        // Cache entries survive a diagnostic recording.
        assert_eq!(state.response_hash, "hash-1");
    }
}
