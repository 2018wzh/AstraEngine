mod bangumi;
mod cover;
mod license;
mod matcher;
mod model;
mod vndb;

pub use bangumi::{BangumiProvider, BangumiProviderConfig};
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
}
