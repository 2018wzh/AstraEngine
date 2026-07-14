#![cfg(not(target_arch = "wasm32"))]

use futures_util::StreamExt;

use astra_platform::{PackageSourcePolicy, PlatformError, PlatformErrorCode};

use crate::VerifiedPackageCache;

pub struct HttpRangeClient {
    client: reqwest::Client,
    allowed_origins: Vec<String>,
}

impl HttpRangeClient {
    pub fn from_policies(policies: &[PackageSourcePolicy]) -> Result<Self, PlatformError> {
        let allowed_origins = policies
            .iter()
            .find_map(|policy| match policy {
                PackageSourcePolicy::HttpsRange { allowed_origins } => {
                    Some(allowed_origins.clone())
                }
                _ => None,
            })
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::PermissionDenied,
                    "package.https.open",
                    "HTTPS package source is not declared by the profile",
                )
            })?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "package.https.open",
                    "HTTPS transport could not be initialized",
                )
            })?;
        Ok(Self {
            client,
            allowed_origins,
        })
    }

    pub async fn fetch_into_cache(
        &self,
        url: &str,
        expected_hash: &str,
        cache: &mut VerifiedPackageCache,
    ) -> Result<(), PlatformError> {
        validate_origin(url, &self.allowed_origins)?;
        let response = self.client.get(url).send().await.map_err(|_| io_error())?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(PlatformError::new(
                PlatformErrorCode::Io,
                "package.https.open",
                "HTTPS package server did not return a complete response",
            ));
        }
        if response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
        {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.https.open",
                "HTTPS package response uses unsupported content encoding",
            ));
        }
        let declared_length = response.content_length().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.https.open",
                "HTTPS package response must declare content length",
            )
        })?;
        if declared_length > cache.max_entry_bytes() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.https.open",
                "HTTPS package exceeds cache entry limit",
            ));
        }
        let mut staging = cache.begin_staging(expected_hash)?;
        let mut stream = response.bytes_stream();
        let mut received = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| io_error())?;
            received = received
                .checked_add(u64::try_from(chunk.len()).map_err(|_| io_error())?)
                .ok_or_else(io_error)?;
            if received > declared_length {
                return Err(PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "package.https.open",
                    "HTTPS package response exceeds declared content length",
                ));
            }
            staging.write(&chunk)?;
        }
        if received != declared_length {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.https.open",
                "HTTPS package response is truncated",
            ));
        }
        staging.commit()
    }
}

fn validate_origin(url: &str, allowed_origins: &[String]) -> Result<(), PlatformError> {
    let parsed = url::Url::parse(url).map_err(|_| invalid_origin())?;
    if parsed.scheme() != "https"
        || parsed.username() != ""
        || parsed.password().is_some()
        || !allowed_origins
            .iter()
            .any(|allowed| allowed == &parsed.origin().ascii_serialization())
    {
        return Err(invalid_origin());
    }
    Ok(())
}

fn invalid_origin() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::PermissionDenied,
        "package.https.open",
        "HTTPS package origin is not allowed by the profile",
    )
}

fn io_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        "package.https.open",
        "HTTPS package request failed",
    )
}
