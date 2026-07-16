use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

pub const ECNU_BASE_URL: &str = "https://chat.ecnu.edu.cn/open/api/v1";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TranslationEndpointKind {
    Ecnu,
    OpenAi,
    ThirdParty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TranslationProtocol {
    Responses,
    ChatCompletions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TranslationProfile {
    pub profile_id: String,
    pub endpoint_kind: TranslationEndpointKind,
    pub endpoint: String,
    pub protocol: TranslationProtocol,
    pub model: String,
    pub target_language: String,
    pub context_sentences: u8,
    pub body_limit_bytes: u32,
    pub timeout_ms: u64,
    pub secret_reference: String,
}

impl TranslationProfile {
    pub fn validate(&self) -> Result<(), TranslationError> {
        validate_secret_reference(&self.profile_id)?;
        let endpoint = self.endpoint.trim_end_matches('/');
        let parsed = reqwest::Url::parse(endpoint)
            .map_err(|_| TranslationError::Profile("endpoint is not a valid URL".into()))?;
        if parsed.scheme() != "https"
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(TranslationError::Profile(
                "endpoint must be an HTTPS API base URL without credentials, query, or fragment"
                    .into(),
            ));
        }
        match self.endpoint_kind {
            TranslationEndpointKind::Ecnu if endpoint != ECNU_BASE_URL => {
                return Err(TranslationError::Profile(
                    "ECNU endpoint must exactly match the configured API base URL".into(),
                ))
            }
            TranslationEndpointKind::OpenAi
                if endpoint != OPENAI_BASE_URL
                    || self.protocol != TranslationProtocol::Responses =>
            {
                return Err(TranslationError::Profile(
                    "official OpenAI profiles must use the Responses API".into(),
                ))
            }
            TranslationEndpointKind::ThirdParty
                if self.protocol != TranslationProtocol::ChatCompletions =>
            {
                return Err(TranslationError::Profile(
                    "third-party endpoints must explicitly use Chat Completions".into(),
                ))
            }
            _ => {}
        }
        if self.model.trim().is_empty() {
            return Err(TranslationError::Profile("model is required".into()));
        }
        if self.target_language.trim().is_empty() {
            return Err(TranslationError::Profile(
                "target language is required".into(),
            ));
        }
        if self.context_sentences > 32 {
            return Err(TranslationError::Profile(
                "context_sentences must be within 0..=32".into(),
            ));
        }
        if self.body_limit_bytes == 0 || self.body_limit_bytes > 16 * 1024 {
            return Err(TranslationError::Profile(
                "body_limit_bytes must be within 1..=16384".into(),
            ));
        }
        if !(1_000..=120_000).contains(&self.timeout_ms) {
            return Err(TranslationError::Profile(
                "timeout_ms must be within 1000..=120000".into(),
            ));
        }
        if self.secret_reference.trim().is_empty() {
            return Err(TranslationError::Profile(
                "secret_reference is required".into(),
            ));
        }
        Ok(())
    }

    pub fn provider_identity(&self) -> String {
        let kind = match self.endpoint_kind {
            TranslationEndpointKind::Ecnu => "ecnu-openai-compatible",
            TranslationEndpointKind::OpenAi => "openai",
            TranslationEndpointKind::ThirdParty => "third-party-openai-compatible",
        };
        format!("{kind}:{}", self.profile_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationRequest {
    pub current: String,
    pub recent: VecDeque<String>,
    pub background: Option<String>,
    pub glossary: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationResult {
    pub translated: String,
    pub provider_identity: String,
    pub latency_ms: u64,
    pub sent_sentence_count: usize,
}

#[derive(Debug, Error)]
pub enum TranslationError {
    #[error("ASTRA_EMU_TRANSLATION_PROFILE: {0}")]
    Profile(String),
    #[error("ASTRA_EMU_TRANSLATION_SECRET_UNAVAILABLE")]
    SecretUnavailable,
    #[error("ASTRA_EMU_TRANSLATION_CIRCUIT_OPEN")]
    CircuitOpen,
    #[error("ASTRA_EMU_TRANSLATION_TIMEOUT")]
    Timeout,
    #[error("ASTRA_EMU_TRANSLATION_RATE_LIMITED")]
    RateLimited,
    #[error("ASTRA_EMU_TRANSLATION_HTTP_{status}: {code}")]
    Http { status: u16, code: String },
    #[error("ASTRA_EMU_TRANSLATION_PROTOCOL: {0}")]
    Protocol(String),
    #[error("ASTRA_EMU_TRANSLATION_TRANSPORT: {0}")]
    Transport(String),
}

pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError>;
}

/// Platform credential-store adapter. Only opaque references are persisted by AstraEMU; the
/// credential value remains in Windows Credential Manager, macOS Keychain, iOS Keychain, Android
/// credential storage, or the desktop Secret Service selected by `keyring`.
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
            return Err(TranslationError::Profile(
                "secret store service identity is invalid".into(),
            ));
        }
        Ok(Self { service })
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        validate_secret_reference(reference)?;
        if secret.is_empty() || secret.len() > 16 * 1024 {
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
        self.entry(reference)?
            .get_password()
            .map_err(|_| TranslationError::SecretUnavailable)
    }
}

fn validate_secret_reference(reference: &str) -> Result<(), TranslationError> {
    if reference.is_empty()
        || reference.len() > 128
        || !reference
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(TranslationError::Profile(
            "secret_reference must be a bounded safe identifier".into(),
        ));
    }
    Ok(())
}

#[derive(Default)]
struct CircuitState {
    consecutive_failures: u8,
    opened: bool,
}

impl CircuitState {
    fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= 3 {
            self.opened = true;
        }
    }
}

