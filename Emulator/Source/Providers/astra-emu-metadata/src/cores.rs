use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::{is_bounded_text, is_plugin_id, is_sha256_hex, MAX_ENTRIES};
use crate::MetadataError;

pub const CORE_CATALOG_SCHEMA_VERSION: &str = "astra.emu.cores.v1";
pub const CORE_RELEASE_URL_PREFIX: &str =
    "https://github.com/2018wzh/AstraEngine/releases/download/";
pub const MAX_CORE_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCatalog {
    #[schemars(regex(pattern = r"^astra\.emu\.cores\.v1$"))]
    pub schema: String,
    pub entries: Vec<CoreCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCatalogEntry {
    #[schemars(regex(pattern = r"^[a-z0-9._-]{1,128}$"))]
    pub plugin_id: String,
    #[schemars(length(min = 1, max = 64))]
    pub display_name: String,
    #[schemars(length(min = 1, max = 64))]
    pub family_name: String,
    #[schemars(length(min = 1, max = 64))]
    pub version: String,
    #[schemars(length(min = 1, max = 128))]
    pub abi: String,
    #[schemars(regex(pattern = r"^[a-z0-9_]{1,32}-[a-z0-9_]{1,32}$"))]
    pub platform: String,
    #[schemars(regex(
        pattern = r"^https://github\.com/2018wzh/AstraEngine/releases/download/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$"
    ))]
    pub url: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub sha256: String,
    #[schemars(range(min = 1, max = 536870912))]
    pub size: u64,
    #[schemars(length(min = 1, max = 128))]
    pub license: String,
    #[schemars(regex(pattern = r"^[A-Za-z0-9._-]{1,128}$"))]
    pub release_tag: String,
}

impl CoreCatalogEntry {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if !is_plugin_id(&self.plugin_id) {
            return Err(MetadataError::SchemaMismatch("core.plugin_id"));
        }
        if !is_bounded_text(&self.display_name, 1, 64) {
            return Err(MetadataError::ResponseBounds("core.display_name"));
        }
        if !is_bounded_text(&self.family_name, 1, 64) {
            return Err(MetadataError::ResponseBounds("core.family_name"));
        }
        if self.version.len() > 64 || semver::Version::parse(&self.version).is_err() {
            return Err(MetadataError::SchemaMismatch("core.version"));
        }
        if !is_bounded_text(&self.abi, 1, 128) {
            return Err(MetadataError::SchemaMismatch("core.abi"));
        }
        if !is_platform_triple(&self.platform) {
            return Err(MetadataError::SchemaMismatch("core.platform"));
        }
        if !is_release_tag(&self.release_tag) {
            return Err(MetadataError::SchemaMismatch("core.release_tag"));
        }
        if !is_release_asset_url(&self.url, &self.release_tag) {
            return Err(MetadataError::SchemaMismatch("core.url"));
        }
        if !is_sha256_hex(&self.sha256) {
            return Err(MetadataError::SchemaMismatch("core.sha256"));
        }
        if self.size == 0 || self.size > MAX_CORE_PACKAGE_BYTES {
            return Err(MetadataError::ResponseBounds("core.size"));
        }
        if !is_bounded_text(&self.license, 1, 128) {
            return Err(MetadataError::ResponseBounds("core.license"));
        }
        Ok(())
    }

    pub fn semantic_version(&self) -> Option<semver::Version> {
        semver::Version::parse(&self.version).ok()
    }
}

impl CoreCatalog {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.schema != CORE_CATALOG_SCHEMA_VERSION {
            return Err(MetadataError::SchemaMismatch("cores.schema"));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(MetadataError::ResponseBounds("cores.entries"));
        }
        let mut keys = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !keys.insert((
                entry.plugin_id.as_str(),
                entry.platform.as_str(),
                entry.version.as_str(),
            )) {
                return Err(MetadataError::Duplicate(format!(
                    "{}/{}/{}",
                    entry.plugin_id, entry.platform, entry.version
                )));
            }
        }
        Ok(())
    }

    pub fn latest(&self, plugin_id: &str, platform: &str, abi: &str) -> Option<&CoreCatalogEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.plugin_id == plugin_id && entry.platform == platform && entry.abi == abi
            })
            .filter_map(|entry| entry.semantic_version().map(|version| (version, entry)))
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, entry)| entry)
    }
}

