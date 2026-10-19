//! AstraEMU compatibility database (`compatibility.json` in the AstraEMU
//! metadata repository).
//!
//! Each entry is keyed by a VNDB release id (`rID`) and the emulator plugin id
//! the report was made with, so compatibility is precise to a game version and
//! a core. The Rust types are the schema source of truth and are exported as
//! JSON Schema for the data repository.

use std::collections::BTreeSet;

use reqwest::header::IF_NONE_MATCH;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::fields::{
    is_bounded_multiline, is_bounded_text, is_platform, is_plugin_id, is_vndb_release_id,
    is_vndb_vn_id, MAX_ENTRIES,
};

pub const COMPATIBILITY_SCHEMA_VERSION: &str = "astra.emu.compatibility.v3";

pub const DEFAULT_COMPATIBILITY_SOURCE_URL: &str =
    "https://astraengine.github.io/astra-emu-compatibility/compatibility.json";

const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const MAX_NOTES_CHARS: usize = 1024;
const MAX_VERSION_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Perfect,
    Completable,
    Flawed,
    BootOnly,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityEntry {
    #[schemars(regex(pattern = r"^v[0-9]{1,63}$"))]
    pub vn_id: String,
    #[schemars(regex(pattern = r"^r[0-9]{1,63}$"))]
    pub release_id: String,
    #[schemars(regex(pattern = r"^[a-z0-9._-]{1,128}$"))]
    pub emulator: String,
    pub status: CompatibilityStatus,
    #[schemars(length(max = 1024))]
    pub notes: Option<String>,
    #[schemars(length(min = 1, max = 64))]
    pub emulator_version: Option<String>,
    #[schemars(length(min = 1, max = 64))]
    pub manager_version: Option<String>,
    #[schemars(regex(pattern = r"^(windows|linux|macos|android)$"))]
    pub platform: String,
    #[schemars(range(min = 1))]
    pub issue: Option<u64>,
    pub updated_at_unix_ms: i64,
}

impl CompatibilityEntry {
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if !is_vndb_vn_id(&self.vn_id) {
            return Err(CompatibilityError::SchemaMismatch("vn_id"));
        }
        if !is_vndb_release_id(&self.release_id) {
            return Err(CompatibilityError::SchemaMismatch("release_id"));
        }
        if !is_plugin_id(&self.emulator) {
            return Err(CompatibilityError::SchemaMismatch("emulator"));
        }
        if !is_platform(&self.platform) {
            return Err(CompatibilityError::SchemaMismatch("platform"));
        }
        if self
            .notes
            .as_deref()
            .is_some_and(|notes| !is_bounded_multiline(notes, MAX_NOTES_CHARS))
        {
            return Err(CompatibilityError::ResponseBounds("notes"));
        }
        if self
            .emulator_version
            .as_deref()
            .is_some_and(|version| !is_bounded_text(version, 1, MAX_VERSION_CHARS))
        {
            return Err(CompatibilityError::ResponseBounds("emulator_version"));
        }
        if self
            .manager_version
            .as_deref()
            .is_some_and(|version| !is_bounded_text(version, 1, MAX_VERSION_CHARS))
        {
            return Err(CompatibilityError::ResponseBounds("manager_version"));
        }
        if self.issue == Some(0) {
            return Err(CompatibilityError::SchemaMismatch("issue"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityDatabase {
    #[schemars(regex(pattern = r"^astra\.emu\.compatibility\.v3$"))]
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
        let mut keys = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !keys.insert((entry.release_id.as_str(), entry.emulator.as_str())) {
                return Err(CompatibilityError::Duplicate(format!(
                    "{}/{}",
                    entry.release_id, entry.emulator
                )));
            }
        }
        Ok(())
    }

    pub fn entry(&self, release_id: &str, emulator: &str) -> Option<&CompatibilityEntry> {
        self.entries
            .iter()
            .find(|entry| entry.release_id == release_id && entry.emulator == emulator)
    }
}

pub fn parse_compatibility_database(
    bytes: &[u8],
) -> Result<CompatibilityDatabase, CompatibilityError> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(CompatibilityError::ResponseBounds("payload"));
    }
    let database: CompatibilityDatabase =
        serde_json::from_slice(bytes).map_err(|_| CompatibilityError::SchemaMismatch("json"))?;
    database.validate()?;
    Ok(database)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityFetch {
    NotModified,
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
    #[error("ASTRA_EMU_COMPATIBILITY_DUPLICATE: {0}")]
    Duplicate(String),
}

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
    let database = parse_compatibility_database(bytes)?;
    Ok(CompatibilityFetch::Updated {
        database,
        response_hash,
    })
}

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