pub struct OpenAiCompatibleTranslationProvider<R: ?Sized> {
    client: reqwest::Client,
    profile: TranslationProfile,
    secrets: Arc<R>,
    serial: Semaphore,
    circuit: Mutex<CircuitState>,
}

impl<R: SecretResolver + ?Sized> OpenAiCompatibleTranslationProvider<R> {
    pub fn new(profile: TranslationProfile, secrets: Arc<R>) -> Result<Self, TranslationError> {
        profile.validate()?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_millis(profile.timeout_ms))
            .user_agent("AstraEMU/0.1")
            .build()
            .map_err(|error| TranslationError::Transport(error.to_string()))?;
        Ok(Self {
            client,
            profile,
            secrets,
            serial: Semaphore::new(1),
            circuit: Mutex::new(CircuitState::default()),
        })
    }

    pub async fn translate(
        &self,
        request: &TranslationRequest,
    ) -> Result<TranslationResult, TranslationError> {
        if self.circuit.lock().await.opened {
            tracing::warn!(
                event = "astra.emu.translation.circuit_blocked",
                protocol = ?self.profile.protocol
            );
            return Err(TranslationError::CircuitOpen);
        }
        let _permit = self
            .serial
            .acquire()
            .await
            .map_err(|_| TranslationError::CircuitOpen)?;
        let secret = self.secrets.resolve(&self.profile.secret_reference)?;
        let (prompt, sent_sentence_count) = build_prompt(&self.profile, request)?;
        let started = Instant::now();
        let mut attempt = 0_usize;
        let result = loop {
            let result = match self.profile.protocol {
                TranslationProtocol::Responses => self.responses(&secret, &prompt).await,
                TranslationProtocol::ChatCompletions => {
                    self.chat_completions(&secret, &prompt).await
                }
            };
            if result.as_ref().is_err_and(retryable) && attempt < 2 {
                let delay = [Duration::from_millis(250), Duration::from_millis(750)][attempt];
                attempt += 1;
                tokio::time::sleep(delay).await;
                continue;
            }
            break result;
        };
        match result {
            Ok(translated) => {
                self.circuit.lock().await.record_success();
                tracing::info!(
                    event = "astra.emu.translation.completed",
                    protocol = ?self.profile.protocol,
                    sentence_count = sent_sentence_count,
                    latency_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
                );
                Ok(TranslationResult {
                    translated,
                    provider_identity: self.provider_identity(),
                    latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                    sent_sentence_count,
                })
            }
            Err(error) => {
                let mut circuit = self.circuit.lock().await;
                circuit.record_failure();
                tracing::warn!(
                    event = "astra.emu.translation.failed",
                    protocol = ?self.profile.protocol,
                    retryable = retryable(&error),
                    circuit_open = circuit.opened
                );
                Err(error)
            }
        }
    }

    pub async fn reset_circuit_by_user(&self) {
        *self.circuit.lock().await = CircuitState::default();
        tracing::info!(event = "astra.emu.translation.circuit_reset_by_user");
    }

    pub fn profile(&self) -> &TranslationProfile {
        &self.profile
    }

    pub fn provider_identity(&self) -> String {
        self.profile.provider_identity()
    }

    async fn responses(&self, secret: &str, prompt: &str) -> Result<String, TranslationError> {
        let url = format!("{}/responses", self.profile.endpoint.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .bearer_auth(secret)
            .json(&json!({
                "model": self.profile.model,
                "input": prompt,
                "stream": true
            }))
            .send()
            .await
            .map_err(map_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.json::<Value>().await.ok();
            return validate_status(status, body.as_ref()).and(Err(TranslationError::Protocol(
                "unreachable status validation state".into(),
            )));
        }
        let mut bytes = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut output = String::new();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(map_reqwest)?;
            buffer.extend_from_slice(&chunk);
            drain_sse_frames(&mut buffer, &mut output)?;
        }
        if !buffer.iter().all(u8::is_ascii_whitespace) {
            let frame = std::str::from_utf8(&buffer)
                .map_err(|_| TranslationError::Protocol("SSE frame was not UTF-8".into()))?;
            parse_sse_frame(frame, &mut output)?;
        }
        if output.is_empty() {
            return Err(TranslationError::Protocol(
                "Responses stream completed without output_text delta".into(),
            ));
        }
        Ok(output)
    }

    async fn chat_completions(
        &self,
        secret: &str,
        prompt: &str,
    ) -> Result<String, TranslationError> {
        let url = format!(
            "{}/chat/completions",
            self.profile.endpoint.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .bearer_auth(secret)
            .json(&json!({
                "model": self.profile.model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": false
            }))
            .send()
            .await
            .map_err(map_reqwest)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(map_reqwest)?;
        validate_status(status, Some(&body))?;
        body.pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                TranslationError::Protocol(
                    "Chat Completions response omitted choices[0].message.content".into(),
                )
            })
    }
}

