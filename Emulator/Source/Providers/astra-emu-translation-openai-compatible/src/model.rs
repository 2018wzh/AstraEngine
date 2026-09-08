use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DEFAULT_TIMEOUT_MS, MAX_CONTEXT_CHARS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TranslationEndpointKind {
    OpenAiCompatible,
    OpenAi,
    Ecnu,
    ThirdParty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TranslationProtocol {
    Responses,
    ChatCompletions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TranslationProfile {
    pub profile_id: String,
    pub endpoint_kind: TranslationEndpointKind,
    pub endpoint: String,
    pub protocol: TranslationProtocol,
    pub model: String,
    pub target_language: String,
    pub timeout_ms: u64,
    pub secret_reference: String,
}

impl Default for TranslationProfile {
    fn default() -> Self {
        Self {
            profile_id: "translation.default".into(),
            endpoint_kind: TranslationEndpointKind::OpenAiCompatible,
            endpoint: crate::OPENAI_BASE_URL.into(),
            protocol: TranslationProtocol::Responses,
            model: String::new(),
            target_language: "zh-CN".into(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            secret_reference: "translation.default".into(),
        }
    }
}

impl TranslationProfile {
    pub fn validate(&self) -> Result<(), TranslationError> {
        validate_identifier("profile_id", &self.profile_id)?;
        validate_secret_reference(&self.secret_reference)?;

        let endpoint = self.endpoint.trim_end_matches('/');
        let parsed = reqwest::Url::parse(endpoint)
            .map_err(|_| TranslationError::InvalidProfile("endpoint is not a URL"))?;
        if parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(TranslationError::InvalidProfile(
                "endpoint must be an HTTPS base URL without credentials, query, or fragment",
            ));
        }
        if endpoint.len() > 1_024 {
            return Err(TranslationError::InvalidProfile("endpoint is too long"));
        }
        if self.model.trim().is_empty() || self.model.chars().count() > 256 {
            return Err(TranslationError::InvalidProfile(
                "model must contain 1..=256 characters",
            ));
        }
        if self.target_language.trim().is_empty() || self.target_language.chars().count() > 32 {
            return Err(TranslationError::InvalidProfile(
                "target language must contain 1..=32 characters",
            ));
        }
        if !(1_000..=120_000).contains(&self.timeout_ms) {
            return Err(TranslationError::InvalidProfile(
                "timeout_ms must be within 1000..=120000",
            ));
        }
        Ok(())
    }

    pub fn provider_identity(&self) -> String {
        let kind = match self.endpoint_kind {
            TranslationEndpointKind::OpenAiCompatible => "openai-compatible",
            TranslationEndpointKind::OpenAi => "openai",
            TranslationEndpointKind::Ecnu => "ecnu",
            TranslationEndpointKind::ThirdParty => "third-party",
        };
        format!("{kind}:{}", self.profile_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranslationSegment {
    pub text: String,
    pub speaker: Option<String>,
    pub ruby: Option<String>,
}

impl TranslationSegment {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            speaker: None,
            ruby: None,
        }
    }

    pub(crate) fn rendered(&self) -> String {
        let mut prefix = String::new();
        if let Some(speaker) = self.speaker.as_deref().filter(|value| !value.is_empty()) {
            prefix.push_str("speaker=");
            prefix.push_str(speaker);
        }
        if let Some(ruby) = self.ruby.as_deref().filter(|value| !value.is_empty()) {
            if !prefix.is_empty() {
                prefix.push_str(", ");
            }
            prefix.push_str("ruby=");
            prefix.push_str(ruby);
        }
        if prefix.is_empty() {
            self.text.clone()
        } else {
            format!("[{prefix}] {}", self.text)
        }
    }

    fn validate(&self, name: &'static str) -> Result<(), TranslationError> {
        if self.text.trim().is_empty() {
            return Err(TranslationError::InvalidRequest(name));
        }
        if self.text.chars().count() > MAX_CONTEXT_CHARS {
            return Err(TranslationError::ContextLimit);
        }
        for value in [self.speaker.as_deref(), self.ruby.as_deref()]
            .into_iter()
            .flatten()
        {
            if value.chars().count() > 256 {
                return Err(TranslationError::InvalidRequest(name));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranslationRequest {
    pub current: TranslationSegment,
    pub recent: Vec<TranslationSegment>,
}

impl TranslationRequest {
    pub fn plain(current: impl Into<String>, recent: impl IntoIterator<Item = String>) -> Self {
        Self {
            current: TranslationSegment::new(current),
            recent: recent.into_iter().map(TranslationSegment::new).collect(),
        }
    }

    pub fn validate(&self) -> Result<(), TranslationError> {
        self.current.validate("current text")?;
        if self.current.rendered().chars().count() > MAX_CONTEXT_CHARS {
            return Err(TranslationError::ContextLimit);
        }
        for segment in &self.recent {
            segment.validate("context text")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationResult {
    pub translated: String,
    pub provider_identity: String,
    pub latency_ms: u64,
    pub sent_segment_count: usize,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionTestResult {
    pub provider_identity: String,
    pub latency_ms: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TranslationError {
    #[error("ASTRA_EMU_TRANSLATION_PROFILE: {0}")]
    InvalidProfile(&'static str),
    #[error("ASTRA_EMU_TRANSLATION_REQUEST: {0}")]
    InvalidRequest(&'static str),
    #[error("ASTRA_EMU_TRANSLATION_CONTEXT_LIMIT")]
    ContextLimit,
    #[error("ASTRA_EMU_TRANSLATION_SECRET_UNAVAILABLE")]
    SecretUnavailable,
    #[error("ASTRA_EMU_TRANSLATION_TIMEOUT")]
    Timeout,
    #[error("ASTRA_EMU_TRANSLATION_HTTP_{0}")]
    HttpStatus(u16),
    #[error("ASTRA_EMU_TRANSLATION_PROTOCOL: {0}")]
    Protocol(&'static str),
    #[error("ASTRA_EMU_TRANSLATION_TRANSPORT")]
    Transport,
    #[error("ASTRA_EMU_TRANSLATION_SESSION_RESET")]
    SessionReset,
}

pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError>;
}

pub(crate) fn validate_identifier(name: &'static str, value: &str) -> Result<(), TranslationError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(TranslationError::InvalidProfile(name));
    }
    Ok(())
}

pub(crate) fn validate_secret_reference(reference: &str) -> Result<(), TranslationError> {
    validate_identifier("secret reference", reference)
}