pub fn compatibility_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(CompatibilityDatabase)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(release_id: &str, emulator: &str, status: CompatibilityStatus) -> CompatibilityEntry {
        CompatibilityEntry {
            vn_id: "v17".into(),
            release_id: release_id.into(),
            emulator: emulator.into(),
            status,
            notes: Some("通关无异常。".into()),
            emulator_version: Some("0.2.0".into()),
            manager_version: Some("0.2.0".into()),
            platform: "windows".into(),
            issue: Some(12),
            updated_at_unix_ms: 1_700_000_000_000,
        }
    }

    fn sample_database() -> CompatibilityDatabase {
        CompatibilityDatabase {
            schema: COMPATIBILITY_SCHEMA_VERSION.to_owned(),
            generated_at_unix_ms: 1_700_000_000_000,
            entries: vec![
                entry("r123", "astra.emu.musica", CompatibilityStatus::Perfect),
                entry("r123", "sena-native-koikake", CompatibilityStatus::Flawed),
                entry("r456", "astra.emu.musica", CompatibilityStatus::BootOnly),
            ],
        }
    }

    #[test]
    fn serde_round_trip_preserves_database() {
        let database = sample_database();
        let json = serde_json::to_string(&database).unwrap();
        let parsed = parse_compatibility_database(json.as_bytes()).unwrap();
        assert_eq!(parsed, database);
        assert_eq!(
            parsed.entry("r123", "sena-native-koikake").unwrap().status,
            CompatibilityStatus::Flawed
        );
    }

    #[test]
    fn optional_fields_may_be_omitted() {
        let json = br#"{"schema":"astra.emu.compatibility.v3","generated_at_unix_ms":1,
            "entries":[{"vn_id":"v1","release_id":"r2","emulator":"astra.emu.fvp",
            "status":"completable","platform":"linux","updated_at_unix_ms":3}]}"#;
        let database = parse_compatibility_database(json).unwrap();
        assert_eq!(database.entries[0].notes, None);
        assert_eq!(database.entries[0].issue, None);
    }

    #[test]
    fn status_serializes_as_snake_case() {
        let json = serde_json::to_value(CompatibilityStatus::BootOnly).unwrap();
        assert_eq!(json, serde_json::Value::String("boot_only".into()));
        assert_eq!(CompatibilityStatus::BootOnly.as_str(), "boot_only");
    }

    #[test]
    fn duplicate_release_and_emulator_is_rejected() {
        let mut database = sample_database();
        database.entries.push(entry(
            "r123",
            "astra.emu.musica",
            CompatibilityStatus::Unplayable,
        ));
        let error = database.validate().unwrap_err();
        assert!(matches!(error, CompatibilityError::Duplicate(_)));
        assert!(error
            .to_string()
            .starts_with("ASTRA_EMU_COMPATIBILITY_DUPLICATE"));
    }

    #[test]
    fn identity_fields_are_validated() {
        for (mutate, field) in [
            (
                (|entry: &mut CompatibilityEntry| entry.vn_id = "1234".into())
                    as fn(&mut CompatibilityEntry),
                "vn_id",
            ),
            (|entry| entry.release_id = "v17".into(), "release_id"),
            (|entry| entry.release_id = String::new(), "release_id"),
            (|entry| entry.emulator = "Astra Musica".into(), "emulator"),
            (|entry| entry.platform = "dos".into(), "platform"),
            (|entry| entry.issue = Some(0), "issue"),
        ] {
            let mut database = sample_database();
            mutate(&mut database.entries[0]);
            assert!(
                matches!(database.validate(), Err(CompatibilityError::SchemaMismatch(found)) if found == field),
                "{field}"
            );
        }
        let mut database = sample_database();
        database.entries[0].notes = Some("x".repeat(1025));
        assert!(matches!(
            database.validate(),
            Err(CompatibilityError::ResponseBounds("notes"))
        ));
    }

    #[test]
    fn parse_rejects_schema_mismatch_unknown_fields_and_malformed_json() {
        let mut database = sample_database();
        database.schema = "astra.emu.compatibility.v2".into();
        let bytes = serde_json::to_vec(&database).unwrap();
        assert!(matches!(
            parse_compatibility_database(&bytes),
            Err(CompatibilityError::SchemaMismatch("schema"))
        ));
        let mut value = serde_json::to_value(sample_database()).unwrap();
        value["entries"][0]["reporter"] = "tester".into();
        assert!(matches!(
            parse_compatibility_database(&serde_json::to_vec(&value).unwrap()),
            Err(CompatibilityError::SchemaMismatch("json"))
        ));
        assert!(matches!(
            parse_compatibility_database(b"{not-json"),
            Err(CompatibilityError::SchemaMismatch("json"))
        ));
    }

    #[test]
    fn parse_response_reports_not_modified_for_cached_hash() {
        let bytes = serde_json::to_vec(&sample_database()).unwrap();
        let hash = match parse_compatibility_response(&bytes, None).unwrap() {
            CompatibilityFetch::Updated { response_hash, .. } => response_hash,
            CompatibilityFetch::NotModified => panic!("expected Updated"),
        };
        assert_eq!(
            parse_compatibility_response(&bytes, Some(&hash)).unwrap(),
            CompatibilityFetch::NotModified
        );
    }

    #[test]
    fn json_schema_export_names_entry_fields() {
        let text = compatibility_json_schema().to_string();
        assert!(text.contains("CompatibilityEntry"));
        assert!(text.contains("emulator_version"));
        assert!(text.contains("^r[0-9]{1,63}$"));
    }

    #[test]
    fn client_requires_https() {
        assert!(CompatibilityClient::new(
            "http://example.com/compatibility.json",
            std::time::Duration::from_secs(3)
        )
        .is_err());
    }
}
