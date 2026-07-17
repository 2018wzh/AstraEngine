use std::sync::Arc;

use vn::Vndb;

use crate::{
    CoverAsset, CoverFetcher, CoverPolicy, MetadataError, MetadataLicenseManifest,
    MetadataProvider, MetadataProviderId, MetadataRecord, MetadataSearchQuery, RemoteCover,
};

const ASTRA_EMU_USER_AGENT: &str =
    "AstraEngine/AstraEMU (https://github.com/AstraEngine/AstraEngine)";

#[derive(Debug, Clone)]
pub struct VndbProviderConfig {
    pub network_consent: bool,
    pub license: MetadataLicenseManifest,
    pub timeout: std::time::Duration,
    pub minimum_request_delay: std::time::Duration,
}

pub struct VndbProvider {
    client: Arc<Vndb>,
    cover_fetcher: CoverFetcher,
    config: VndbProviderConfig,
}

impl VndbProvider {
    pub fn new(config: VndbProviderConfig) -> Result<Self, MetadataError> {
        config.license.permit_vndb()?;
        let client = Vndb::builder()
            .max_concurrent_requests(1)
            .delay(config.minimum_request_delay)
            .timeout(config.timeout)
            .user_agent(ASTRA_EMU_USER_AGENT)
            .build();
        Ok(Self {
            client,
            cover_fetcher: CoverFetcher::new(CoverPolicy::default())?,
            config,
        })
    }

    fn authorize(&self) -> Result<(), MetadataError> {
        if !self.config.network_consent {
            return Err(MetadataError::ConsentRequired("vndb"));
        }
        self.config.license.permit_vndb()
    }

    async fn query(
        &self,
        query: vn::http::request::post::VisualNovelQuery,
    ) -> Result<Vec<MetadataRecord>, MetadataError> {
        let response = query
            .raw_fields(
                [
                    "title",
                    "alttitle",
                    "aliases",
                    "titles.lang",
                    "titles.title",
                    "titles.latin",
                    "titles.official",
                    "titles.main",
                    "released",
                    "platforms",
                    "developers.name",
                    "developers.original",
                    "image.url",
                    "image.thumbnail",
                    "image.dims",
                    "image.thumbnail_dims",
                    "image.sexual",
                    "image.violence",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .send()
            .await
            .map_err(map_vndb_error)?;
        Ok(response.results.into_iter().map(convert_record).collect())
    }
}

impl MetadataProvider for VndbProvider {
    fn provider_id(&self) -> MetadataProviderId {
        MetadataProviderId::Vndb
    }

    async fn search(
        &self,
        query: &MetadataSearchQuery,
    ) -> Result<Vec<MetadataRecord>, MetadataError> {
        self.authorize()?;
        query.validate()?;
        self.query(
            self.client
                .search_visual_novel(&query.title)
                .results(query.limit),
        )
        .await
    }

    async fn fetch_by_id(&self, remote_id: &str) -> Result<MetadataRecord, MetadataError> {
        self.authorize()?;
        if !is_vndb_id(remote_id) {
            return Err(MetadataError::InvalidRemoteId(remote_id.to_owned()));
        }
        let id = vn::VisualNovelId::new(remote_id)
            .ok_or_else(|| MetadataError::InvalidRemoteId(remote_id.to_owned()))?;
        let mut records = self.query(self.client.find_visual_novel(&id)).await?;
        if records.len() != 1 {
            return Err(MetadataError::NotFound(remote_id.to_owned()));
        }
        Ok(records.remove(0))
    }

    async fn fetch_cover(
        &self,
        record: &MetadataRecord,
        allow_sensitive: bool,
    ) -> Result<CoverAsset, MetadataError> {
        self.authorize()?;
        if record.provider != MetadataProviderId::Vndb {
            return Err(MetadataError::InvalidRequest("provider"));
        }
        self.cover_fetcher.fetch(record, allow_sensitive).await
    }
}

fn is_vndb_id(value: &str) -> bool {
    value.strip_prefix('v').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn convert_record(value: vn::VisualNovel) -> MetadataRecord {
    let mut alternate_titles = value.aliases.unwrap_or_default();
    if let Some(title) = value.alttitle {
        alternate_titles.push(title);
    }
    if let Some(titles) = value.titles {
        for title in titles {
            if let Some(title) = title.title {
                alternate_titles.push(title);
            }
            if let Some(latin) = title.latin {
                alternate_titles.push(latin);
            }
        }
    }
    alternate_titles.sort();
    alternate_titles.dedup();
    let developers = value
        .developers
        .unwrap_or_default()
        .into_iter()
        .filter_map(|developer| developer.producer.name.or(developer.producer.original))
        .collect();
    let cover = value.image.map(|image| RemoteCover {
        url: image.thumbnail.or(image.url).unwrap_or_default(),
        width: image.thumbnail_dims.or(image.dims).map(|dims| dims[0]),
        height: image.thumbnail_dims.or(image.dims).map(|dims| dims[1]),
        sexual: image.sexual,
        violence: image.violence,
    });
    let sensitive = cover.as_ref().is_some_and(|cover| {
        cover.sexual.unwrap_or(2.0) > 0.0 || cover.violence.unwrap_or(2.0) > 0.0
    });
    MetadataRecord {
        provider: MetadataProviderId::Vndb,
        remote_id: value.id.to_string(),
        title: value.title.unwrap_or_default(),
        alternate_titles,
        developers,
        release_date: value.released,
        platforms: value
            .platforms
            .unwrap_or_default()
            .into_iter()
            .map(|platform| platform.to_string())
            .collect(),
        engine: None,
        cover: cover.filter(|cover| !cover.url.is_empty()),
        sensitive,
    }
}

fn map_vndb_error(error: vn::error::Error) -> MetadataError {
    let message = error.to_string();
    if message.contains("429") {
        MetadataError::RateLimited
    } else if message.contains("401") {
        MetadataError::Unauthorized
    } else if message.contains("deserialize") || message.contains("decode") {
        MetadataError::SchemaMismatch("vndb-kana")
    } else {
        MetadataError::Upstream(format!("vndb:{message}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReleaseUse;

    #[test]
    fn validates_remote_ids_without_network() {
        assert!(is_vndb_id("v17"));
        assert!(!is_vndb_id("17"));
        assert!(!is_vndb_id("v17/path"));
    }

    #[test]
    fn consent_is_checked_before_network() {
        let provider = VndbProvider::new(VndbProviderConfig {
            network_consent: false,
            license: MetadataLicenseManifest {
                release_use: ReleaseUse::NonCommercial,
                vndb_commercial_license_id: None,
            },
            timeout: std::time::Duration::from_secs(3),
            minimum_request_delay: std::time::Duration::from_secs(1),
        })
        .unwrap();
        assert!(matches!(
            provider.authorize(),
            Err(MetadataError::ConsentRequired("vndb"))
        ));
    }
}