pub fn parse_core_catalog(bytes: &[u8]) -> Result<CoreCatalog, MetadataError> {
    let catalog: CoreCatalog =
        serde_json::from_slice(bytes).map_err(|_| MetadataError::SchemaMismatch("cores.json"))?;
    catalog.validate()?;
    Ok(catalog)
}

fn is_platform_triple(value: &str) -> bool {
    let segment = |part: &str| {
        (1..=32).contains(&part.len())
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    };
    value
        .split_once('-')
        .is_some_and(|(os, arch)| segment(os) && segment(arch))
}

fn is_release_tag(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(is_asset_byte)
}

fn is_asset_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

fn is_release_asset_url(value: &str, release_tag: &str) -> bool {
    value
        .strip_prefix(CORE_RELEASE_URL_PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(tag, asset)| {
            tag == release_tag
                && !asset.is_empty()
                && asset.len() <= 256
                && asset.bytes().all(is_asset_byte)
                && asset != "."
                && asset != ".."
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: &str) -> CoreCatalogEntry {
        CoreCatalogEntry {
            plugin_id: "astra.emu.musica".into(),
            display_name: "Astra Musica".into(),
            family_name: "Musica".into(),
            version: version.into(),
            abi: "astra.emu.native.c.v3".into(),
            platform: "windows-x86_64".into(),
            url: format!(
                "{CORE_RELEASE_URL_PREFIX}emu-v{version}/astra-emu-core-astra_emu_musica-{version}-windows-x86_64.zip"
            ),
            sha256: "b".repeat(64),
            size: 1024,
            license: "MIT OR Apache-2.0".into(),
            release_tag: format!("emu-v{version}"),
        }
    }

    fn catalog(entries: Vec<CoreCatalogEntry>) -> CoreCatalog {
        CoreCatalog {
            schema: CORE_CATALOG_SCHEMA_VERSION.into(),
            entries,
        }
    }

    #[test]
    fn latest_picks_highest_semver_for_platform_and_abi() {
        let catalog = catalog(vec![entry("0.2.0"), entry("0.10.0"), entry("0.9.1")]);
        let parsed = parse_core_catalog(&serde_json::to_vec(&catalog).unwrap()).unwrap();
        let latest = parsed
            .latest(
                "astra.emu.musica",
                "windows-x86_64",
                "astra.emu.native.c.v3",
            )
            .unwrap();
        assert_eq!(latest.version, "0.10.0");
        assert!(parsed
            .latest(
                "astra.emu.musica",
                "windows-x86_64",
                "astra.emu.native.c.v2"
            )
            .is_none());
    }

    #[test]
    fn rejects_foreign_urls_duplicates_and_bad_versions() {
        let mut foreign = entry("0.2.0");
        foreign.url = "https://example.com/core.zip".into();
        assert!(catalog(vec![foreign]).validate().is_err());
        let mut mismatched_tag = entry("0.2.0");
        mismatched_tag.release_tag = "emu-v0.3.0".into();
        assert!(catalog(vec![mismatched_tag]).validate().is_err());
        let mut traversal = entry("0.2.0");
        traversal.url = format!("{CORE_RELEASE_URL_PREFIX}emu-v0.2.0/../x.zip");
        assert!(catalog(vec![traversal]).validate().is_err());
        assert!(matches!(
            catalog(vec![entry("0.2.0"), entry("0.2.0")]).validate(),
            Err(MetadataError::Duplicate(_))
        ));
        let mut bad_version = entry("0.2.0");
        bad_version.version = "latest".into();
        assert!(catalog(vec![bad_version]).validate().is_err());
        let mut oversized = entry("0.2.0");
        oversized.size = MAX_CORE_PACKAGE_BYTES + 1;
        assert!(catalog(vec![oversized]).validate().is_err());
    }
}
