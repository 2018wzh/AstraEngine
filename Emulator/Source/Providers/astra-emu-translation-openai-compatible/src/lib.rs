//! OpenAI-compatible translation service used by the independent AstraEMU host.
//!
//! The service owns only the network request and an optional in-memory session
//! cache. It does not know about a Family, a game save, or a rendered frame.
//! Dropping the future returned by [`OpenAiCompatibleTranslationProvider::translate`]
//! cancels the request; callers should do that when a game session exits.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const ECNU_BASE_URL: &str = "https://chat.ecnu.edu.cn/open/api/v1";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub const MAX_CONTEXT_SEGMENTS: usize = 8;
pub const MAX_CONTEXT_CHARS: usize = 6_000;
pub const DEFAULT_CACHE_ENTRIES: usize = 128;
pub const DEFAULT_CACHE_CHARS: usize = 256_000;
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_SECRET_BYTES: usize = 16 * 1024;

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
            endpoint: OPENAI_BASE_URL.into(),
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

    fn rendered(&self) -> String {
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
}

pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError>;
}

/// Platform credential-store adapter. Only an opaque reference is persisted by
/// AstraEMU; the credential itself stays in the platform keyring.
#[derive(Debug, Clone)]
#[cfg(not(target_os = "android"))]
pub struct PlatformSecretStore {
    service: String,
}

#[cfg(not(target_os = "android"))]
impl PlatformSecretStore {
    pub fn new(service: impl Into<String>) -> Result<Self, TranslationError> {
        let service = service.into();
        if service.is_empty() || service.len() > 128 || !service.is_ascii() {
            return Err(TranslationError::InvalidProfile("invalid keyring service"));
        }
        Ok(Self { service })
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        validate_secret_reference(reference)?;
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(TranslationError::SecretUnavailable);
        }
        self.entry(reference)?
            .set_password(secret)
            .map_err(|_| TranslationError::SecretUnavailable)
    }

    pub fn delete(&self, reference: &str) -> Result<(), TranslationError> {
        validate_secret_reference(reference)?;
        self.entry(reference)?
            .delete_credential()
            .map_err(|_| TranslationError::SecretUnavailable)
    }

    fn entry(&self, reference: &str) -> Result<keyring::Entry, TranslationError> {
        keyring::Entry::new(&self.service, reference)
            .map_err(|_| TranslationError::SecretUnavailable)
    }
}

#[cfg(not(target_os = "android"))]
impl SecretResolver for PlatformSecretStore {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        validate_secret_reference(reference)?;
        let secret = self
            .entry(reference)?
            .get_password()
            .map_err(|_| TranslationError::SecretUnavailable)?;
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(TranslationError::SecretUnavailable);
        }
        Ok(secret)
    }
}

