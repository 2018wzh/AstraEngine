use bangumi_api::{
    common::model::BangumiClient,
    module::{
        collection::model::{CollectionSubjectUpdate, CollectionType},
        subject::model::{
            Subject, SubjectSearch, SubjectSearchFilter, SubjectSearchSort, SubjectType,
        },
    },
};

use crate::{
    BangumiPlayStatus, BangumiPlayUpdate, CoverAsset, CoverFetcher, CoverPolicy, MetadataError,
    MetadataProvider, MetadataProviderId, MetadataRecord, MetadataSearchQuery, RemoteCover,
};

const BANGUMI_BASE_URL: &str = "https://api.bgm.tv";
const ASTRA_EMU_USER_AGENT: &str =
    "AstraEngine/AstraEMU/0.1 (https://github.com/AstraEngine/AstraEngine)";

#[derive(Debug, Clone)]
pub struct BangumiProviderConfig {
    pub network_consent: bool,
    pub access_token: Option<String>,
    pub timeout: std::time::Duration,
}

pub struct BangumiProvider {
    client: BangumiClient,
    cover_fetcher: CoverFetcher,
    network_consent: bool,
}

impl BangumiProvider {
    pub fn new(config: BangumiProviderConfig) -> Result<Self, MetadataError> {
        let http = reqwest12::Client::builder()
            .use_rustls_tls()
            .redirect(reqwest12::redirect::Policy::none())
            .timeout(config.timeout)
            .build()
            .map_err(|error| MetadataError::Network(error.to_string()))?;
        let mut client = BangumiClient::new(
            BANGUMI_BASE_URL.into(),
            Some(ASTRA_EMU_USER_AGENT.into()),
            config.access_token,
        );
        client.client = http;
        Ok(Self {
            client,
            cover_fetcher: CoverFetcher::new(CoverPolicy::default())?,
            network_consent: config.network_consent,
        })
    }

    fn authorize(&self) -> Result<(), MetadataError> {
        if self.network_consent {
            Ok(())
        } else {
            Err(MetadataError::ConsentRequired("bangumi"))
        }
    }

    pub async fn sync_play_status(&self, update: &BangumiPlayUpdate) -> Result<(), MetadataError> {
        self.authorize()?;
        update.validate()?;
        if self.client.access_token.is_none() {
            return Err(MetadataError::ConsentRequired("bangumi-token"));
        }
        let status = match update.status {
            BangumiPlayStatus::Wish => CollectionType::Wish,
            BangumiPlayStatus::Doing => CollectionType::Doing,
            BangumiPlayStatus::Collect => CollectionType::Done,
            BangumiPlayStatus::OnHold => CollectionType::OnHold,
            BangumiPlayStatus::Dropped => CollectionType::Dropped,
        };
        self.client
            .post_collection_subject(
                update.subject_id,
                Some(CollectionSubjectUpdate {
                    r#type: Some(status),
                    rate: update.rating.map(u32::from),
                    ep_status: None,
                    vol_status: None,
                    comment: update.note.clone(),
                    private: Some(update.private),
                    tags: None,
                }),
            )
            .await
            .map_err(map_bangumi_error)
    }
}

impl MetadataProvider for BangumiProvider {
    fn provider_id(&self) -> MetadataProviderId {
        MetadataProviderId::Bangumi
    }

    async fn search(
        &self,
        query: &MetadataSearchQuery,
    ) -> Result<Vec<MetadataRecord>, MetadataError> {
        self.authorize()?;
        query.validate()?;
        let response = self
            .client
            .search_subjects(
                Some(u32::from(query.limit)),
                Some(0),
                Some(SubjectSearch {
                    keyword: query.title.clone(),
                    sort: Some(SubjectSearchSort::Match),
                    filter: Some(SubjectSearchFilter {
                        r#type: vec![SubjectType::Game],
                        meta_tags: Vec::new(),
                        tag: Vec::new(),
                        air_date: Vec::new(),
                        rating: Vec::new(),
                        rank: Vec::new(),
                        nsfw: false,
                    }),
                }),
            )
            .await
            .map_err(map_bangumi_error)?;
        Ok(response
            .data
            .unwrap_or_default()
            .into_iter()
            .filter(|subject| matches!(&subject.r#type, SubjectType::Game))
            .map(convert_record)
            .collect())
    }

    async fn fetch_by_id(&self, remote_id: &str) -> Result<MetadataRecord, MetadataError> {
        self.authorize()?;
        let subject_id = remote_id
            .parse::<u32>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or_else(|| MetadataError::InvalidRemoteId(remote_id.to_owned()))?;
        let subject = self
            .client
            .get_subject(subject_id)
            .await
            .map_err(map_bangumi_error)?;
        if !matches!(subject.r#type, SubjectType::Game) {
            return Err(MetadataError::SchemaMismatch("bangumi-subject-not-game"));
        }
        Ok(convert_record(subject))
    }

    async fn fetch_cover(
        &self,
        record: &MetadataRecord,
        allow_sensitive: bool,
    ) -> Result<CoverAsset, MetadataError> {
        self.authorize()?;
        if record.provider != MetadataProviderId::Bangumi {
            return Err(MetadataError::InvalidRequest("provider"));
        }
        self.cover_fetcher.fetch(record, allow_sensitive).await
    }
}

fn convert_record(subject: Subject) -> MetadataRecord {
    let mut alternate_titles = Vec::new();
    if !subject.name_cn.is_empty() && subject.name_cn != subject.name {
        alternate_titles.push(subject.name_cn);
    }
    let developers = subject
        .infobox
        .iter()
        .filter(|item| matches!(item.key.as_str(), "开发" | "发行" | "品牌" | "Developer"))
        .flat_map(|item| infobox_strings(&item.value))
        .collect();
    MetadataRecord {
        provider: MetadataProviderId::Bangumi,
        remote_id: subject.id.to_string(),
        title: subject.name,
        alternate_titles,
        developers,
        release_date: subject.date,
        platforms: if subject.platform.is_empty() {
            Vec::new()
        } else {
            vec![subject.platform]
        },
        engine: None,
        cover: Some(RemoteCover {
            url: subject.images.medium,
            width: None,
            height: None,
            sexual: None,
            violence: None,
        }),
        sensitive: subject.nsfw,
    }
}

fn infobox_strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(value) => vec![value.clone()],
        serde_json::Value::Array(values) => values.iter().flat_map(infobox_strings).collect(),
        serde_json::Value::Object(value) => value
            .get("v")
            .or_else(|| value.get("value"))
            .map(infobox_strings)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn map_bangumi_error(error: impl std::fmt::Display) -> MetadataError {
    let message = error.to_string();
    if message.contains("429") {
        MetadataError::RateLimited
    } else if message.contains("401") || message.contains("403") {
        MetadataError::Unauthorized
    } else if message.contains("decode") || message.contains("missing field") {
        MetadataError::SchemaMismatch("bangumi-v0")
    } else {
        MetadataError::Upstream(format!("bangumi:{message}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_update_validation_rejects_fake_progress_or_invalid_rating() {
        let update = BangumiPlayUpdate {
            subject_id: 1,
            status: BangumiPlayStatus::Doing,
            rating: Some(11),
            note: None,
            private: true,
        };
        assert!(update.validate().is_err());
    }

    #[test]
    fn extracts_bounded_infobox_values_without_summary_payload() {
        let value = serde_json::json!([{ "v": "Key" }, { "value": "VisualArts" }]);
        assert_eq!(infobox_strings(&value), vec!["Key", "VisualArts"]);
    }
}
