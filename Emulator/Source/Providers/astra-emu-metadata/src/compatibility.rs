//! Central compatibility database client.
//!
//! The AstraEMU community maintains a read-only compatibility database as a
//! static JSON document (hosted on GitHub Pages). This module defines the
//! schema (the Rust types are the source of truth, exported as JSON Schema via
//! `schemars`), a validating fetch client built on `reqwest`, and content-hash
//! based incremental sync. Matching against library works and local caching are
//! handled by `astra-emu-manager-core`; orchestration lives in the manager.

use reqwest::header::IF_NONE_MATCH;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// JSON Schema identifier for the central compatibility document.
pub const COMPATIBILITY_SCHEMA_VERSION: &str = "astra.emu.compatibility.v1";

/// Default community-hosted source. Configurable; the data repository is
/// created and maintained separately.
pub const DEFAULT_COMPATIBILITY_SOURCE_URL: &str =
    "https://astraengine.github.io/astra-emu-compatibility/compatibility.json";

const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

/// VN adaptation quality grade (five levels, mirroring RPCS3-style buckets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    /// 完美运行: runs flawlessly end to end.
    Perfect,
    /// 可通关: completable with minor issues.
    Completable,
    /// 有瑕疵: playable but with noticeable flaws.
    Flawed,
    /// 仅能启动: boots but cannot be played through.
    BootOnly,
    /// 无法运行: does not run.
    Unplayable,
}

impl CompatibilityStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Perfect => "perfect",
            Self::Completable => "completable",
            Self::Flawed => "flawed",
            Self::BootOnly => "boot_only",
            Self::Unplayable => "unplayable",
        }
    }
}

/// A single compatibility record keyed by metadata provider identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilityEntry {
    /// "bangumi" | "vndb".
    pub provider: String,
    /// Bangumi subject id (e.g. "12345") or VNDB id (e.g. "v17").
    pub remote_id: String,
    pub status: CompatibilityStatus,
    pub notes: Option<String>,
    pub updated_at_unix_ms: i64,
    pub reporter: Option<String>,
}

impl CompatibilityEntry {
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if !matches!(self.provider.as_str(), "bangumi" | "vndb") {
            return Err(CompatibilityError::SchemaMismatch("provider"));
        }
        if self.remote_id.trim().is_empty() || self.remote_id.chars().count() > 64 {
            return Err(CompatibilityError::SchemaMismatch("remote_id"));
        }
        if self
            .notes
            .as_ref()
            .is_some_and(|notes| notes.chars().count() > 1024)
        {
            return Err(CompatibilityError::ResponseBounds("notes"));
        }
        if self
            .reporter
            .as_ref()
            .is_some_and(|reporter| reporter.chars().count() > 128)
        {
            return Err(CompatibilityError::ResponseBounds("reporter"));
        }
        Ok(())
    }
}

/// The whole compatibility document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompatibilityDatabase {
    /// Must equal [`COMPATIBILITY_SCHEMA_VERSION`].
    pub schema: String,
    pub generated_at_unix_ms: i64,
    pub entries: Vec<CompatibilityEntry>,
}

impl CompatibilityDatabase {
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if self.schema != COMPATIBILITY_SCHEMA_VERSION {
            return Err(CompatibilityError::SchemaMismatch("schema"));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(CompatibilityError::ResponseBounds("entries"));
        }
        for entry in &self.entries {
            entry.validate()?;
        }
        Ok(())
    }
}

/// Outcome of a compatibility fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityFetch {
    /// The source is unchanged relative to the cached hash (or HTTP 304).
    NotModified,
    /// A fresh, validated database plus the content hash to cache.
    Updated {
        database: CompatibilityDatabase,
        response_hash: String,
    },
}

#[derive(Debug, Error)]
pub enum CompatibilityError {
    #[error("ASTRA_EMU_COMPATIBILITY_CONSENT_REQUIRED")]
    ConsentRequired,
    #[error("ASTRA_EMU_COMPATIBILITY_SOURCE_URL")]
    InvalidSourceUrl,
    #[error("ASTRA_EMU_COMPATIBILITY_NETWORK: {0}")]
    Network(String),
    #[error("ASTRA_EMU_COMPATIBILITY_SCHEMA_MISMATCH: {0}")]
    SchemaMismatch(&'static str),
    #[error("ASTRA_EMU_COMPATIBILITY_RESPONSE_BOUNDS: {0}")]
    ResponseBounds(&'static str),
}

/// Parse and validate a downloaded compatibility payload. Content-hash based:
/// when `cached_hash` matches the payload digest the result is
/// [`CompatibilityFetch::NotModified`]. Exposed for network-free testing.
pub fn parse_compatibility_response(
    bytes: &[u8],
    cached_hash: Option<&str>,
) -> Result<CompatibilityFetch, CompatibilityError> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(CompatibilityError::ResponseBounds("payload"));
    }
    let response_hash = hex::encode(Sha256::digest(bytes));
    if cached_hash == Some(response_hash.as_str()) {
        return Ok(CompatibilityFetch::NotModified);
    }
    let database: CompatibilityDatabase =
        serde_json::from_slice(bytes).map_err(|_| CompatibilityError::SchemaMismatch("json"))?;
    database.validate()?;
    Ok(CompatibilityFetch::Updated {
        database,
        response_hash,
    })
}

/// Read-only HTTP client for the central compatibility database.
#[derive(Debug, Clone)]
pub struct CompatibilityClient {
    client: reqwest::Client,
    source_url: String,
}