fn validate_identifier(name: &'static str, value: &str) -> Result<(), TranslationError> {
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

fn validate_secret_reference(reference: &str) -> Result<(), TranslationError> {
    validate_identifier("secret reference", reference)
}

pub struct OpenAiCompatibleTranslationProvider<R: ?Sized> {
    client: reqwest::Client,
    profile: TranslationProfile,
    secrets: Arc<R>,
}

impl<R: SecretResolver + ?Sized> OpenAiCompatibleTranslationProvider<R> {
    pub fn new(profile: TranslationProfile, secrets: Arc<R>) -> Result<Self, TranslationError> {
        profile.validate()?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_millis(profile.timeout_ms))
            .user_agent("AstraEMU/0.1")
            .build()
            .map_err(|_| TranslationError::Transport)?;
        Ok(Self {
            client,
            profile,
            secrets,
        })
    }

    pub fn profile(&self) -> &TranslationProfile {
        &self.profile
    }

    pub fn provider_identity(&self) -> String {
        self.profile.provider_identity()
    }

    /// Translate one displayed segment. The request future is cancellation-safe:
    /// dropping it cancels the underlying HTTP request and no cache entry is made.
    pub async fn translate(
        &self,
        request: &TranslationRequest,
    ) -> Result<TranslationResult, TranslationError> {
        request.validate()?;
        let prompt = build_prompt(&self.profile, request)?;
        let secret = self.secrets.resolve(&self.profile.secret_reference)?;
        let started = Instant::now();
        let response = self.send_translation(&secret, &prompt).await?;
        let translated = response.trim().to_owned();
        if translated.is_empty() {
            return Err(TranslationError::Protocol(
                "provider returned empty translation",
            ));
        }
        if translated.chars().count() > MAX_CONTEXT_CHARS {
            return Err(TranslationError::Protocol(
                "translation exceeds bounded output",
            ));
        }
        let result = TranslationResult {
            translated,
            provider_identity: self.provider_identity(),
            latency_ms: elapsed_ms(started.elapsed()),
            sent_segment_count: prompt_segment_count(request),
            cache_hit: false,
        };
        tracing::info!(
            event = "astra.emu.translation.completed",
            provider = %result.provider_identity,
            segment_count = result.sent_segment_count,
            latency_ms = result.latency_ms,
        );
        Ok(result)
    }

    /// Authenticate the configured endpoint without sending game text.
    pub async fn test_connection(&self) -> Result<ConnectionTestResult, TranslationError> {
        let secret = self.secrets.resolve(&self.profile.secret_reference)?;
        let endpoint = self.profile.endpoint.trim_end_matches('/');
        let started = Instant::now();
        let response = self
            .client
            .get(format!("{endpoint}/models"))
            .bearer_auth(secret)
            .send()
            .await
            .map_err(map_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TranslationError::HttpStatus(status.as_u16()));
        }
        Ok(ConnectionTestResult {
            provider_identity: self.provider_identity(),
            latency_ms: elapsed_ms(started.elapsed()),
        })
    }

    async fn send_translation(
        &self,
        secret: &str,
        prompt: &str,
    ) -> Result<String, TranslationError> {
        let endpoint = self.profile.endpoint.trim_end_matches('/');
        let (url, body) = match self.profile.protocol {
            TranslationProtocol::Responses => (
                format!("{endpoint}/responses"),
                serde_json::json!({
                    "model": self.profile.model,
                    "input": prompt,
                    "stream": false,
                }),
            ),
            TranslationProtocol::ChatCompletions => (
                format!("{endpoint}/chat/completions"),
                serde_json::json!({
                    "model": self.profile.model,
                    "messages": [{"role": "user", "content": prompt}],
                    "stream": false,
                }),
            ),
        };
        let response = self
            .client
            .post(url)
            .bearer_auth(secret)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TranslationError::HttpStatus(status.as_u16()));
        }
        let bytes = response.bytes().await.map_err(map_reqwest)?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(TranslationError::Protocol("provider response is too large"));
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| TranslationError::Protocol("provider response is not JSON"))?;
        match self.profile.protocol {
            TranslationProtocol::Responses => parse_responses_output(&value),
            TranslationProtocol::ChatCompletions => parse_chat_output(&value),
        }
    }
}

fn map_reqwest(error: reqwest::Error) -> TranslationError {
    if error.is_timeout() {
        TranslationError::Timeout
    } else {
        TranslationError::Transport
    }
}

fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn prompt_segment_count(request: &TranslationRequest) -> usize {
    let mut remaining =
        MAX_CONTEXT_CHARS.saturating_sub(request.current.rendered().chars().count());
    let mut count = 1;
    for segment in request.recent.iter().rev().take(MAX_CONTEXT_SEGMENTS) {
        let chars = segment.rendered().chars().count();
        if chars <= remaining {
            remaining -= chars;
            count += 1;
        }
    }
    count
}

pub fn build_prompt(
    profile: &TranslationProfile,
    request: &TranslationRequest,
) -> Result<String, TranslationError> {
    profile.validate()?;
    request.validate()?;

    let current = request.current.rendered();
    let mut selected = Vec::new();
    let mut remaining = MAX_CONTEXT_CHARS.saturating_sub(current.chars().count());
    for segment in request.recent.iter().rev().take(MAX_CONTEXT_SEGMENTS) {
        let rendered = segment.rendered();
        let chars = rendered.chars().count();
        if chars <= remaining {
            remaining -= chars;
            selected.push(rendered);
        }
    }
    selected.reverse();

    let mut prompt = format!(
        "Translate the CURRENT segment into {}. Preserve names, markup, ruby, and line breaks. Return only the translation.\n",
        profile.target_language
    );
    if !selected.is_empty() {
        prompt.push_str("CONTEXT:\n");
        for segment in selected {
            prompt.push_str(&segment);
            prompt.push('\n');
        }
    }
    prompt.push_str("CURRENT:\n");
    prompt.push_str(&current);
    if prompt.chars().count() > MAX_CONTEXT_CHARS + 256 {
        return Err(TranslationError::ContextLimit);
    }
    Ok(prompt)
}

