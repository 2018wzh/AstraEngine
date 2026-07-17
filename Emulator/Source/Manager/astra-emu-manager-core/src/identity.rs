use astra_core::Hash256;
use rusqlite::{params, OptionalExtension, Transaction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::library::{validate_symbol, Library, LibraryError, ScanCandidate};

pub const MATCHER_VERSION: &str = "astra.emu.metadata_matcher.v1";
const FINGERPRINT_SCHEMA: &str = "astra.emu.installation_fingerprint.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkRecord {
    pub work_id: String,
    pub local_title: String,
    pub user_title: Option<String>,
    pub retained: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayTitle {
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InstallationRecord {
    pub case_identity: String,
    pub work_id: String,
    pub source_id: String,
    pub relative_path: String,
    pub installation_fingerprint: String,
    pub content_hash: String,
    pub title: String,
    pub family_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalIdentityRecord {
    pub work_id: String,
    pub provider: String,
    pub remote_id: String,
    pub provenance: String,
    pub verified_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataSnapshotRecord {
    pub work_id: String,
    pub provider: String,
    pub remote_id: String,
    pub normalized_json: String,
    pub response_hash: String,
    pub fetched_at_unix_ms: i64,
    pub state: String,
    pub cover_safe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MatchCandidateRecord {
    pub candidate_id: String,
    pub case_identity: String,
    pub installation_fingerprint: String,
    pub provider: String,
    pub remote_id: String,
    pub matcher_version: String,
    pub score_millis: u16,
    pub state: String,
    pub evidence_json: String,
    pub created_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MatchDecisionRecord {
    pub decision_id: String,
    pub candidate_id: String,
    pub decision: String,
    pub provenance: String,
    pub decided_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScanRunRecord {
    pub scan_id: String,
    pub source_id: String,
    pub stage: String,
    pub state: String,
    pub discovered_count: u32,
    pub matched_count: u32,
    pub failed_count: u32,
    pub diagnostic_code: Option<String>,
    pub started_at_unix_ms: i64,
    pub finished_at_unix_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderConsentRecord {
    pub provider: String,
    pub network_enabled: bool,
    pub sensitive_cover_enabled: bool,
    pub secret_reference: Option<String>,
    pub granted_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BangumiPlayStateRecord {
    pub work_id: String,
    pub subject_id: u32,
    pub status: String,
    pub rating: Option<u8>,
    pub note: Option<String>,
    pub auto_mark_doing: bool,
    pub last_synced_at_unix_ms: Option<i64>,
    pub last_diagnostic_code: Option<String>,
}

pub(crate) fn work_id_for_case(case_identity: &str) -> String {
    format!(
        "work-{}",
        &Hash256::from_sha256(format!("astra.emu.work.v1\0{case_identity}").as_bytes()).to_hex()
            [..32]
    )
}

pub(crate) fn installation_fingerprint(content_hash: &str, byte_size: i64) -> String {
    let material = format!("{FINGERPRINT_SCHEMA}\0{content_hash}\0{byte_size}");
    format!(
        "ifp-v1-{}",
        &Hash256::from_sha256(material.as_bytes()).to_hex()[..32]
    )
}

pub(crate) fn migrate_v6(tx: &Transaction<'_>) -> Result<(), LibraryError> {
    tx.execute_batch(
        "CREATE TABLE library_work (
            work_id TEXT PRIMARY KEY NOT NULL,
            local_title TEXT NOT NULL,
            user_title TEXT,
            retained INTEGER NOT NULL DEFAULT 0 CHECK(retained IN (0, 1))
         );
         ALTER TABLE library_case ADD COLUMN work_id TEXT;
         ALTER TABLE library_case ADD COLUMN installation_fingerprint TEXT;
         CREATE TABLE external_identity (
            work_id TEXT NOT NULL REFERENCES library_work(work_id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            provenance TEXT NOT NULL,
            verified_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY(work_id, provider),
            UNIQUE(provider, remote_id)
         );
         CREATE TABLE metadata_snapshot (
            work_id TEXT NOT NULL REFERENCES library_work(work_id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            normalized_json TEXT NOT NULL,
            response_hash TEXT NOT NULL,
            fetched_at_unix_ms INTEGER NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('fresh', 'stale', 'failed')),
            cover_safe INTEGER NOT NULL CHECK(cover_safe IN (0, 1)),
            PRIMARY KEY(work_id, provider)
         );
         CREATE TABLE metadata_match_candidate (
            candidate_id TEXT PRIMARY KEY NOT NULL,
            case_identity TEXT NOT NULL REFERENCES library_case(case_identity) ON DELETE CASCADE,
            installation_fingerprint TEXT NOT NULL,
            provider TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            matcher_version TEXT NOT NULL,
            score_millis INTEGER NOT NULL CHECK(score_millis BETWEEN 0 AND 1000),
            state TEXT NOT NULL CHECK(state IN ('pending', 'accepted', 'rejected', 'unlinked', 'failed')),
            evidence_json TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            UNIQUE(installation_fingerprint, provider, remote_id, matcher_version)
         );
         CREATE TABLE metadata_match_decision (
            decision_id TEXT PRIMARY KEY NOT NULL,
            candidate_id TEXT NOT NULL REFERENCES metadata_match_candidate(candidate_id) ON DELETE CASCADE,
            decision TEXT NOT NULL CHECK(decision IN ('accepted', 'rejected', 'unlinked')),
            provenance TEXT NOT NULL,
            decided_at_unix_ms INTEGER NOT NULL
         );
         CREATE TABLE library_scan_run (
            scan_id TEXT PRIMARY KEY NOT NULL,
            source_id TEXT NOT NULL REFERENCES source_grant(source_id) ON DELETE CASCADE,
            stage TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'cancelled', 'failed')),
            discovered_count INTEGER NOT NULL DEFAULT 0 CHECK(discovered_count >= 0),
            matched_count INTEGER NOT NULL DEFAULT 0 CHECK(matched_count >= 0),
            failed_count INTEGER NOT NULL DEFAULT 0 CHECK(failed_count >= 0),
            diagnostic_code TEXT,
            started_at_unix_ms INTEGER NOT NULL,
            finished_at_unix_ms INTEGER
         );
         CREATE TABLE metadata_provider_consent (
            provider TEXT PRIMARY KEY NOT NULL,
            network_enabled INTEGER NOT NULL CHECK(network_enabled IN (0, 1)),
            sensitive_cover_enabled INTEGER NOT NULL CHECK(sensitive_cover_enabled IN (0, 1)),
            secret_reference TEXT,
            granted_at_unix_ms INTEGER NOT NULL
         );
         CREATE TABLE bangumi_play_state (
            work_id TEXT PRIMARY KEY NOT NULL REFERENCES library_work(work_id) ON DELETE CASCADE,
            subject_id INTEGER NOT NULL CHECK(subject_id > 0),
            status TEXT NOT NULL CHECK(status IN ('wish', 'doing', 'collect', 'on_hold', 'dropped')),
            rating INTEGER CHECK(rating BETWEEN 1 AND 10),
            note TEXT,
            auto_mark_doing INTEGER NOT NULL DEFAULT 0 CHECK(auto_mark_doing IN (0, 1)),
            last_synced_at_unix_ms INTEGER,
            last_diagnostic_code TEXT
         );",
    )?;

    let cases = {
        let mut statement = tx.prepare(
            "SELECT case_identity, content_hash, byte_size, title FROM library_case ORDER BY case_identity",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (case_identity, content_hash, byte_size, title) in cases {
        let work_id = work_id_for_case(&case_identity);
        tx.execute(
            "INSERT INTO library_work(work_id, local_title, retained) VALUES(?1, ?2, 0)",
            params![work_id, title],
        )?;
        tx.execute(
            "UPDATE library_case SET work_id=?2, installation_fingerprint=?3 WHERE case_identity=?1",
            params![
                case_identity,
                work_id,
                installation_fingerprint(&content_hash, byte_size)
            ],
        )?;
    }
    tx.execute_batch(
        "CREATE INDEX library_case_work ON library_case(work_id);
         CREATE INDEX library_case_fingerprint ON library_case(installation_fingerprint);
         CREATE TRIGGER library_case_identity_fields_insert
         BEFORE INSERT ON library_case
         WHEN NEW.work_id IS NULL OR NEW.work_id = '' OR NEW.installation_fingerprint IS NULL OR NEW.installation_fingerprint = ''
         BEGIN SELECT RAISE(ABORT, 'ASTRA_EMU_LIBRARY_INSTALLATION_IDENTITY_REQUIRED'); END;
         CREATE TRIGGER library_case_identity_fields_update
         BEFORE UPDATE OF work_id, installation_fingerprint ON library_case
         WHEN NEW.work_id IS NULL OR NEW.work_id = '' OR NEW.installation_fingerprint IS NULL OR NEW.installation_fingerprint = ''
         BEGIN SELECT RAISE(ABORT, 'ASTRA_EMU_LIBRARY_INSTALLATION_IDENTITY_REQUIRED'); END;",
    )?;
    Ok(())
}

pub(crate) fn ensure_work_for_candidate(
    tx: &Transaction<'_>,
    candidate: &ScanCandidate,
) -> Result<(), LibraryError> {
    let work_id = work_id_for_case(&candidate.case_identity);
    tx.execute(
        "INSERT INTO library_work(work_id, local_title, retained)
         VALUES(?1, ?2, 0)
         ON CONFLICT(work_id) DO UPDATE SET local_title=excluded.local_title",
        params![work_id, candidate.title],
    )?;
    Ok(())
}

fn validate_provider(provider: &str) -> Result<(), LibraryError> {
    validate_symbol(provider)?;
    if matches!(provider, "vndb" | "bangumi") {
        Ok(())
    } else {
        Err(LibraryError::InvalidSymbol(provider.to_owned()))
    }
}

fn validate_json(value: &str) -> Result<(), LibraryError> {
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|_| LibraryError::InvalidSymbol("invalid-json".into()))
}

impl Library {
    pub fn list_works(&self) -> Result<Vec<WorkRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT work_id, local_title, user_title, retained FROM library_work
             ORDER BY COALESCE(user_title, local_title) COLLATE NOCASE, work_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(WorkRecord {
                    work_id: row.get(0)?,
                    local_title: row.get(1)?,
                    user_title: row.get(2)?,
                    retained: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn list_installations(&self) -> Result<Vec<InstallationRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT case_identity, work_id, source_id, relative_path, installation_fingerprint,
                    content_hash, title, family_override
             FROM library_case ORDER BY title COLLATE NOCASE, case_identity",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(InstallationRecord {
                    case_identity: row.get(0)?,
                    work_id: row.get(1)?,
                    source_id: row.get(2)?,
                    relative_path: row.get(3)?,
                    installation_fingerprint: row.get(4)?,
                    content_hash: row.get(5)?,
                    title: row.get(6)?,
                    family_override: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn work_for_case(&self, case_identity: &str) -> Result<Option<WorkRecord>, LibraryError> {
        validate_symbol(case_identity)?;
        self.connection
            .query_row(
                "SELECT w.work_id, w.local_title, w.user_title, w.retained
                 FROM library_case c JOIN library_work w ON w.work_id=c.work_id
                 WHERE c.case_identity=?1",
                [case_identity],
                |row| {
                    Ok(WorkRecord {
                        work_id: row.get(0)?,
                        local_title: row.get(1)?,
                        user_title: row.get(2)?,
                        retained: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn display_title_for_case(
        &self,
        case_identity: &str,
        preferred_provider: Option<&str>,
    ) -> Result<Option<DisplayTitle>, LibraryError> {
        let Some(work) = self.work_for_case(case_identity)? else {
            return Ok(None);
        };
        if let Some(value) = work.user_title.filter(|value| !value.trim().is_empty()) {
            return Ok(Some(DisplayTitle {
                value,
                source: "user".into(),
            }));
        }
        if let Some(provider) = preferred_provider {
            validate_provider(provider)?;
            let snapshot = self
                .connection
                .query_row(
                    "SELECT s.normalized_json FROM metadata_snapshot s
                     JOIN external_identity e ON e.work_id=s.work_id AND e.provider=s.provider
                         AND e.remote_id=s.remote_id
                     WHERE s.work_id=?1 AND s.provider=?2 AND s.state IN ('fresh', 'stale')",
                    params![work.work_id, provider],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(snapshot) = snapshot {
                let value: serde_json::Value = serde_json::from_str(&snapshot)
                    .map_err(|_| LibraryError::InvalidSymbol("metadata-snapshot-json".into()))?;
                if let Some(title) = value
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .filter(|title| !title.trim().is_empty())
                {
                    return Ok(Some(DisplayTitle {
                        value: title.into(),
                        source: provider.into(),
                    }));
                }
            }
        }
        Ok(Some(DisplayTitle {
            value: work.local_title,
            source: "local".into(),
        }))
    }

    pub fn upsert_external_identity(
        &mut self,
        record: &ExternalIdentityRecord,
    ) -> Result<(), LibraryError> {
        validate_symbol(&record.work_id)?;
        validate_provider(&record.provider)?;
        validate_symbol(&record.remote_id)?;
        validate_symbol(&record.provenance)?;
        let tx = self.connection.transaction()?;
        let work_id =
            merge_work_for_remote_id(&tx, &record.work_id, &record.provider, &record.remote_id)?;
        tx.execute(
            "INSERT INTO external_identity(work_id, provider, remote_id, provenance, verified_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(work_id, provider) DO UPDATE SET remote_id=excluded.remote_id,
                provenance=excluded.provenance, verified_at_unix_ms=excluded.verified_at_unix_ms",
            params![work_id, record.provider, record.remote_id, record.provenance, record.verified_at_unix_ms],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn external_identities(
        &self,
        work_id: &str,
    ) -> Result<Vec<ExternalIdentityRecord>, LibraryError> {
        validate_symbol(work_id)?;
        let mut statement = self.connection.prepare(
            "SELECT work_id, provider, remote_id, provenance, verified_at_unix_ms
             FROM external_identity WHERE work_id=?1 ORDER BY provider",
        )?;
        let records = statement
            .query_map([work_id], |row| {
                Ok(ExternalIdentityRecord {
                    work_id: row.get(0)?,
                    provider: row.get(1)?,
                    remote_id: row.get(2)?,
                    provenance: row.get(3)?,
                    verified_at_unix_ms: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn unlink_external_identity(
        &mut self,
        work_id: &str,
        provider: &str,
    ) -> Result<(), LibraryError> {
        validate_symbol(work_id)?;
        validate_provider(provider)?;
        self.connection.execute(
            "DELETE FROM external_identity WHERE work_id=?1 AND provider=?2",
            params![work_id, provider],
        )?;
        Ok(())
    }

    pub fn upsert_metadata_snapshot(
        &mut self,
        record: &MetadataSnapshotRecord,
    ) -> Result<(), LibraryError> {
        validate_symbol(&record.work_id)?;
        validate_provider(&record.provider)?;
        validate_symbol(&record.remote_id)?;
        validate_symbol(&record.response_hash)?;
        validate_json(&record.normalized_json)?;
        if !matches!(record.state.as_str(), "fresh" | "stale" | "failed") {
            return Err(LibraryError::InvalidSymbol(record.state.clone()));
        }
        self.connection.execute(
            "INSERT INTO metadata_snapshot(work_id, provider, remote_id, normalized_json,
                response_hash, fetched_at_unix_ms, state, cover_safe)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(work_id, provider) DO UPDATE SET remote_id=excluded.remote_id,
                normalized_json=excluded.normalized_json, response_hash=excluded.response_hash,
                fetched_at_unix_ms=excluded.fetched_at_unix_ms, state=excluded.state,
                cover_safe=excluded.cover_safe",
            params![
                record.work_id,
                record.provider,
                record.remote_id,
                record.normalized_json,
                record.response_hash,
                record.fetched_at_unix_ms,
                record.state,
                record.cover_safe
            ],
        )?;
        Ok(())
    }

    pub fn upsert_match_candidate(
        &mut self,
        record: &MatchCandidateRecord,
    ) -> Result<(), LibraryError> {
        validate_symbol(&record.candidate_id)?;
        validate_symbol(&record.case_identity)?;
        validate_symbol(&record.installation_fingerprint)?;
        validate_provider(&record.provider)?;
        validate_symbol(&record.remote_id)?;
        validate_symbol(&record.matcher_version)?;
        validate_json(&record.evidence_json)?;
        if record.score_millis > 1000
            || !matches!(
                record.state.as_str(),
                "pending" | "accepted" | "rejected" | "unlinked" | "failed"
            )
        {
            return Err(LibraryError::InvalidSymbol(record.state.clone()));
        }
        self.connection.execute(
            "INSERT INTO metadata_match_candidate(candidate_id, case_identity, installation_fingerprint, provider, remote_id,
                matcher_version, score_millis, state, evidence_json, created_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(installation_fingerprint, provider, remote_id, matcher_version) DO UPDATE SET
                score_millis=excluded.score_millis, evidence_json=excluded.evidence_json,
                created_at_unix_ms=excluded.created_at_unix_ms",
            params![
                record.candidate_id,
                record.case_identity,
                record.installation_fingerprint,
                record.provider,
                record.remote_id,
                record.matcher_version,
                record.score_millis,
                record.state,
                record.evidence_json,
                record.created_at_unix_ms
            ],
        )?;
        Ok(())
    }

    pub fn pending_match_candidates(&self) -> Result<Vec<MatchCandidateRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT candidate_id, case_identity, installation_fingerprint, provider, remote_id, matcher_version,
                    score_millis, state, evidence_json, created_at_unix_ms
             FROM metadata_match_candidate WHERE state='pending'
             ORDER BY created_at_unix_ms, candidate_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(MatchCandidateRecord {
                    candidate_id: row.get(0)?,
                    case_identity: row.get(1)?,
                    installation_fingerprint: row.get(2)?,
                    provider: row.get(3)?,
                    remote_id: row.get(4)?,
                    matcher_version: row.get(5)?,
                    score_millis: row.get(6)?,
                    state: row.get(7)?,
                    evidence_json: row.get(8)?,
                    created_at_unix_ms: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn record_match_decision(
        &mut self,
        record: &MatchDecisionRecord,
    ) -> Result<(), LibraryError> {
        validate_symbol(&record.decision_id)?;
        validate_symbol(&record.candidate_id)?;
        validate_symbol(&record.provenance)?;
        if !matches!(
            record.decision.as_str(),
            "accepted" | "rejected" | "unlinked"
        ) {
            return Err(LibraryError::InvalidSymbol(record.decision.clone()));
        }
        let tx = self.connection.transaction()?;
        let candidate: (String, String, String) = tx.query_row(
            "SELECT c.work_id, m.provider, m.remote_id
             FROM metadata_match_candidate m JOIN library_case c ON c.case_identity=m.case_identity
             WHERE m.candidate_id=?1",
            [&record.candidate_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        tx.execute(
            "INSERT INTO metadata_match_decision(decision_id, candidate_id, decision, provenance, decided_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![record.decision_id, record.candidate_id, record.decision, record.provenance, record.decided_at_unix_ms],
        )?;
        tx.execute(
            "UPDATE metadata_match_candidate SET state=?2 WHERE candidate_id=?1",
            params![record.candidate_id, record.decision],
        )?;
        if record.decision == "accepted" {
            let work_id = merge_work_for_remote_id(&tx, &candidate.0, &candidate.1, &candidate.2)?;
            tx.execute(
                "INSERT INTO external_identity(work_id, provider, remote_id, provenance, verified_at_unix_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(work_id, provider) DO UPDATE SET remote_id=excluded.remote_id,
                    provenance=excluded.provenance, verified_at_unix_ms=excluded.verified_at_unix_ms",
                params![work_id, candidate.1, candidate.2, record.provenance, record.decided_at_unix_ms],
            )?;
        } else if record.decision == "unlinked" {
            tx.execute(
                "DELETE FROM external_identity WHERE work_id=?1 AND provider=?2",
                params![candidate.0, candidate.1],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_scan_run(&mut self, record: &ScanRunRecord) -> Result<(), LibraryError> {
        validate_symbol(&record.scan_id)?;
        validate_symbol(&record.source_id)?;
        validate_symbol(&record.stage)?;
        if !matches!(
            record.state.as_str(),
            "running" | "completed" | "cancelled" | "failed"
        ) {
            return Err(LibraryError::InvalidSymbol(record.state.clone()));
        }
        if let Some(code) = &record.diagnostic_code {
            validate_symbol(code)?;
        }
        self.connection.execute(
            "INSERT INTO library_scan_run(scan_id, source_id, stage, state, discovered_count,
                matched_count, failed_count, diagnostic_code, started_at_unix_ms, finished_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(scan_id) DO UPDATE SET stage=excluded.stage, state=excluded.state,
                discovered_count=excluded.discovered_count, matched_count=excluded.matched_count,
                failed_count=excluded.failed_count, diagnostic_code=excluded.diagnostic_code,
                finished_at_unix_ms=excluded.finished_at_unix_ms",
            params![record.scan_id, record.source_id, record.stage, record.state,
                record.discovered_count, record.matched_count, record.failed_count,
                record.diagnostic_code, record.started_at_unix_ms, record.finished_at_unix_ms],
        )?;
        Ok(())
    }

    pub fn incomplete_scan_runs(&self) -> Result<Vec<ScanRunRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT scan_id, source_id, stage, state, discovered_count, matched_count,
                    failed_count, diagnostic_code, started_at_unix_ms, finished_at_unix_ms
             FROM library_scan_run WHERE state IN ('running', 'failed', 'cancelled')
             ORDER BY started_at_unix_ms, scan_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(ScanRunRecord {
                    scan_id: row.get(0)?,
                    source_id: row.get(1)?,
                    stage: row.get(2)?,
                    state: row.get(3)?,
                    discovered_count: row.get(4)?,
                    matched_count: row.get(5)?,
                    failed_count: row.get(6)?,
                    diagnostic_code: row.get(7)?,
                    started_at_unix_ms: row.get(8)?,
                    finished_at_unix_ms: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn set_provider_consent(
        &mut self,
        record: &ProviderConsentRecord,
    ) -> Result<(), LibraryError> {
        validate_provider(&record.provider)?;
        if let Some(reference) = &record.secret_reference {
            validate_symbol(reference)?;
        }
        self.connection.execute(
            "INSERT INTO metadata_provider_consent(provider, network_enabled,
                sensitive_cover_enabled, secret_reference, granted_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(provider) DO UPDATE SET network_enabled=excluded.network_enabled,
                sensitive_cover_enabled=excluded.sensitive_cover_enabled,
                secret_reference=excluded.secret_reference, granted_at_unix_ms=excluded.granted_at_unix_ms",
            params![record.provider, record.network_enabled, record.sensitive_cover_enabled,
                record.secret_reference, record.granted_at_unix_ms],
        )?;
        Ok(())
    }

    pub fn provider_consent(
        &self,
        provider: &str,
    ) -> Result<Option<ProviderConsentRecord>, LibraryError> {
        validate_provider(provider)?;
        self.connection
            .query_row(
                "SELECT provider, network_enabled, sensitive_cover_enabled, secret_reference,
                        granted_at_unix_ms FROM metadata_provider_consent WHERE provider=?1",
                [provider],
                |row| {
                    Ok(ProviderConsentRecord {
                        provider: row.get(0)?,
                        network_enabled: row.get(1)?,
                        sensitive_cover_enabled: row.get(2)?,
                        secret_reference: row.get(3)?,
                        granted_at_unix_ms: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_bangumi_play_state(
        &mut self,
        record: &BangumiPlayStateRecord,
    ) -> Result<(), LibraryError> {
        validate_symbol(&record.work_id)?;
        if !matches!(
            record.status.as_str(),
            "wish" | "doing" | "collect" | "on_hold" | "dropped"
        ) || record
            .rating
            .is_some_and(|rating| !(1..=10).contains(&rating))
            || record
                .note
                .as_ref()
                .is_some_and(|note| note.chars().count() > 1024)
        {
            return Err(LibraryError::InvalidSymbol("bangumi-play-state".into()));
        }
        self.connection.execute(
            "INSERT INTO bangumi_play_state(work_id, subject_id, status, rating, note,
                auto_mark_doing, last_synced_at_unix_ms, last_diagnostic_code)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(work_id) DO UPDATE SET subject_id=excluded.subject_id,
                status=excluded.status, rating=excluded.rating, note=excluded.note,
                auto_mark_doing=excluded.auto_mark_doing,
                last_synced_at_unix_ms=excluded.last_synced_at_unix_ms,
                last_diagnostic_code=excluded.last_diagnostic_code",
            params![
                record.work_id,
                record.subject_id,
                record.status,
                record.rating,
                record.note,
                record.auto_mark_doing,
                record.last_synced_at_unix_ms,
                record.last_diagnostic_code
            ],
        )?;
        Ok(())
    }

    pub fn bangumi_play_state(
        &self,
        work_id: &str,
    ) -> Result<Option<BangumiPlayStateRecord>, LibraryError> {
        validate_symbol(work_id)?;
        self.connection
            .query_row(
                "SELECT work_id, subject_id, status, rating, note, auto_mark_doing,
                        last_synced_at_unix_ms, last_diagnostic_code
                 FROM bangumi_play_state WHERE work_id=?1",
                [work_id],
                |row| {
                    Ok(BangumiPlayStateRecord {
                        work_id: row.get(0)?,
                        subject_id: row.get(1)?,
                        status: row.get(2)?,
                        rating: row.get(3)?,
                        note: row.get(4)?,
                        auto_mark_doing: row.get(5)?,
                        last_synced_at_unix_ms: row.get(6)?,
                        last_diagnostic_code: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

fn merge_work_for_remote_id(
    tx: &Transaction<'_>,
    current_work_id: &str,
    provider: &str,
    remote_id: &str,
) -> Result<String, LibraryError> {
    let existing_work_id = tx
        .query_row(
            "SELECT work_id FROM external_identity WHERE provider=?1 AND remote_id=?2",
            params![provider, remote_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(existing_work_id) = existing_work_id else {
        return Ok(current_work_id.to_owned());
    };
    if existing_work_id == current_work_id {
        return Ok(existing_work_id);
    }
    let current_identity_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM external_identity WHERE work_id=?1",
        [current_work_id],
        |row| row.get(0),
    )?;
    let current_retained: bool = tx.query_row(
        "SELECT retained FROM library_work WHERE work_id=?1",
        [current_work_id],
        |row| row.get(0),
    )?;
    if current_identity_count != 0 || current_retained {
        return Err(LibraryError::DuplicateCaseIdentity(
            "ASTRA_EMU_WORK_MERGE_CONFLICT".into(),
        ));
    }
    tx.execute(
        "UPDATE library_case SET work_id=?2 WHERE work_id=?1",
        params![current_work_id, existing_work_id],
    )?;
    tx.execute(
        "DELETE FROM library_work WHERE work_id=?1",
        [current_work_id],
    )?;
    Ok(existing_work_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CancellationToken, ScanCandidate, SourceGrant};

    fn seeded() -> Library {
        let mut library = Library::in_memory().unwrap();
        library
            .upsert_grant(&SourceGrant {
                source_id: "source-a".into(),
                alias: "A".into(),
                platform_token: "token".into(),
                token_kind: "desktop".into(),
                active: true,
            })
            .unwrap();
        library
            .apply_scan(
                "source-a",
                &[ScanCandidate {
                    source_id: "source-a".into(),
                    relative_path: "game/start.hcb".into(),
                    case_identity: "case-a".into(),
                    content_hash: "hash-a".into(),
                    modified_ns: 1,
                    byte_size: 2,
                    title: "Game".into(),
                }],
                &CancellationToken::default(),
            )
            .unwrap();
        library
    }

    #[test]
    fn v6_creates_distinct_work_and_installation_records() {
        let library = seeded();
        let works = library.list_works().unwrap();
        let installations = library.list_installations().unwrap();
        assert_eq!(works.len(), 1);
        assert_eq!(installations[0].work_id, works[0].work_id);
        assert!(installations[0]
            .installation_fingerprint
            .starts_with("ifp-v1-"));
    }

    #[test]
    fn fuzzy_candidate_requires_persisted_decision() {
        let mut library = seeded();
        let fingerprint = library.list_installations().unwrap()[0]
            .installation_fingerprint
            .clone();
        library
            .upsert_match_candidate(&MatchCandidateRecord {
                candidate_id: "candidate-a".into(),
                case_identity: "case-a".into(),
                installation_fingerprint: fingerprint,
                provider: "vndb".into(),
                remote_id: "v1".into(),
                matcher_version: MATCHER_VERSION.into(),
                score_millis: 920,
                state: "pending".into(),
                evidence_json: "[]".into(),
                created_at_unix_ms: 1,
            })
            .unwrap();
        assert_eq!(library.pending_match_candidates().unwrap().len(), 1);
        library
            .record_match_decision(&MatchDecisionRecord {
                decision_id: "decision-a".into(),
                candidate_id: "candidate-a".into(),
                decision: "accepted".into(),
                provenance: "user-confirmed".into(),
                decided_at_unix_ms: 2,
            })
            .unwrap();
        let work = library.work_for_case("case-a").unwrap().unwrap();
        assert_eq!(
            library.external_identities(&work.work_id).unwrap()[0].remote_id,
            "v1"
        );
    }

    #[test]
    fn verified_remote_identity_merges_two_installations_into_one_work() {
        let mut library = seeded();
        library
            .upsert_grant(&SourceGrant {
                source_id: "source-b".into(),
                alias: "B".into(),
                platform_token: "opaque-b".into(),
                token_kind: "test".into(),
                active: true,
            })
            .unwrap();
        library
            .apply_scan(
                "source-b",
                &[ScanCandidate {
                    source_id: "source-b".into(),
                    relative_path: "moved/start.hcb".into(),
                    case_identity: "case-b".into(),
                    content_hash:
                        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            .into(),
                    modified_ns: 2,
                    byte_size: 4,
                    title: "Game copy".into(),
                }],
                &CancellationToken::default(),
            )
            .unwrap();
        let first = library.work_for_case("case-a").unwrap().unwrap();
        let second = library.work_for_case("case-b").unwrap().unwrap();
        library
            .upsert_external_identity(&ExternalIdentityRecord {
                work_id: first.work_id,
                provider: "vndb".into(),
                remote_id: "v1".into(),
                provenance: "user-verified-id".into(),
                verified_at_unix_ms: 1,
            })
            .unwrap();
        library
            .upsert_external_identity(&ExternalIdentityRecord {
                work_id: second.work_id,
                provider: "vndb".into(),
                remote_id: "v1".into(),
                provenance: "user-verified-id".into(),
                verified_at_unix_ms: 2,
            })
            .unwrap();
        assert_eq!(library.list_works().unwrap().len(), 1);
        let installations = library.list_installations().unwrap();
        assert_eq!(installations.len(), 2);
        assert_eq!(installations[0].work_id, installations[1].work_id);
    }

    #[test]
    fn provider_consent_and_bangumi_state_never_store_secret_value() {
        let mut library = seeded();
        let work = library.work_for_case("case-a").unwrap().unwrap();
        library
            .set_provider_consent(&ProviderConsentRecord {
                provider: "bangumi".into(),
                network_enabled: true,
                sensitive_cover_enabled: false,
                secret_reference: Some("bangumi.default".into()),
                granted_at_unix_ms: 1,
            })
            .unwrap();
        library
            .set_bangumi_play_state(&BangumiPlayStateRecord {
                work_id: work.work_id,
                subject_id: 1,
                status: "doing".into(),
                rating: None,
                note: None,
                auto_mark_doing: true,
                last_synced_at_unix_ms: None,
                last_diagnostic_code: None,
            })
            .unwrap();
        assert_eq!(
            library
                .provider_consent("bangumi")
                .unwrap()
                .unwrap()
                .secret_reference
                .as_deref(),
            Some("bangumi.default")
        );
    }
}
