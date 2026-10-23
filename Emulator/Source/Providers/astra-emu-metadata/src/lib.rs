mod compatibility;
mod cores;
mod cover;
mod fields;
mod index;
mod license;
mod matcher;
mod model;
mod presets;
mod sync;
mod vndb;

#[cfg(not(target_os = "android"))]
mod bangumi;

#[cfg(not(target_os = "android"))]
pub use bangumi::{BangumiProvider, BangumiProviderConfig};
pub use compatibility::{
    compatibility_json_schema, parse_compatibility_database, CompatibilityDatabase,
    CompatibilityEntry, CompatibilityError, CompatibilityStatus, COMPATIBILITY_SCHEMA_VERSION,
};
pub use cores::{
    parse_core_catalog, CoreCatalog, CoreCatalogEntry, CORE_CATALOG_SCHEMA_VERSION,
    CORE_RELEASE_URL_PREFIX, MAX_CORE_PACKAGE_BYTES,
};
pub use cover::{CoverFetcher, CoverPolicy};
pub use index::{
    parse_metadata_index, IndexedFile, MetadataIndex, MAX_METADATA_FILE_BYTES,
    MAX_METADATA_TOTAL_BYTES, METADATA_INDEX_FILE, METADATA_INDEX_SCHEMA_VERSION,
};
pub use license::{MetadataLicenseManifest, ReleaseUse};
pub use matcher::{
    match_metadata, normalize_title, unique_exact_match, MatchInput, MATCHER_VERSION,
};
pub use model::*;
pub use presets::{parse_preset_database, Preset, PresetDatabase, PRESET_DATABASE_SCHEMA_VERSION};
pub use sync::{MetadataSyncClient, SyncOutcome, DEFAULT_METADATA_BASE_URL};
pub use vndb::{plain_description, VndbProvider, VndbProviderConfig};

#[allow(async_fn_in_trait)]
pub trait MetadataProvider: Send + Sync {
    fn provider_id(&self) -> MetadataProviderId;
    async fn search(
        &self,
        query: &MetadataSearchQuery,
    ) -> Result<Vec<MetadataRecord>, MetadataError>;
    async fn fetch_by_id(&self, remote_id: &str) -> Result<MetadataRecord, MetadataError>;
    async fn fetch_cover(
        &self,
        record: &MetadataRecord,
        allow_sensitive: bool,
    ) -> Result<CoverAsset, MetadataError>;
    /// Fetch the concrete releases (versions) of a visual novel, keyed by the
    /// VNDB release id (`rID`). Used to pin a local installation to a specific
    /// game version so compatibility can be reported per-version. Only the VNDB
    /// provider returns releases; Bangumi is used solely for progress tracking
    /// and returns an empty list.
    async fn fetch_releases(&self, remote_id: &str) -> Result<Vec<MetadataRelease>, MetadataError>;
}
