use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::StatusCode;
use sha2::{Digest, Sha256};

use crate::index::{
    parse_metadata_index, MetadataIndex, MAX_METADATA_FILE_BYTES, METADATA_INDEX_FILE,
};
use crate::MetadataError;

pub const DEFAULT_METADATA_BASE_URL: &str = "https://2018wzh.github.io/AstraEMU-metadata/v1/";
const MAX_ETAG_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Unchanged,
    Updated {
        index: MetadataIndex,
        etag: Option<String>,
        files: Vec<(String, Vec<u8>)>,
    },
}

#[derive(Debug, Clone)]
pub struct MetadataSyncClient {
    client: reqwest::Client,
    base_url: url::Url,
}

impl MetadataSyncClient {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, MetadataError> {
        Self::with_builder(base_url, reqwest::Client::builder().timeout(timeout))
    }

    fn with_builder(
        base_url: &str,
        builder: reqwest::ClientBuilder,
    ) -> Result<Self, MetadataError> {
        let base_url = parse_base_url(base_url)?;
        let client = builder
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| MetadataError::Network(error.to_string()))?;
        Ok(Self { client, base_url })
    }

    pub fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    pub async fn sync(
        &self,
        cached_etag: Option<&str>,
        cached: &BTreeMap<String, String>,
    ) -> Result<SyncOutcome, MetadataError> {
        let mut request = self.client.get(self.url(METADATA_INDEX_FILE)?);
        if let Some(etag) = cached_etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(network)?;
        if cached_etag.is_some() && response.status() == StatusCode::NOT_MODIFIED {
            return Ok(SyncOutcome::Unchanged);
        }
        check_status(response.status())?;
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty() && value.len() <= MAX_ETAG_BYTES)
            .map(str::to_owned);
        let bytes = read_limited(response, MAX_METADATA_FILE_BYTES)
            .await?
            .ok_or(MetadataError::ResponseBounds("index"))?;
        let index = parse_metadata_index(&bytes)?;

        let mut files = Vec::new();
        for entry in &index.files {
            if cached.get(&entry.path) == Some(&entry.sha256) {
                continue;
            }
            let response = self
                .client
                .get(self.url(&entry.path)?)
                .send()
                .await
                .map_err(network)?;
            check_status(response.status())?;
            let bytes = read_limited(response, entry.size)
                .await?
                .ok_or_else(|| MetadataError::HashMismatch(entry.path.clone()))?;
            if bytes.len() as u64 != entry.size
                || hex::encode(Sha256::digest(&bytes)) != entry.sha256
            {
                return Err(MetadataError::HashMismatch(entry.path.clone()));
            }
            files.push((entry.path.clone(), bytes));
        }
        tracing::info!(
            target: "astra_emu_metadata::sync",
            event = "astra.emu.metadata.sync",
            indexed = index.files.len(),
            downloaded = files.len(),
            "synchronized AstraEMU metadata"
        );
        Ok(SyncOutcome::Updated { index, etag, files })
    }

    fn url(&self, path: &str) -> Result<url::Url, MetadataError> {
        self.base_url
            .join(path)
            .map_err(|_| MetadataError::InvalidRequest("metadata_path"))
    }
}

fn parse_base_url(value: &str) -> Result<url::Url, MetadataError> {
    let url = url::Url::parse(value).map_err(|_| MetadataError::InvalidRequest("base_url"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err(MetadataError::InvalidRequest("base_url"));
    }
    Ok(url)
}

fn network(error: reqwest::Error) -> MetadataError {
    MetadataError::Network(error.without_url().to_string())
}

fn check_status(status: StatusCode) -> Result<(), MetadataError> {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(MetadataError::RateLimited);
    }
    if status.is_redirection() {
        return Err(MetadataError::Network("redirect".into()));
    }
    if !status.is_success() {
        return Err(MetadataError::Network(format!("http-{}", status.as_u16())));
    }
    Ok(())
}

async fn read_limited(
    mut response: reqwest::Response,
    limit: u64,
) -> Result<Option<Vec<u8>>, MetadataError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network)? {
        if (bytes.len() + chunk.len()) as u64 > limit {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some(bytes))
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
