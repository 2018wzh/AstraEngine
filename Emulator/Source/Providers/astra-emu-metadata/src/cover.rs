use image::GenericImageView;
use reqwest::header::CONTENT_TYPE;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{CoverAsset, MetadataError, MetadataProviderId, MetadataRecord};

#[derive(Debug, Clone)]
pub struct CoverPolicy {
    pub max_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
}

impl Default for CoverPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_width: 4096,
            max_height: 4096,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoverFetcher {
    client: reqwest::Client,
    policy: CoverPolicy,
}

impl CoverFetcher {
    pub fn new(policy: CoverPolicy) -> Result<Self, MetadataError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|error| MetadataError::Network(error.to_string()))?;
        Ok(Self { client, policy })
    }

    pub async fn fetch(
        &self,
        record: &MetadataRecord,
        allow_sensitive: bool,
    ) -> Result<CoverAsset, MetadataError> {
        let cover = record
            .cover
            .as_ref()
            .ok_or_else(|| MetadataError::NotFound("cover".into()))?;
        if record.sensitive && !allow_sensitive {
            return Err(MetadataError::CoverBlocked("sensitive"));
        }
        let url = Url::parse(&cover.url).map_err(|_| MetadataError::CoverBlocked("url"))?;
        validate_cover_url(record.provider, &url)?;
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| MetadataError::Network(error.to_string()))?;
        if response.status().is_redirection() {
            return Err(MetadataError::CoverBlocked("redirect"));
        }
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(MetadataError::RateLimited);
        }
        if !response.status().is_success() {
            return Err(MetadataError::Network(format!(
                "http-{}",
                response.status().as_u16()
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.policy.max_bytes as u64)
        {
            return Err(MetadataError::ResponseBounds("cover-content-length"));
        }
        let media_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_default()
            .to_owned();
        if !matches!(
            media_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        ) {
            return Err(MetadataError::CoverBlocked("media-type"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| MetadataError::Network(error.to_string()))?
        {
            if bytes.len().saturating_add(chunk.len()) > self.policy.max_bytes {
                return Err(MetadataError::ResponseBounds("cover-bytes"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let image =
            image::load_from_memory(&bytes).map_err(|_| MetadataError::CoverBlocked("decode"))?;
        let (width, height) = image.dimensions();
        if width == 0
            || height == 0
            || width > self.policy.max_width
            || height > self.policy.max_height
        {
            return Err(MetadataError::ResponseBounds("cover-dimensions"));
        }
        Ok(CoverAsset {
            sha256: hex::encode(Sha256::digest(&bytes)),
            bytes,
            media_type,
            width,
            height,
        })
    }
}

fn validate_cover_url(provider: MetadataProviderId, url: &Url) -> Result<(), MetadataError> {
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(MetadataError::CoverBlocked("authority"));
    }
    let host = url.host_str().ok_or(MetadataError::CoverBlocked("host"))?;
    let permitted = match provider {
        MetadataProviderId::Vndb => matches!(host, "t.vndb.org" | "s2.vndb.org" | "s.vndb.org"),
        MetadataProviderId::Bangumi => matches!(host, "lain.bgm.tv" | "api.bgm.tv"),
    };
    if permitted {
        Ok(())
    } else {
        Err(MetadataError::CoverBlocked("host"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_or_non_https_cover_hosts() {
        assert!(validate_cover_url(
            MetadataProviderId::Vndb,
            &Url::parse("http://t.vndb.org/a.jpg").unwrap()
        )
        .is_err());
        assert!(validate_cover_url(
            MetadataProviderId::Bangumi,
            &Url::parse("https://example.com/a.jpg").unwrap()
        )
        .is_err());
    }
}