impl CompatibilityClient {
    pub fn new(source_url: &str, timeout: std::time::Duration) -> Result<Self, CompatibilityError> {
        let parsed =
            url::Url::parse(source_url).map_err(|_| CompatibilityError::InvalidSourceUrl)?;
        if parsed.scheme() != "https" {
            return Err(CompatibilityError::InvalidSourceUrl);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|error| CompatibilityError::Network(error.to_string()))?;
        Ok(Self {
            client,
            source_url: source_url.to_owned(),
        })
    }

    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// Fetch the database. Gated on `network_consent`. When `cached_etag` is
    /// provided it is sent as `If-None-Match`; an HTTP 304 yields
    /// [`CompatibilityFetch::NotModified`]. Otherwise the payload is compared
    /// against `cached_hash`.
    pub async fn fetch(
        &self,
        network_consent: bool,
        cached_hash: Option<&str>,
        cached_etag: Option<&str>,
    ) -> Result<CompatibilityFetch, CompatibilityError> {
        if !network_consent {
            return Err(CompatibilityError::ConsentRequired);
        }
        let mut request = self.client.get(&self.source_url);
        if let Some(etag) = cached_etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let response = request
            .send()
            .await
            .map_err(|error| CompatibilityError::Network(error.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(CompatibilityFetch::NotModified);
        }
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(CompatibilityError::Network("rate-limited".into()));
        }
        if response.status().is_redirection() {
            return Err(CompatibilityError::Network("redirect".into()));
        }
        if !response.status().is_success() {
            return Err(CompatibilityError::Network(format!(
                "http-{}",
                response.status().as_u16()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| CompatibilityError::Network(error.to_string()))?;
        tracing::info!(
            target: "astra_emu_metadata::compatibility",
            event = "emu.compatibility.fetch",
            bytes = bytes.len(),
            "fetched compatibility database"
        );
        parse_compatibility_response(&bytes, cached_hash)
    }
}

/// Export the JSON Schema for [`CompatibilityDatabase`] (Rust types are the
/// source of truth). Used by the data repository for validation.
pub fn compatibility_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(CompatibilityDatabase)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_database() -> CompatibilityDatabase {
        CompatibilityDatabase {
            schema: COMPATIBILITY_SCHEMA_VERSION.to_owned(),
            generated_at_unix_ms: 1_700_000_000_000,
            entries: vec![
                CompatibilityEntry {
                    provider: "vndb".into(),
                    remote_id: "v17".into(),
                    status: CompatibilityStatus::Perfect,
                    notes: Some("Runs flawlessly.".into()),
                    updated_at_unix_ms: 1_700_000_000_000,
                    reporter: Some("tester".into()),
                },
                CompatibilityEntry {
                    provider: "bangumi".into(),
                    remote_id: "12345".into(),
                    status: CompatibilityStatus::BootOnly,
                    notes: None,
                    updated_at_unix_ms: 1_700_000_000_001,
                    reporter: None,
                },
            ],
        }
    }

    #[test]
    fn serde_round_trip_preserves_database() {
        let database = sample_database();
        let json = serde_json::to_string(&database).unwrap();
        let parsed: CompatibilityDatabase = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, database);
        parsed.validate().unwrap();
    }

    #[test]
    fn status_serializes_as_snake_case() {
        let json = serde_json::to_value(CompatibilityStatus::BootOnly).unwrap();
        assert_eq!(json, serde_json::Value::String("boot_only".into()));
        assert_eq!(CompatibilityStatus::BootOnly.as_str(), "boot_only");
    }

    #[test]
    fn json_schema_export_contains_schema_marker() {
        let schema = compatibility_json_schema();
        let text = schema.to_string();
        assert!(text.contains("CompatibilityDatabase"));
        assert!(text.contains("entries"));
    }

    #[test]
    fn parse_returns_updated_with_stable_hash() {
        let bytes = serde_json::to_vec(&sample_database()).unwrap();
        let first = parse_compatibility_response(&bytes, None).unwrap();
        let hash = match &first {
            CompatibilityFetch::Updated { response_hash, .. } => response_hash.clone(),
            CompatibilityFetch::NotModified => panic!("expected Updated"),
        };
        // Same payload + cached hash -> NotModified.
        assert_eq!(
            parse_compatibility_response(&bytes, Some(&hash)).unwrap(),
            CompatibilityFetch::NotModified
        );
    }

    #[test]
    fn parse_rejects_schema_mismatch() {
        let mut database = sample_database();
        database.schema = "astra.emu.compatibility.v0".into();
        let bytes = serde_json::to_vec(&database).unwrap();
        assert!(matches!(
            parse_compatibility_response(&bytes, None),
            Err(CompatibilityError::SchemaMismatch("schema"))
        ));
    }

    #[test]
    fn parse_rejects_unknown_provider_and_malformed_json() {
        let mut database = sample_database();
        database.entries[0].provider = "igdb".into();
        let bytes = serde_json::to_vec(&database).unwrap();
        assert!(matches!(
            parse_compatibility_response(&bytes, None),
            Err(CompatibilityError::SchemaMismatch("provider"))
        ));
        assert!(matches!(
            parse_compatibility_response(b"{not-json", None),
            Err(CompatibilityError::SchemaMismatch("json"))
        ));
    }

    #[test]
    fn client_requires_https_and_consent() {
        assert!(CompatibilityClient::new(
            "http://example.com/compatibility.json",
            std::time::Duration::from_secs(3)
        )
        .is_err());
        let client = CompatibilityClient::new(
            DEFAULT_COMPATIBILITY_SOURCE_URL,
            std::time::Duration::from_secs(3),
        )
        .unwrap();
        assert_eq!(client.source_url(), DEFAULT_COMPATIBILITY_SOURCE_URL);
    }
}