fn retryable(error: &TranslationError) -> bool {
    matches!(
        error,
        TranslationError::Timeout
            | TranslationError::RateLimited
            | TranslationError::Transport(_)
            | TranslationError::Http {
                status: 500..=599,
                ..
            }
    )
}

fn build_prompt(
    profile: &TranslationProfile,
    request: &TranslationRequest,
) -> Result<(String, usize), TranslationError> {
    if request.current.is_empty() {
        return Err(TranslationError::Profile(
            "current sentence is required".into(),
        ));
    }
    let instruction = format!(
        "Translate the CURRENT sentence into {}. Preserve names, markup, and line breaks. Return only the translation.\n",
        profile.target_language
    );
    let current = format!("CURRENT:\n{}", request.current);
    let limit = profile.body_limit_bytes as usize;
    if instruction.len() + current.len() > limit {
        return Err(TranslationError::Profile(
            "current sentence exceeds body limit".into(),
        ));
    }

    let mut optional = String::new();
    let mut remaining = limit - instruction.len() - current.len();
    let glossary = request
        .glossary
        .iter()
        .map(|(source, target)| format!("{source} => {target}"))
        .collect::<Vec<_>>();
    append_prefix_section("GLOSSARY", &glossary, &mut optional, &mut remaining);

    let background = request
        .background
        .as_deref()
        .map(split_sentences)
        .unwrap_or_default();
    append_prefix_section("BACKGROUND", &background, &mut optional, &mut remaining);

    let recent = request
        .recent
        .iter()
        .rev()
        .take(profile.context_sentences as usize)
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    let sent_sentence_count =
        append_suffix_section("CONTEXT", &recent, &mut optional, &mut remaining);

    let mut prompt = instruction;
    prompt.push_str(&optional);
    prompt.push_str(&current);
    if prompt.len() > limit {
        return Err(TranslationError::Protocol(
            "deterministic prompt budgeting exceeded its declared bound".into(),
        ));
    }
    Ok((prompt, sent_sentence_count))
}

