use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DEFAULT_CONTEXT_CHARS, DEFAULT_CONTEXT_SEGMENTS, DEFAULT_TIMEOUT_MS, MAX_CONTEXT_CHARS,
    MAX_CONTEXT_SEGMENTS, MAX_CUSTOM_PROMPT_CHARS, MIN_CONTEXT_CHARS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TranslationProvider {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "google")]
    Google,
}

impl TranslationProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Google => "https://generativelanguage.googleapis.com/v1beta",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApi {
    Responses,
    ChatCompletions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TranslationProfile {
    pub profile_id: String,
    pub provider: TranslationProvider,
    pub base_url: String,
    pub openai_api: OpenAiApi,
    pub model: String,
    pub target_language: String,
    pub timeout_ms: u64,
    pub secret_reference: String,
    pub custom_prompt: String,
    pub context_segments: u32,
    pub context_chars: u32,
}

impl Default for TranslationProfile {
    fn default() -> Self {
        Self {
            profile_id: "translation.default".into(),
            provider: TranslationProvider::OpenAi,
            base_url: TranslationProvider::OpenAi.default_base_url().into(),
            openai_api: OpenAiApi::Responses,
            model: String::new(),
            target_language: "zh-CN".into(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            secret_reference: "translation.default".into(),
            custom_prompt: String::new(),
            context_segments: DEFAULT_CONTEXT_SEGMENTS,
            context_chars: DEFAULT_CONTEXT_CHARS,
        }
    }
}

impl TranslationProfile {
    pub fn validate(&self) -> Result<(), TranslationError> {
        validate_identifier("profile_id", &self.profile_id)?;
        validate_secret_reference(&self.secret_reference)?;
        validate_base_url(&self.base_url)?;
        if self.model.trim().is_empty()
            || self.model.chars().count() > 256
            || self.model.chars().any(char::is_control)
        {
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
        if self.custom_prompt.chars().count() > MAX_CUSTOM_PROMPT_CHARS {
            return Err(TranslationError::InvalidProfile(
                "custom_prompt must contain at most 2000 characters",
            ));
        }
        if self.context_segments as usize > MAX_CONTEXT_SEGMENTS {
            return Err(TranslationError::InvalidProfile(
                "context_segments must be within 0..=16",
            ));
        }
        if !(MIN_CONTEXT_CHARS..=MAX_CONTEXT_CHARS).contains(&(self.context_chars as usize)) {
            return Err(TranslationError::InvalidProfile(
                "context_chars must be within 1000..=12000",
            ));
        }
        Ok(())
    }

    pub fn provider_identity(&self) -> String {
        format!("{}:{}", self.provider.as_str(), self.profile_id)
    }
}

fn validate_base_url(base_url: &str) -> Result<(), TranslationError> {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.len() > 1_024 {
        return Err(TranslationError::InvalidProfile("base_url is too long"));
    }
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| TranslationError::InvalidProfile("base_url is not a URL"))?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let scheme_allowed = match parsed.scheme() {
        "https" => true,
        "http" => loopback,
        _ => false,
    };
    if !scheme_allowed
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(TranslationError::InvalidProfile(
            "base_url must be HTTPS (or HTTP on loopback) without credentials, query, or fragment",
        ));
    }
    Ok(())
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
    pub models: Vec<String>,
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
