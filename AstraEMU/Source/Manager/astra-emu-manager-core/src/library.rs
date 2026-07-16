use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use astra_core::Hash256;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 5;

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceGrant {
    pub source_id: String,
    pub alias: String,
    pub platform_token: String,
    pub token_kind: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanCandidate {
    pub source_id: String,
    pub relative_path: String,
    pub case_identity: String,
    pub content_hash: String,
    pub modified_ns: i64,
    pub byte_size: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CaseRecord {
    pub case_identity: String,
    pub source_id: String,
    pub relative_path: String,
    pub content_hash: String,
    pub modified_ns: i64,
    pub byte_size: i64,
    pub title: String,
    pub family_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TranslationConsent {
    pub provider_identity: String,
    pub endpoint: String,
    pub model: String,
    pub granted_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TranslationCacheRecord {
    pub case_identity: String,
    pub source_hash: String,
    pub source_text: String,
    pub translated_text: String,
    pub provider_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TranslationProfileRecord {
    pub profile_id: String,
    pub endpoint_kind: String,
    pub endpoint: String,
    pub protocol: String,
    pub model: String,
    pub target_language: String,
    pub context_sentences: u8,
    pub body_limit_bytes: u32,
    pub timeout_ms: u64,
    pub secret_reference: String,
    pub background: Option<String>,
    pub glossary: Vec<(String, String)>,
}

type TranslationProfileRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    u8,
    u32,
    i64,
    String,
    Option<String>,
    String,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CoverCacheRecord {
    pub case_identity: String,
    pub source_hash: String,
    pub cache_relative_path: String,
    pub image_hash: String,
    pub width: u32,
    pub height: u32,
    pub byte_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceDiagnosticRecord {
    pub source_id: String,
    pub code: String,
    pub subject_hash: String,
    pub observed_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CaseRuntimeProfileRecord {
    pub case_identity: String,
    pub family_id: String,
    pub fixed_delta_ns: u64,
    pub compatibility_profile: String,
    pub family_options: BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("ASTRA_EMU_LIBRARY_SQLITE: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("ASTRA_EMU_LIBRARY_INVALID_SYMBOL: {0}")]
    InvalidSymbol(String),
    #[error("ASTRA_EMU_LIBRARY_INVALID_RELATIVE_PATH: {0}")]
    InvalidRelativePath(String),
    #[error("ASTRA_EMU_LIBRARY_DUPLICATE_CASE_IDENTITY: {0}")]
    DuplicateCaseIdentity(String),
    #[error("ASTRA_EMU_LIBRARY_SOURCE_GRANT_INACTIVE: {0}")]
    SourceGrantInactive(String),
    #[error("ASTRA_EMU_LIBRARY_SCAN_CANCELLED")]
    Cancelled,
    #[error("ASTRA_EMU_LIBRARY_SCHEMA_VERSION: found {found}, supported {supported}")]
    SchemaVersion { found: i64, supported: i64 },
}

pub struct Library {
    connection: Connection,
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
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut library = Self { connection };
        library.migrate()?;
        tracing::info!(
            event = "astra.emu.library.opened",
            schema_version = SCHEMA_VERSION
        );
        Ok(library)
    }

    fn migrate(&mut self) -> Result<(), LibraryError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(LibraryError::SchemaVersion {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        if version == 0 {
            tx.execute_batch(
                "CREATE TABLE source_grant (
                    source_id TEXT PRIMARY KEY NOT NULL,
                    alias TEXT NOT NULL,
                    platform_token TEXT NOT NULL,
                    token_kind TEXT NOT NULL,
                    active INTEGER NOT NULL CHECK(active IN (0, 1))
                 );
                 CREATE TABLE library_case (
                    case_identity TEXT PRIMARY KEY NOT NULL,
                    source_id TEXT NOT NULL REFERENCES source_grant(source_id) ON DELETE RESTRICT,
                    relative_path TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    modified_ns INTEGER NOT NULL,
                    byte_size INTEGER NOT NULL CHECK(byte_size >= 0),
                    title TEXT NOT NULL,
                    family_override TEXT,
                    UNIQUE(source_id, relative_path)
                 );
                 CREATE INDEX library_case_hash_mtime ON library_case(content_hash, modified_ns);",
            )?;
            tx.execute_batch(
                "CREATE TABLE translation_consent (
                    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
                    provider_identity TEXT NOT NULL,
                    endpoint TEXT NOT NULL,
                    model TEXT NOT NULL,
                    granted_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE TABLE translation_cache_policy (
                    case_identity TEXT PRIMARY KEY NOT NULL REFERENCES library_case(case_identity) ON DELETE CASCADE,
                    persistent INTEGER NOT NULL CHECK(persistent IN (0, 1))
                 );
                 CREATE TABLE translation_cache (
                    case_identity TEXT NOT NULL REFERENCES library_case(case_identity) ON DELETE CASCADE,
                    source_hash TEXT NOT NULL,
                    source_text TEXT NOT NULL,
                    translated_text TEXT NOT NULL,
                    provider_identity TEXT NOT NULL,
                    PRIMARY KEY(case_identity, source_hash, provider_identity)
                 );",
            )?;
            tx.pragma_update(None, "user_version", 1)?;
            version = 1;
        }
        if version == 1 {
            tx.execute_batch(
                "CREATE TABLE cover_cache (
                    case_identity TEXT PRIMARY KEY NOT NULL REFERENCES library_case(case_identity) ON DELETE CASCADE,
                    source_hash TEXT NOT NULL,
                    cache_relative_path TEXT NOT NULL,
                    image_hash TEXT NOT NULL,
                    width INTEGER NOT NULL CHECK(width > 0),
                    height INTEGER NOT NULL CHECK(height > 0),
                    byte_size INTEGER NOT NULL CHECK(byte_size >= 0)
                 );
                 CREATE TABLE source_diagnostic (
                    source_id TEXT NOT NULL REFERENCES source_grant(source_id) ON DELETE CASCADE,
                    code TEXT NOT NULL,
                    subject_hash TEXT NOT NULL,
                    observed_at_unix_ms INTEGER NOT NULL,
                    PRIMARY KEY(source_id, code, subject_hash)
                 );",
            )?;
            tx.pragma_update(None, "user_version", 2)?;
            version = 2;
        }
        if version == 2 {
            tx.execute_batch(
                "CREATE TABLE case_runtime_profile (
                    case_identity TEXT PRIMARY KEY NOT NULL REFERENCES library_case(case_identity) ON DELETE CASCADE,
                    family_id TEXT NOT NULL,
                    fixed_delta_ns INTEGER NOT NULL CHECK(fixed_delta_ns > 0),
                    compatibility_profile TEXT NOT NULL,
                    family_options_json TEXT NOT NULL
                 );",
            )?;
            tx.pragma_update(None, "user_version", 3)?;
            version = 3;
        }
        if version == 3 {
            tx.execute_batch(
                "CREATE TABLE translation_profile (
                    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
                    profile_id TEXT NOT NULL,
                    endpoint_kind TEXT NOT NULL,
                    endpoint TEXT NOT NULL,
                    protocol TEXT NOT NULL,
                    model TEXT NOT NULL,
                    target_language TEXT NOT NULL,
                    context_sentences INTEGER NOT NULL CHECK(context_sentences BETWEEN 0 AND 32),
                    body_limit_bytes INTEGER NOT NULL CHECK(body_limit_bytes BETWEEN 1 AND 16384),
                    timeout_ms INTEGER NOT NULL CHECK(timeout_ms BETWEEN 1000 AND 120000),
                    secret_reference TEXT NOT NULL,
                    background TEXT,
                    glossary_json TEXT NOT NULL
                 );",
            )?;
            tx.pragma_update(None, "user_version", 4)?;
            version = 4;
        }
        if version == 4 {
            tx.pragma_update(None, "defer_foreign_keys", true)?;
            let legacy_cases = {
                let mut statement = tx.prepare(
                    "SELECT case_identity, source_id, relative_path FROM library_case
                     WHERE case_identity LIKE 'case-sha256:%'",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (old_identity, source_id, relative_path) in legacy_cases {
                let material = format!("{source_id}\0{relative_path}");
                let new_identity = format!(
                    "case-{}",
                    &Hash256::from_sha256(material.as_bytes()).to_hex()[..32]
                );
                let collision: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM library_case WHERE case_identity=?1)",
                    [&new_identity],
                    |row| row.get(0),
                )?;
                if collision {
                    return Err(LibraryError::DuplicateCaseIdentity(new_identity));
                }
                tx.execute(
                    "UPDATE translation_cache_policy SET case_identity=?1 WHERE case_identity=?2",
                    params![new_identity, old_identity],
                )?;
                tx.execute(
                    "UPDATE translation_cache SET case_identity=?1 WHERE case_identity=?2",
                    params![new_identity, old_identity],
                )?;
                tx.execute(
                    "UPDATE case_runtime_profile SET case_identity=?1 WHERE case_identity=?2",
                    params![new_identity, old_identity],
                )?;
                tx.execute(
                    "DELETE FROM cover_cache WHERE case_identity=?1",
                    [&old_identity],
                )?;
                tx.execute(
                    "UPDATE library_case SET case_identity=?1 WHERE case_identity=?2",
                    params![new_identity, old_identity],
                )?;
            }
            tx.pragma_update(None, "user_version", 5)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_grant(&mut self, grant: &SourceGrant) -> Result<(), LibraryError> {
        validate_symbol(&grant.source_id)?;
        validate_symbol(&grant.token_kind)?;
        self.connection.execute(
            "INSERT INTO source_grant(source_id, alias, platform_token, token_kind, active)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_id) DO UPDATE SET
                alias=excluded.alias, platform_token=excluded.platform_token,
                token_kind=excluded.token_kind, active=excluded.active",
            params![
                grant.source_id,
                grant.alias,
                grant.platform_token,
                grant.token_kind,
                grant.active
            ],
        )?;
        Ok(())
    }

    pub fn apply_scan(
        &mut self,
        source_id: &str,
        candidates: &[ScanCandidate],
        cancellation: &CancellationToken,
    ) -> Result<ScanReport, LibraryError> {
        validate_symbol(source_id)?;
        if cancellation.is_cancelled() {
            return Err(LibraryError::Cancelled);
        }
        let active: Option<bool> = self
            .connection
            .query_row(
                "SELECT active FROM source_grant WHERE source_id=?1",
                [source_id],
                |row| row.get(0),
            )
            .optional()?;
        if active != Some(true) {
            return Err(LibraryError::SourceGrantInactive(source_id.to_owned()));
        }

        let mut identities = BTreeSet::new();
        let mut locations = BTreeSet::new();
        for candidate in candidates {
            if candidate.source_id != source_id {
                return Err(LibraryError::SourceGrantInactive(
                    candidate.source_id.clone(),
                ));
            }
            validate_relative_path(&candidate.relative_path)?;
            validate_symbol(&candidate.case_identity)?;
            if !identities.insert(candidate.case_identity.clone())
                || !locations.insert(candidate.relative_path.clone())
            {
                return Err(LibraryError::DuplicateCaseIdentity(
                    candidate.case_identity.clone(),
                ));
            }
        }

        tracing::debug!(
            event = "astra.emu.library.scan_apply_started",
            source_id = %source_id,
            record_count = candidates.len()
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for candidate in candidates {
            let conflicting_source: Option<String> = tx
                .query_row(
                    "SELECT source_id FROM library_case WHERE case_identity=?1 AND source_id<>?2",
                    params![candidate.case_identity, source_id],
                    |row| row.get(0),
                )
                .optional()?;
            if conflicting_source.is_some() {
                return Err(LibraryError::DuplicateCaseIdentity(
                    candidate.case_identity.clone(),
                ));
            }
        }
        let mut existing = BTreeMap::new();
        {
            let mut statement = tx.prepare(
                "SELECT case_identity, content_hash, modified_ns, byte_size, title, relative_path
                 FROM library_case WHERE source_id=?1",
            )?;
            let rows = statement.query_map([source_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ),
                ))
            })?;
            for row in rows {
                let (identity, fingerprint) = row?;
                existing.insert(identity, fingerprint);
            }
        }

        let mut report = ScanReport {
            inserted: 0,
            updated: 0,
            unchanged: 0,
            removed: 0,
        };
        for candidate in candidates {
            if cancellation.is_cancelled() {
                return Err(LibraryError::Cancelled);
            }
            let fingerprint = (
                candidate.content_hash.clone(),
                candidate.modified_ns,
                candidate.byte_size,
                candidate.title.clone(),
                candidate.relative_path.clone(),
            );
            match existing.get(&candidate.case_identity) {
                None => report.inserted += 1,
                Some(old) if old == &fingerprint => {
                    report.unchanged += 1;
                    continue;
                }
                Some(_) => report.updated += 1,
            }
            tx.execute(
                "INSERT INTO library_case(case_identity, source_id, relative_path, content_hash,
                    modified_ns, byte_size, title, family_override)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
                 ON CONFLICT(case_identity) DO UPDATE SET
                    source_id=excluded.source_id, relative_path=excluded.relative_path,
                    content_hash=excluded.content_hash, modified_ns=excluded.modified_ns,
                    byte_size=excluded.byte_size, title=excluded.title",
                params![
                    candidate.case_identity,
                    candidate.source_id,
                    candidate.relative_path,
                    candidate.content_hash,
                    candidate.modified_ns,
                    candidate.byte_size,
                    candidate.title
                ],
            )?;
        }
        for stale_identity in existing.keys().filter(|id| !identities.contains(*id)) {
            report.removed += tx.execute(
                "DELETE FROM library_case WHERE source_id=?1 AND case_identity=?2",
                params![source_id, stale_identity],
            )?;
        }
        tx.commit()?;
        Ok(report)
    }

    pub fn list_grants(&self) -> Result<Vec<SourceGrant>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT source_id, alias, platform_token, token_kind, active
             FROM source_grant ORDER BY alias, source_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SourceGrant {
                source_id: row.get(0)?,
                alias: row.get(1)?,
                platform_token: row.get(2)?,
                token_kind: row.get(3)?,
                active: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_cases(&self) -> Result<Vec<CaseRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT case_identity, source_id, relative_path, content_hash, modified_ns,
                    byte_size, title, family_override
             FROM library_case ORDER BY title COLLATE NOCASE, case_identity",
        )?;
        let rows = statement.query_map([], case_record_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn case(&self, case_identity: &str) -> Result<Option<CaseRecord>, LibraryError> {
        validate_symbol(case_identity)?;
        self.connection
            .query_row(
                "SELECT case_identity, source_id, relative_path, content_hash, modified_ns,
                        byte_size, title, family_override
                 FROM library_case WHERE case_identity=?1",
                [case_identity],
                case_record_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_family_override(
        &mut self,
        case_identity: &str,
        family_id: Option<&str>,
    ) -> Result<(), LibraryError> {
        validate_symbol(case_identity)?;
        if let Some(family_id) = family_id {
            validate_symbol(family_id)?;
        }
        let changed = self.connection.execute(
            "UPDATE library_case SET family_override=?2 WHERE case_identity=?1",
            params![case_identity, family_id],
        )?;
        if changed != 1 {
            return Err(LibraryError::InvalidSymbol(case_identity.to_owned()));
        }
        Ok(())
    }

    pub fn upsert_cover_cache(&mut self, record: &CoverCacheRecord) -> Result<(), LibraryError> {
        validate_symbol(&record.case_identity)?;
        validate_symbol(&record.source_hash)?;
        validate_symbol(&record.image_hash)?;
        validate_relative_path(&record.cache_relative_path)?;
        if record.width == 0 || record.height == 0 || record.byte_size < 0 {
            return Err(LibraryError::InvalidSymbol("cover dimensions".into()));
        }
        self.connection.execute(
            "INSERT INTO cover_cache(case_identity, source_hash, cache_relative_path, image_hash,
                width, height, byte_size) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(case_identity) DO UPDATE SET source_hash=excluded.source_hash,
                cache_relative_path=excluded.cache_relative_path, image_hash=excluded.image_hash,
                width=excluded.width, height=excluded.height, byte_size=excluded.byte_size",
            params![
                record.case_identity,
                record.source_hash,
                record.cache_relative_path,
                record.image_hash,
                record.width,
                record.height,
                record.byte_size
            ],
        )?;
        Ok(())
    }

    pub fn cover_cache(
        &self,
        case_identity: &str,
    ) -> Result<Option<CoverCacheRecord>, LibraryError> {
        validate_symbol(case_identity)?;
        self.connection
            .query_row(
                "SELECT case_identity, source_hash, cache_relative_path, image_hash, width, height,
                        byte_size FROM cover_cache WHERE case_identity=?1",
                [case_identity],
                |row| {
                    Ok(CoverCacheRecord {
                        case_identity: row.get(0)?,
                        source_hash: row.get(1)?,
                        cache_relative_path: row.get(2)?,
                        image_hash: row.get(3)?,
                        width: row.get(4)?,
                        height: row.get(5)?,
                        byte_size: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn replace_source_diagnostics(
        &mut self,
        source_id: &str,
        diagnostics: &[SourceDiagnosticRecord],
    ) -> Result<(), LibraryError> {
        validate_symbol(source_id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM source_diagnostic WHERE source_id=?1",
            [source_id],
        )?;
        for diagnostic in diagnostics {
            if diagnostic.source_id != source_id {
                return Err(LibraryError::SourceGrantInactive(
                    diagnostic.source_id.clone(),
                ));
            }
            validate_symbol(&diagnostic.code)?;
            validate_symbol(&diagnostic.subject_hash)?;
            tx.execute(
                "INSERT INTO source_diagnostic(source_id, code, subject_hash, observed_at_unix_ms)
                 VALUES(?1, ?2, ?3, ?4)",
                params![
                    diagnostic.source_id,
                    diagnostic.code,
                    diagnostic.subject_hash,
                    diagnostic.observed_at_unix_ms
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn source_diagnostics(
        &self,
        source_id: &str,
    ) -> Result<Vec<SourceDiagnosticRecord>, LibraryError> {
        validate_symbol(source_id)?;
        let mut statement = self.connection.prepare(
            "SELECT source_id, code, subject_hash, observed_at_unix_ms
             FROM source_diagnostic WHERE source_id=?1 ORDER BY code, subject_hash",
        )?;
        let rows = statement.query_map([source_id], |row| {
            Ok(SourceDiagnosticRecord {
                source_id: row.get(0)?,
                code: row.get(1)?,
                subject_hash: row.get(2)?,
                observed_at_unix_ms: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn set_case_runtime_profile(
        &mut self,
        profile: &CaseRuntimeProfileRecord,
    ) -> Result<(), LibraryError> {
        validate_symbol(&profile.case_identity)?;
        validate_symbol(&profile.family_id)?;
        validate_symbol(&profile.compatibility_profile)?;
        if profile.fixed_delta_ns == 0 || profile.fixed_delta_ns > i64::MAX as u64 {
            return Err(LibraryError::InvalidSymbol("fixed_delta_ns".into()));
        }
        for (key, value) in &profile.family_options {
            validate_symbol(key)?;
            validate_symbol(value)?;
        }
        let options = serde_json::to_string(&profile.family_options)
            .map_err(|_| LibraryError::InvalidSymbol("family_options".into()))?;
        self.connection.execute(
            "INSERT INTO case_runtime_profile(case_identity, family_id, fixed_delta_ns,
                compatibility_profile, family_options_json) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(case_identity) DO UPDATE SET family_id=excluded.family_id,
                fixed_delta_ns=excluded.fixed_delta_ns,
                compatibility_profile=excluded.compatibility_profile,
                family_options_json=excluded.family_options_json",
            params![
                profile.case_identity,
                profile.family_id,
                profile.fixed_delta_ns as i64,
                profile.compatibility_profile,
                options
            ],
        )?;
        Ok(())
    }

    pub fn case_runtime_profile(
        &self,
        case_identity: &str,
    ) -> Result<Option<CaseRuntimeProfileRecord>, LibraryError> {
        validate_symbol(case_identity)?;
        let raw: Option<(String, i64, String, String)> = self
            .connection
            .query_row(
                "SELECT family_id, fixed_delta_ns, compatibility_profile, family_options_json
                 FROM case_runtime_profile WHERE case_identity=?1",
                [case_identity],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        raw.map(
            |(family_id, fixed_delta_ns, compatibility_profile, options)| {
                let family_options = serde_json::from_str(&options)
                    .map_err(|_| LibraryError::InvalidSymbol("family_options_json".into()))?;
                Ok(CaseRuntimeProfileRecord {
                    case_identity: case_identity.to_owned(),
                    family_id,
                    fixed_delta_ns: u64::try_from(fixed_delta_ns)
                        .map_err(|_| LibraryError::InvalidSymbol("fixed_delta_ns".into()))?,
                    compatibility_profile,
                    family_options,
                })
            },
        )
        .transpose()
    }

    pub fn grant_translation_consent(
        &mut self,
        consent: &TranslationConsent,
    ) -> Result<(), LibraryError> {
        validate_symbol(&consent.provider_identity)?;
        if consent.endpoint.is_empty() || consent.model.is_empty() {
            return Err(LibraryError::InvalidSymbol(
                "translation endpoint/model".into(),
            ));
        }
        self.connection.execute(
            "INSERT INTO translation_consent(singleton, provider_identity, endpoint, model, granted_at_unix_ms)
             VALUES(1, ?1, ?2, ?3, ?4)
             ON CONFLICT(singleton) DO UPDATE SET provider_identity=excluded.provider_identity,
               endpoint=excluded.endpoint, model=excluded.model, granted_at_unix_ms=excluded.granted_at_unix_ms",
            params![consent.provider_identity, consent.endpoint, consent.model, consent.granted_at_unix_ms],
        )?;
        Ok(())
    }

    pub fn translation_consent(&self) -> Result<Option<TranslationConsent>, LibraryError> {
        self.connection.query_row(
            "SELECT provider_identity, endpoint, model, granted_at_unix_ms FROM translation_consent WHERE singleton=1",
            [],
            |row| Ok(TranslationConsent { provider_identity: row.get(0)?, endpoint: row.get(1)?, model: row.get(2)?, granted_at_unix_ms: row.get(3)? }),
        ).optional().map_err(Into::into)
    }

    pub fn set_translation_profile(
        &mut self,
        profile: &TranslationProfileRecord,
    ) -> Result<(), LibraryError> {
        for value in [
            &profile.profile_id,
            &profile.endpoint_kind,
            &profile.protocol,
            &profile.secret_reference,
        ] {
            validate_symbol(value)?;
        }
        if profile.endpoint.is_empty()
            || profile.model.is_empty()
            || profile.target_language.is_empty()
            || profile.context_sentences > 32
            || profile.body_limit_bytes == 0
            || profile.body_limit_bytes > 16 * 1024
            || !(1_000..=120_000).contains(&profile.timeout_ms)
            || profile
                .background
                .as_ref()
                .is_some_and(|value| value.len() > 16 * 1024)
            || profile.glossary.len() > 1024
            || profile.glossary.iter().any(|(source, target)| {
                source.is_empty() || target.is_empty() || source.len() > 512 || target.len() > 512
            })
        {
            return Err(LibraryError::InvalidSymbol("translation_profile".into()));
        }
        let glossary = serde_json::to_string(&profile.glossary)
            .map_err(|_| LibraryError::InvalidSymbol("translation_glossary".into()))?;
        let timeout_ms = i64::try_from(profile.timeout_ms)
            .map_err(|_| LibraryError::InvalidSymbol("translation_timeout_ms".into()))?;
        self.connection.execute(
            "INSERT INTO translation_profile(singleton, profile_id, endpoint_kind, endpoint,
               protocol, model, target_language, context_sentences, body_limit_bytes, timeout_ms,
               secret_reference, background, glossary_json)
             VALUES(1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(singleton) DO UPDATE SET profile_id=excluded.profile_id,
               endpoint_kind=excluded.endpoint_kind, endpoint=excluded.endpoint,
               protocol=excluded.protocol, model=excluded.model,
               target_language=excluded.target_language,
               context_sentences=excluded.context_sentences,
               body_limit_bytes=excluded.body_limit_bytes, timeout_ms=excluded.timeout_ms,
               secret_reference=excluded.secret_reference, background=excluded.background,
               glossary_json=excluded.glossary_json",
            params![
                profile.profile_id,
                profile.endpoint_kind,
                profile.endpoint,
                profile.protocol,
                profile.model,
                profile.target_language,
                profile.context_sentences,
                profile.body_limit_bytes,
                timeout_ms,
                profile.secret_reference,
                profile.background,
                glossary,
            ],
        )?;
        Ok(())
    }

    pub fn translation_profile(&self) -> Result<Option<TranslationProfileRecord>, LibraryError> {
        let raw: Option<TranslationProfileRow> = self
            .connection
            .query_row(
                "SELECT profile_id, endpoint_kind, endpoint, protocol, model, target_language,
                    context_sentences, body_limit_bytes, timeout_ms, secret_reference, background,
                    glossary_json FROM translation_profile WHERE singleton=1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                    ))
                },
            )
            .optional()?;
        raw.map(
            |(
                profile_id,
                endpoint_kind,
                endpoint,
                protocol,
                model,
                target_language,
                context_sentences,
                body_limit_bytes,
                timeout_ms,
                secret_reference,
                background,
                glossary_json,
            )| {
                let glossary = serde_json::from_str(&glossary_json)
                    .map_err(|_| LibraryError::InvalidSymbol("translation_glossary".into()))?;
                Ok(TranslationProfileRecord {
                    profile_id,
                    endpoint_kind,
                    endpoint,
                    protocol,
                    model,
                    target_language,
                    context_sentences,
                    body_limit_bytes,
                    timeout_ms: u64::try_from(timeout_ms).map_err(|_| {
                        LibraryError::InvalidSymbol("translation_timeout_ms".into())
                    })?,
                    secret_reference,
                    background,
                    glossary,
                })
            },
        )
        .transpose()
    }

    pub fn set_persistent_translation_cache(
        &mut self,
        case_identity: &str,
        enabled: bool,
    ) -> Result<(), LibraryError> {
        validate_symbol(case_identity)?;
        self.connection.execute(
            "INSERT INTO translation_cache_policy(case_identity, persistent) VALUES(?1, ?2)
             ON CONFLICT(case_identity) DO UPDATE SET persistent=excluded.persistent",
            params![case_identity, enabled],
        )?;
        if !enabled {
            self.connection.execute(
                "DELETE FROM translation_cache WHERE case_identity=?1",
                [case_identity],
            )?;
        }
        Ok(())
    }

    pub fn persistent_translation_cache_enabled(
        &self,
        case_identity: &str,
    ) -> Result<bool, LibraryError> {
        validate_symbol(case_identity)?;
        self.connection
            .query_row(
                "SELECT persistent FROM translation_cache_policy WHERE case_identity=?1",
                [case_identity],
                |row| row.get(0),
            )
            .optional()
            .map(|value| value == Some(true))
            .map_err(Into::into)
    }

    pub fn store_translation(
        &mut self,
        record: &TranslationCacheRecord,
    ) -> Result<bool, LibraryError> {
        validate_symbol(&record.case_identity)?;
        validate_symbol(&record.source_hash)?;
        validate_symbol(&record.provider_identity)?;
        let enabled: Option<bool> = self
            .connection
            .query_row(
                "SELECT persistent FROM translation_cache_policy WHERE case_identity=?1",
                [&record.case_identity],
                |row| row.get(0),
            )
            .optional()?;
        if enabled != Some(true) {
            return Ok(false);
        }
        self.connection.execute(
            "INSERT INTO translation_cache(case_identity, source_hash, source_text, translated_text, provider_identity)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(case_identity, source_hash, provider_identity) DO UPDATE SET
               source_text=excluded.source_text, translated_text=excluded.translated_text",
            params![record.case_identity, record.source_hash, record.source_text, record.translated_text, record.provider_identity],
        )?;
        Ok(true)
    }

    pub fn translation(
        &self,
        case_identity: &str,
        source_hash: &str,
        provider_identity: &str,
    ) -> Result<Option<TranslationCacheRecord>, LibraryError> {
        validate_symbol(case_identity)?;
        validate_symbol(source_hash)?;
        validate_symbol(provider_identity)?;
        self.connection
            .query_row(
                "SELECT source_text, translated_text FROM translation_cache
                 WHERE case_identity=?1 AND source_hash=?2 AND provider_identity=?3",
                params![case_identity, source_hash, provider_identity],
                |row| {
                    Ok(TranslationCacheRecord {
                        case_identity: case_identity.to_owned(),
                        source_hash: source_hash.to_owned(),
                        source_text: row.get(0)?,
                        translated_text: row.get(1)?,
                        provider_identity: provider_identity.to_owned(),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn translations_for_case(
        &self,
        case_identity: &str,
    ) -> Result<Vec<TranslationCacheRecord>, LibraryError> {
        validate_symbol(case_identity)?;
        let mut statement = self.connection.prepare(
            "SELECT source_hash, source_text, translated_text, provider_identity
             FROM translation_cache WHERE case_identity=?1 ORDER BY source_hash, provider_identity",
        )?;
        let rows = statement.query_map([case_identity], |row| {
            Ok(TranslationCacheRecord {
                case_identity: case_identity.to_owned(),
                source_hash: row.get(0)?,
                source_text: row.get(1)?,
                translated_text: row.get(2)?,
                provider_identity: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn case_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaseRecord> {
    Ok(CaseRecord {
        case_identity: row.get(0)?,
        source_id: row.get(1)?,
        relative_path: row.get(2)?,
        content_hash: row.get(3)?,
        modified_ns: row.get(4)?,
        byte_size: row.get(5)?,
        title: row.get(6)?,
        family_override: row.get(7)?,
    })
}

fn validate_symbol(value: &str) -> Result<(), LibraryError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
    {
        return Err(LibraryError::InvalidSymbol(value.to_owned()));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<(), LibraryError> {
    if value.is_empty()
        || value.len() > 4096
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(':')
        || value
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LibraryError::InvalidRelativePath(value.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(source_id: &str) -> SourceGrant {
        SourceGrant {
            source_id: source_id.into(),
            alias: source_id.into(),
            platform_token: "opaque".into(),
            token_kind: "desktop-bookmark".into(),
            active: true,
        }
    }

    fn candidate(source_id: &str, case_identity: &str, relative_path: &str) -> ScanCandidate {
        ScanCandidate {
            source_id: source_id.into(),
            relative_path: relative_path.into(),
            case_identity: case_identity.into(),
            content_hash: format!("hash-{case_identity}"),
            modified_ns: 1,
            byte_size: 2,
            title: case_identity.into(),
        }
    }

    #[test]
    fn cancelled_scan_rolls_back_transaction() {
        let mut library = Library::in_memory().unwrap();
        library.upsert_grant(&grant("grant-1")).unwrap();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            library.apply_scan("grant-1", &[], &cancellation),
            Err(LibraryError::Cancelled)
        ));
    }

    #[test]
    fn full_scan_removes_stale_cases_and_lists_current_records() {
        let mut library = Library::in_memory().unwrap();
        library.upsert_grant(&grant("grant-1")).unwrap();
        let cancellation = CancellationToken::default();
        let first = library
            .apply_scan(
                "grant-1",
                &[
                    candidate("grant-1", "case-a", "a/start.hcb"),
                    candidate("grant-1", "case-b", "b/start.hcb"),
                ],
                &cancellation,
            )
            .unwrap();
        assert_eq!((first.inserted, first.removed), (2, 0));

        let second = library
            .apply_scan(
                "grant-1",
                &[candidate("grant-1", "case-b", "b/start.hcb")],
                &cancellation,
            )
            .unwrap();
        assert_eq!((second.unchanged, second.removed), (1, 1));
        let cases = library.list_cases().unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].case_identity, "case-b");
    }

    #[test]
    fn duplicate_identity_across_grants_is_blocking_and_transactional() {
        let mut library = Library::in_memory().unwrap();
        library.upsert_grant(&grant("grant-1")).unwrap();
        library.upsert_grant(&grant("grant-2")).unwrap();
        let cancellation = CancellationToken::default();
        library
            .apply_scan(
                "grant-1",
                &[candidate("grant-1", "same-case", "a/start.hcb")],
                &cancellation,
            )
            .unwrap();
        assert!(matches!(
            library.apply_scan(
                "grant-2",
                &[candidate("grant-2", "same-case", "b/start.hcb")],
                &cancellation,
            ),
            Err(LibraryError::DuplicateCaseIdentity(id)) if id == "same-case"
        ));
        assert_eq!(library.list_cases().unwrap()[0].source_id, "grant-1");
    }

    #[test]
    fn version_one_database_migrates_transactionally_through_translation_profile() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE source_grant (
                    source_id TEXT PRIMARY KEY NOT NULL,
                    alias TEXT NOT NULL,
                    platform_token TEXT NOT NULL,
                    token_kind TEXT NOT NULL,
                    active INTEGER NOT NULL CHECK(active IN (0, 1))
                 );
                 CREATE TABLE library_case (
                    case_identity TEXT PRIMARY KEY NOT NULL,
                    source_id TEXT NOT NULL REFERENCES source_grant(source_id) ON DELETE RESTRICT,
                    relative_path TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    modified_ns INTEGER NOT NULL,
                    byte_size INTEGER NOT NULL CHECK(byte_size >= 0),
                    title TEXT NOT NULL,
                    family_override TEXT,
                    UNIQUE(source_id, relative_path)
                 );
                 PRAGMA user_version=1;",
            )
            .unwrap();
        let library = Library::from_connection(connection).unwrap();
        let version: i64 = library
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        let table_count: i64 = library
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'
                 AND name IN ('cover_cache', 'source_diagnostic', 'case_runtime_profile', 'translation_profile')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 4);
    }

    #[test]
    fn version_four_migration_repairs_legacy_windows_unsafe_case_identity() {
        let mut library = Library::in_memory().unwrap();
        library.upsert_grant(&grant("grant-1")).unwrap();
        library
            .apply_scan(
                "grant-1",
                &[candidate(
                    "grant-1",
                    "case-sha256:0123456789012345678",
                    "game/start.hcb",
                )],
                &CancellationToken::default(),
            )
            .unwrap();
        library
            .set_persistent_translation_cache("case-sha256:0123456789012345678", true)
            .unwrap();
        library
            .connection
            .pragma_update(None, "user_version", 4)
            .unwrap();
        library.migrate().unwrap();
        let expected = format!(
            "case-{}",
            &Hash256::from_sha256(b"grant-1\0game/start.hcb").to_hex()[..32]
        );
        assert_eq!(library.list_cases().unwrap()[0].case_identity, expected);
        assert!(library
            .persistent_translation_cache_enabled(&expected)
            .unwrap());
    }

    #[test]
    fn translation_profile_round_trips_without_secret_value() {
        let mut library = Library::in_memory().unwrap();
        let profile = TranslationProfileRecord {
            profile_id: "ecnu.default".into(),
            endpoint_kind: "ecnu".into(),
            endpoint: "https://chat.ecnu.edu.cn/open/api/v1".into(),
            protocol: "responses".into(),
            model: "example-model".into(),
            target_language: "zh-CN".into(),
            context_sentences: 10,
            body_limit_bytes: 16 * 1024,
            timeout_ms: 30_000,
            secret_reference: "ecnu.default".into(),
            background: Some("sanitized background".into()),
            glossary: vec![("Alice".into(), "爱丽丝".into())],
        };
        library.set_translation_profile(&profile).unwrap();
        assert_eq!(library.translation_profile().unwrap(), Some(profile));
    }
}