fn append_prefix_section(
    heading: &str,
    lines: &[String],
    output: &mut String,
    remaining: &mut usize,
) -> usize {
    let heading = format!("{heading}:\n");
    if lines.is_empty() || heading.len() > *remaining {
        return 0;
    }
    let mut selected = Vec::new();
    let mut used = heading.len();
    for line in lines {
        let cost = line.len() + 1;
        if used + cost > *remaining {
            break;
        }
        selected.push(line);
        used += cost;
    }
    if selected.is_empty() {
        return 0;
    }
    output.push_str(&heading);
    for line in &selected {
        output.push_str(line);
        output.push('\n');
    }
    *remaining -= used;
    selected.len()
}

fn append_suffix_section(
    heading: &str,
    lines: &[String],
    output: &mut String,
    remaining: &mut usize,
) -> usize {
    let heading_text = format!("{heading}:\n");
    if lines.is_empty() || heading_text.len() > *remaining {
        return 0;
    }
    let mut used = heading_text.len();
    let mut first = lines.len();
    for (index, line) in lines.iter().enumerate().rev() {
        let cost = line.len() + 1;
        if used + cost > *remaining {
            break;
        }
        first = index;
        used += cost;
    }
    if first == lines.len() {
        return 0;
    }
    output.push_str(&heading_text);
    for line in &lines[first..] {
        output.push_str(line);
        output.push('\n');
    }
    *remaining -= used;
    lines.len() - first
}

fn split_sentences(input: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for ch in input.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '。' | '！' | '？' | '\n') {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_owned());
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        sentences.push(trimmed.to_owned());
    }
    sentences
}

fn parse_sse_frame(frame: &str, output: &mut String) -> Result<(), TranslationError> {
    let mut event = None;
    let mut data = String::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim());
        }
        if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim());
        }
    }
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let value: Value = serde_json::from_str(&data)
        .map_err(|error| TranslationError::Protocol(format!("invalid SSE JSON: {error}")))?;
    let kind = event
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");
    match kind {
        "response.output_text.delta" => {
            let delta = value.get("delta").and_then(Value::as_str).ok_or_else(|| {
                TranslationError::Protocol("output_text delta event omitted delta".into())
            })?;
            output.push_str(delta);
        }
        "error" | "response.failed" => {
            return Err(TranslationError::Protocol(
                "provider returned a failed Responses event".into(),
            ))
        }
        _ => {}
    }
    Ok(())
}

fn drain_sse_frames(buffer: &mut Vec<u8>, output: &mut String) -> Result<(), TranslationError> {
    while let Some((boundary, delimiter_len)) = find_sse_boundary(buffer) {
        let frame = std::str::from_utf8(&buffer[..boundary])
            .map_err(|_| TranslationError::Protocol("SSE frame was not UTF-8".into()))?
            .to_owned();
        buffer.drain(..boundary + delimiter_len);
        parse_sse_frame(&frame, output)?;
    }
    Ok(())
}