fn parse_responses_output(value: &Value) -> Result<String, TranslationError> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Ok(text.to_owned());
    }
    value
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("content")
                    .and_then(Value::as_array)
                    .and_then(|parts| {
                        parts.iter().find_map(|part| {
                            part.get("text").and_then(Value::as_str).map(str::to_owned)
                        })
                    })
            })
        })
        .ok_or(TranslationError::Protocol(
            "Responses response omitted output text",
        ))
}

fn parse_chat_output(value: &Value) -> Result<String, TranslationError> {
    let content = value
        .pointer("/choices/0/message/content")
        .ok_or(TranslationError::Protocol(
            "Chat Completions response omitted message content",
        ))?;
    if let Some(text) = content.as_str() {
        return Ok(text.to_owned());
    }
    content
        .as_array()
        .and_then(|parts| {
            parts
                .iter()
                .find_map(|part| part.get("text").and_then(Value::as_str).map(str::to_owned))
        })
        .ok_or(TranslationError::Protocol(
            "Chat Completions content has no text",
        ))
}

#[derive(Debug, Clone)]
struct CacheEntry {
    current_text: String,
    translated: String,
}

/// FIFO cache scoped to one live game session. It intentionally has no
/// serialization or disk interface.
#[derive(Debug)]
pub struct TranslationSessionCache {
    entries: VecDeque<CacheEntry>,
    chars: usize,
    max_entries: usize,
    max_chars: usize,
}

impl Default for TranslationSessionCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_ENTRIES, DEFAULT_CACHE_CHARS)
    }
}

