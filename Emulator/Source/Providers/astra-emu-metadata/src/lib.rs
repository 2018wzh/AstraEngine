mod compatibility;
mod cover;
mod license;
mod matcher;
mod model;
mod vndb;

#[cfg(not(target_os = "android"))]
mod bangumi;

#[cfg(not(target_os = "android"))]
pub use bangumi::{BangumiProvider, BangumiProviderConfig};
pub use compatibility::{
    compatibility_json_schema, parse_compatibility_response, CompatibilityClient,
    CompatibilityDatabase, CompatibilityEntry, CompatibilityError, CompatibilityFetch,
    CompatibilityStatus, COMPATIBILITY_SCHEMA_VERSION, DEFAULT_COMPATIBILITY_SOURCE_URL,
};
pub use cover::{CoverFetcher, CoverPolicy};
pub use license::{MetadataLicenseManifest, ReleaseUse};
pub use matcher::{match_metadata, normalize_title, MatchInput, MATCHER_VERSION};
pub use model::*;
pub use vndb::{VndbProvider, VndbProviderConfig};

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