fn find_sse_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn validate_status(
    status: reqwest::StatusCode,
    body: Option<&Value>,
) -> Result<(), TranslationError> {
    if status.is_success() {
        return Ok(());
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(TranslationError::RateLimited);
    }
    let code = body
        .and_then(|v| v.pointer("/error/code"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    Err(TranslationError::Http {
        status: status.as_u16(),
        code,
    })
}

fn map_reqwest(error: reqwest::Error) -> TranslationError {
    if error.is_timeout() {
        TranslationError::Timeout
    } else {
        TranslationError::Transport(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_opens_after_three_failed_requests_and_requires_explicit_reset() {
        let mut circuit = CircuitState::default();
        circuit.record_failure();
        circuit.record_failure();
        assert!(!circuit.opened);
        circuit.record_failure();
        assert!(circuit.opened);
        circuit.record_success();
        assert!(
            circuit.opened,
            "success cannot implicitly recover an open circuit"
        );
        circuit = CircuitState::default();
        assert!(!circuit.opened);
        assert_eq!(circuit.consecutive_failures, 0);
    }

    #[test]
    fn rate_limit_and_provider_errors_have_stable_redacted_classification() {
        assert!(matches!(
            validate_status(reqwest::StatusCode::TOO_MANY_REQUESTS, None),
            Err(TranslationError::RateLimited)
        ));
        let body = json!({"error": {"code": "model_unavailable", "message": "sensitive"}});
        assert!(matches!(
            validate_status(reqwest::StatusCode::SERVICE_UNAVAILABLE, Some(&body)),
            Err(TranslationError::Http { status: 503, code }) if code == "model_unavailable"
        ));
    }
    #[test]
    fn context_is_truncated_at_sentence_boundaries() {
        let profile = TranslationProfile {
            profile_id: "ecnu".into(),
            endpoint_kind: TranslationEndpointKind::Ecnu,
            endpoint: ECNU_BASE_URL.into(),
            protocol: TranslationProtocol::Responses,
            model: "ecnu-plus".into(),
            target_language: "zh-CN".into(),
            context_sentences: 32,
            body_limit_bytes: 200,
            timeout_ms: 10_000,
            secret_reference: "ecnu".into(),
        };
        let request = TranslationRequest {
            current: "hello".into(),
            recent: (0..32)
                .map(|i| format!("sentence-{i}-long-value"))
                .collect(),
            background: None,
            glossary: vec![],
        };
        let (prompt, count) = build_prompt(&profile, &request).unwrap();
        assert!(prompt.len() <= 200);
        assert!(count < 32);
        assert!(prompt.ends_with("CURRENT:\nhello"));
        assert!(prompt.contains("sentence-31-long-value"));
        assert!(!prompt.contains("sentence-0-long-value"));
    }

    #[test]
    fn optional_background_and_glossary_truncate_without_splitting_utf8() {
        let profile = TranslationProfile {
            profile_id: "ecnu".into(),
            endpoint_kind: TranslationEndpointKind::Ecnu,
            endpoint: ECNU_BASE_URL.into(),
            protocol: TranslationProtocol::Responses,
            model: "ecnu-plus".into(),
            target_language: "zh-CN".into(),
            context_sentences: 10,
            body_limit_bytes: 220,
            timeout_ms: 10_000,
            secret_reference: "ecnu".into(),
        };
        let request = TranslationRequest {
            current: "hello".into(),
            recent: VecDeque::new(),
            background: Some("第一句。第二句非常非常长。第三句。".repeat(20)),
            glossary: vec![("Alice".into(), "爱丽丝".into())],
        };
        let (prompt, _) = build_prompt(&profile, &request).unwrap();
        assert!(prompt.len() <= 220);
        assert!(prompt.contains("Alice => 爱丽丝"));
        assert!(prompt.ends_with("CURRENT:\nhello"));
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn responses_sse_accepts_crlf_and_split_utf8_chunks() {
        let payload = "event: response.output_text.delta\r\ndata: {\"delta\":\"你好\"}\r\n\r\n";
        let bytes = payload.as_bytes();
        let split = payload.find('好').unwrap() + 1;
        let mut buffer = Vec::new();
        let mut output = String::new();
        buffer.extend_from_slice(&bytes[..split]);
        drain_sse_frames(&mut buffer, &mut output).unwrap();
        assert!(output.is_empty());
        buffer.extend_from_slice(&bytes[split..]);
        drain_sse_frames(&mut buffer, &mut output).unwrap();
        assert_eq!(output, "你好");
        assert!(buffer.is_empty());
    }

    #[test]
    fn status_error_code_is_redacted_and_stable() {
        let body = json!({"error": {"code": "invalid_api_key", "message": "secret body"}});
        let error = validate_status(reqwest::StatusCode::UNAUTHORIZED, Some(&body)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "ASTRA_EMU_TRANSLATION_HTTP_401: invalid_api_key"
        );
        assert!(!error.to_string().contains("secret body"));
    }
}