impl TranslationSessionCache {
    pub fn new(max_entries: usize, max_chars: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            chars: 0,
            max_entries,
            max_chars,
        }
    }

    pub fn get(&self, current_text: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.current_text == current_text)
            .map(|entry| entry.translated.as_str())
    }

    pub fn insert(&mut self, current_text: String, translated: String) {
        let entry_chars = translated.chars().count();
        if self.max_entries == 0 || entry_chars > self.max_chars {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.current_text == current_text)
        {
            if let Some(old) = self.entries.remove(index) {
                self.chars = self.chars.saturating_sub(old.translated.chars().count());
            }
        }
        while self.entries.len() >= self.max_entries
            || self.chars.saturating_add(entry_chars) > self.max_chars
        {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.chars = self.chars.saturating_sub(old.translated.chars().count());
        }
        self.chars = self.chars.saturating_add(entry_chars);
        self.entries.push_back(CacheEntry {
            current_text,
            translated,
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.chars = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Provider plus the bounded, non-persistent cache for one active game.
pub struct TranslationSession<R: ?Sized> {
    provider: Arc<OpenAiCompatibleTranslationProvider<R>>,
    cache: Mutex<TranslationSessionCache>,
}

impl<R: SecretResolver + ?Sized> TranslationSession<R> {
    pub fn new(provider: Arc<OpenAiCompatibleTranslationProvider<R>>) -> Self {
        Self {
            provider,
            cache: Mutex::new(TranslationSessionCache::default()),
        }
    }

    pub async fn translate(
        &self,
        request: &TranslationRequest,
    ) -> Result<TranslationResult, TranslationError> {
        if let Some(translated) = self
            .cache
            .lock()
            .expect("translation cache mutex poisoned")
            .get(&request.current.text)
            .map(str::to_owned)
        {
            return Ok(TranslationResult {
                translated,
                provider_identity: self.provider.provider_identity(),
                latency_ms: 0,
                sent_segment_count: prompt_segment_count(request),
                cache_hit: true,
            });
        }
        let result = self.provider.translate(request).await?;
        self.cache
            .lock()
            .expect("translation cache mutex poisoned")
            .insert(request.current.text.clone(), result.translated.clone());
        Ok(result)
    }

    pub fn clear(&self) {
        self.cache
            .lock()
            .expect("translation cache mutex poisoned")
            .clear();
    }

    pub fn clear_for_new_game(&self) {
        self.clear();
    }

    pub fn clear_for_load(&self) {
        self.clear();
    }

    pub fn clear_for_configuration_change(&self) {
        self.clear();
    }

    pub fn cache_len(&self) -> usize {
        self.cache
            .lock()
            .expect("translation cache mutex poisoned")
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestSecrets;

    impl SecretResolver for TestSecrets {
        fn resolve(&self, _reference: &str) -> Result<String, TranslationError> {
            Ok("secret".into())
        }
    }

    fn profile() -> TranslationProfile {
        TranslationProfile {
            profile_id: "test".into(),
            endpoint_kind: TranslationEndpointKind::OpenAiCompatible,
            endpoint: "https://example.com/v1".into(),
            protocol: TranslationProtocol::Responses,
            model: "model".into(),
            target_language: "zh-CN".into(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            secret_reference: "test.secret".into(),
        }
    }

    #[test]
    fn profile_defaults_to_fifteen_second_timeout_and_validates() {
        assert_eq!(TranslationProfile::default().timeout_ms, 15_000);
        profile().validate().unwrap();
    }

    #[test]
    fn request_keeps_newest_eight_segments_and_optional_metadata() {
        let mut request = TranslationRequest {
            current: TranslationSegment {
                text: "现在".into(),
                speaker: Some("Alice".into()),
                ruby: Some("いま".into()),
            },
            recent: (0..12)
                .map(|index| TranslationSegment::new(format!("segment-{index}")))
                .collect(),
        };
        let prompt = build_prompt(&profile(), &request).unwrap();
        assert!(!prompt.contains("segment-0"));
        assert!(!prompt.contains("segment-3"));
        assert!(prompt.contains("segment-4"));
        assert!(prompt.contains("segment-11"));
        assert!(prompt.contains("speaker=Alice, ruby=いま"));
        assert!(prompt.chars().count() <= MAX_CONTEXT_CHARS + 256);

        request.current.text = "x".repeat(MAX_CONTEXT_CHARS + 1);
        assert!(matches!(
            request.validate(),
            Err(TranslationError::ContextLimit)
        ));
    }

    #[test]
    fn cache_is_bounded_and_clears_without_serialization() {
        let mut cache = TranslationSessionCache::new(2, 5);
        let request_a = TranslationRequest::plain("a", Vec::<String>::new());
        let request_b = TranslationRequest::plain("b", Vec::<String>::new());
        let request_c = TranslationRequest::plain("c", Vec::<String>::new());
        cache.insert(request_a.current.text.clone(), "aa".into());
        cache.insert(request_b.current.text.clone(), "bb".into());
        cache.insert(request_c.current.text.clone(), "cc".into());
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&request_a.current.text).is_none());
        assert_eq!(cache.get(&request_c.current.text), Some("cc"));
        let same_text_with_other_context = TranslationRequest {
            current: request_c.current.clone(),
            recent: vec![TranslationSegment::new("different context")],
        };
        assert_eq!(
            cache.get(&same_text_with_other_context.current.text),
            Some("cc")
        );
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn responses_and_chat_payloads_are_parsed_without_body_logging() {
        let responses = serde_json::json!({"output_text":"你好"});
        assert_eq!(parse_responses_output(&responses).unwrap(), "你好");
        let responses_nested =
            serde_json::json!({"output":[{"content":[{"type":"output_text","text":"你好"}]}]});
        assert_eq!(parse_responses_output(&responses_nested).unwrap(), "你好");
        let chat = serde_json::json!({"choices":[{"message":{"content":"你好"}}]});
        assert_eq!(parse_chat_output(&chat).unwrap(), "你好");
        let malformed = serde_json::json!({"error":{"message":"do not expose"}});
        let error = parse_chat_output(&malformed).unwrap_err();
        assert!(!error.to_string().contains("do not expose"));
    }

    #[test]
    fn provider_constructs_with_secret_resolver() {
        let provider =
            OpenAiCompatibleTranslationProvider::new(profile(), Arc::new(TestSecrets)).unwrap();
        assert_eq!(provider.profile().timeout_ms, DEFAULT_TIMEOUT_MS);
    }
}
