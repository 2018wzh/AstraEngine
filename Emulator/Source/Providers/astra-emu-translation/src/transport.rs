use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::Value;

use crate::{
    model::{
        ConnectionTestResult, OpenAiApi, SecretResolver, TranslationError, TranslationProfile,
        TranslationProvider, TranslationRequest, TranslationResult,
    },
    prompt::{build_prompt, prompt_segment_count},
    MAX_CONTEXT_CHARS, MAX_LISTED_MODELS, MAX_RESPONSE_BYTES,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_MAX_TOKENS: u32 = 1_024;

/// One provider request, built without I/O so its shape is testable.
struct HttpCall {
    url: reqwest::Url,
    headers: Vec<(HeaderName, HeaderValue)>,
    body: Option<Value>,
}

pub struct TranslationClient<R: ?Sized> {
    client: reqwest::Client,
    profile: TranslationProfile,
    secrets: Arc<R>,
}

impl<R: SecretResolver + ?Sized> TranslationClient<R> {
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

    /// Translate one displayed segment. Dropping this future cancels the
    /// underlying request and no cache entry is made by this provider.
    pub async fn translate(
        &self,
        request: &TranslationRequest,
    ) -> Result<TranslationResult, TranslationError> {
        request.validate()?;
        let prompt = build_prompt(&self.profile, request)?;
        let secret = self.secrets.resolve(&self.profile.secret_reference)?;
        let started = Instant::now();
        let call = translation_call(&self.profile, &secret, &prompt)?;
        let value = self.send(call).await?;
        let translated = parse_translation(&self.profile, &value)?.trim().to_owned();
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
            sent_segment_count: prompt_segment_count(&self.profile, request),
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

    /// Authenticate the configured endpoint and list its models without
    /// sending game text.
    pub async fn test_connection(&self) -> Result<ConnectionTestResult, TranslationError> {
        let secret = self.secrets.resolve(&self.profile.secret_reference)?;
        let started = Instant::now();
        let call = models_call(&self.profile, &secret)?;
        let value = self.send(call).await?;
        let models = parse_models(self.profile.provider, &value)?;
        Ok(ConnectionTestResult {
            provider_identity: self.provider_identity(),
            latency_ms: elapsed_ms(started.elapsed()),
            models,
        })
    }

    async fn send(&self, call: HttpCall) -> Result<Value, TranslationError> {
        let mut builder = match &call.body {
            Some(body) => self.client.post(call.url).json(body),
            None => self.client.get(call.url),
        };
        for (name, value) in call.headers {
            builder = builder.header(name, value);
        }
        let response = builder.send().await.map_err(map_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TranslationError::HttpStatus(status.as_u16()));
        }
        let bytes = read_bounded_body(response).await?;
        serde_json::from_slice(&bytes)
            .map_err(|_| TranslationError::Protocol("provider response is not JSON"))
    }
}

fn endpoint(base_url: &str, segments: &[&str]) -> Result<reqwest::Url, TranslationError> {
    let mut url = reqwest::Url::parse(base_url.trim_end_matches('/'))
        .map_err(|_| TranslationError::InvalidProfile("base_url is not a URL"))?;
    url.path_segments_mut()
        .map_err(|_| TranslationError::InvalidProfile("base_url is not a URL"))?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

fn secret_header(
    name: HeaderName,
    value: &str,
) -> Result<(HeaderName, HeaderValue), TranslationError> {
    let mut value =
        HeaderValue::from_str(value).map_err(|_| TranslationError::SecretUnavailable)?;
    value.set_sensitive(true);
    Ok((name, value))
}

fn auth_headers(
    provider: TranslationProvider,
    secret: &str,
) -> Result<Vec<(HeaderName, HeaderValue)>, TranslationError> {
    Ok(match provider {
        TranslationProvider::OpenAi => {
            vec![secret_header(AUTHORIZATION, &format!("Bearer {secret}"))?]
        }
        TranslationProvider::Anthropic => vec![
            secret_header(HeaderName::from_static("x-api-key"), secret)?,
            (
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static(ANTHROPIC_VERSION),
            ),
        ],
        TranslationProvider::Google => vec![secret_header(
            HeaderName::from_static("x-goog-api-key"),
            secret,
        )?],
    })
}

fn translation_call(
    profile: &TranslationProfile,
    secret: &str,
    prompt: &str,
) -> Result<HttpCall, TranslationError> {
    let base = profile.base_url.as_str();
    let (url, body) = match (profile.provider, profile.openai_api) {
        (TranslationProvider::OpenAi, OpenAiApi::Responses) => (
            endpoint(base, &["responses"])?,
            serde_json::json!({
                "model": profile.model,
                "input": prompt,
                "stream": false,
            }),
        ),
        (TranslationProvider::OpenAi, OpenAiApi::ChatCompletions) => (
            endpoint(base, &["chat", "completions"])?,
            serde_json::json!({
                "model": profile.model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": false,
            }),
        ),
        (TranslationProvider::Anthropic, _) => (
            endpoint(base, &["messages"])?,
            serde_json::json!({
                "model": profile.model,
                "max_tokens": ANTHROPIC_MAX_TOKENS,
                "messages": [{"role": "user", "content": prompt}],
            }),
        ),
        (TranslationProvider::Google, _) => {
            let model = profile
                .model
                .strip_prefix("models/")
                .unwrap_or(&profile.model);
            (
                endpoint(base, &["models", &format!("{model}:generateContent")])?,
                serde_json::json!({
                    "contents": [{"parts": [{"text": prompt}]}],
                }),
            )
        }
    };
    Ok(HttpCall {
        url,
        headers: auth_headers(profile.provider, secret)?,
        body: Some(body),
    })
}

fn models_call(profile: &TranslationProfile, secret: &str) -> Result<HttpCall, TranslationError> {
    let mut url = endpoint(&profile.base_url, &["models"])?;
    match profile.provider {
        TranslationProvider::OpenAi => {}
        TranslationProvider::Anthropic => {
            url.query_pairs_mut().append_pair("limit", "1000");
        }
        TranslationProvider::Google => {
            url.query_pairs_mut().append_pair("pageSize", "1000");
        }
    }
    Ok(HttpCall {
        url,
        headers: auth_headers(profile.provider, secret)?,
        body: None,
    })
}

fn parse_translation(
    profile: &TranslationProfile,
    value: &Value,
) -> Result<String, TranslationError> {
    match (profile.provider, profile.openai_api) {
        (TranslationProvider::OpenAi, OpenAiApi::Responses) => parse_responses_output(value),
        (TranslationProvider::OpenAi, OpenAiApi::ChatCompletions) => parse_chat_output(value),
        (TranslationProvider::Anthropic, _) => parse_anthropic_output(value),
        (TranslationProvider::Google, _) => parse_google_output(value),
    }
}

fn parse_anthropic_output(value: &Value) -> Result<String, TranslationError> {
    if value.get("error").is_some() {
        return Err(TranslationError::Protocol(
            "provider response contains an error",
        ));
    }
    if value.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        return Err(TranslationError::Protocol(
            "provider response contains refusal",
        ));
    }
    let parts =
        value
            .get("content")
            .and_then(Value::as_array)
            .ok_or(TranslationError::Protocol(
                "Anthropic response omitted content",
            ))?;
    let mut output = String::new();
    let mut saw_output = false;
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let text = part
            .get("text")
            .and_then(Value::as_str)
            .ok_or(TranslationError::Protocol(
                "Anthropic text block is invalid",
            ))?;
        saw_output = true;
        output.push_str(text);
    }
    if saw_output && !output.is_empty() {
        Ok(output)
    } else {
        Err(TranslationError::Protocol(
            "Anthropic response contains no text",
        ))
    }
}

fn parse_google_output(value: &Value) -> Result<String, TranslationError> {
    if value.get("error").is_some() {
        return Err(TranslationError::Protocol(
            "provider response contains an error",
        ));
    }
    let parts = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .ok_or(TranslationError::Protocol(
            "Gemini response omitted candidate parts",
        ))?;
    let mut output = String::new();
    for part in parts {
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(text) = part.get("text") {
            let text = text
                .as_str()
                .ok_or(TranslationError::Protocol("Gemini text part is invalid"))?;
            output.push_str(text);
        }
    }
    if output.is_empty() {
        Err(TranslationError::Protocol(
            "Gemini response contains no text",
        ))
    } else {
        Ok(output)
    }
}

fn parse_models(
    provider: TranslationProvider,
    value: &Value,
) -> Result<Vec<String>, TranslationError> {
    let invalid = TranslationError::Protocol("model list response is invalid");
    let mut models = match provider {
        TranslationProvider::OpenAi | TranslationProvider::Anthropic => value
            .get("data")
            .and_then(Value::as_array)
            .ok_or(invalid.clone())?
            .iter()
            .map(|model| {
                model
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or(invalid.clone())
            })
            .collect::<Result<Vec<_>, _>>()?,
        TranslationProvider::Google => value
            .get("models")
            .and_then(Value::as_array)
            .ok_or(invalid.clone())?
            .iter()
            .filter(|model| {
                model
                    .get("supportedGenerationMethods")
                    .and_then(Value::as_array)
                    .is_some_and(|methods| {
                        methods
                            .iter()
                            .any(|method| method.as_str() == Some("generateContent"))
                    })
            })
            .map(|model| {
                let name = model
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(invalid.clone())?;
                Ok(name.strip_prefix("models/").unwrap_or(name).to_owned())
            })
            .collect::<Result<Vec<_>, TranslationError>>()?,
    };
    models.retain(|model| !model.is_empty());
    models.sort();
    models.dedup();
    models.truncate(MAX_LISTED_MODELS);
    Ok(models)
}

async fn read_bounded_body(response: reqwest::Response) -> Result<Vec<u8>, TranslationError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_reqwest)?;
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or(TranslationError::Protocol("provider response is too large"))?;
        if next_len > MAX_RESPONSE_BYTES {
            return Err(TranslationError::Protocol("provider response is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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

fn parse_responses_output(value: &Value) -> Result<String, TranslationError> {
    validate_response_status(value)?;

    let mut output = String::new();
    let mut saw_output = false;
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            let item_type = item.get("type").and_then(Value::as_str);
            if matches!(item_type, Some("refusal")) {
                return Err(TranslationError::Protocol(
                    "provider response contains refusal",
                ));
            }
            if matches!(item_type, Some("reasoning" | "tool_call" | "function_call")) {
                continue;
            }
            let Some(parts) = item.get("content").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                let part_type = part.get("type").and_then(Value::as_str);
                if matches!(part_type, Some("refusal")) {
                    return Err(TranslationError::Protocol(
                        "provider response contains refusal",
                    ));
                }
                if matches!(part_type, Some("reasoning" | "tool_call" | "function_call")) {
                    continue;
                }
                // Responses uses `output_text`; accepting an omitted type keeps
                // compatibility with older OpenAI-compatible gateways while
                // still excluding explicit reasoning/tool parts.
                if part_type.is_some_and(|kind| kind != "output_text" && kind != "text") {
                    continue;
                }
                let Some(text) = part.get("text").and_then(Value::as_str) else {
                    return Err(TranslationError::Protocol(
                        "Responses output_text part is invalid",
                    ));
                };
                saw_output = true;
                output.push_str(text);
            }
        }
    }
    if saw_output {
        return Ok(output);
    }
    // Some compatible gateways expose the already-concatenated form only.
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Ok(text.to_owned());
    }
    Err(TranslationError::Protocol(
        "Responses response omitted output text",
    ))
}

fn parse_chat_output(value: &Value) -> Result<String, TranslationError> {
    validate_response_status(value)?;
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or(TranslationError::Protocol(
            "Chat Completions response omitted choices",
        ))?;
    if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
        match finish_reason {
            "stop" => {}
            "length" => {
                return Err(TranslationError::Protocol(
                    "Chat Completions response is incomplete",
                ));
            }
            "content_filter" => {
                return Err(TranslationError::Protocol(
                    "Chat Completions response contains refusal",
                ));
            }
            "tool_calls" | "function_call" => {
                return Err(TranslationError::Protocol(
                    "Chat Completions response contains tool call",
                ));
            }
            _ => {
                return Err(TranslationError::Protocol(
                    "Chat Completions response has invalid finish reason",
                ));
            }
        }
    }
    let message = choice.get("message").ok_or(TranslationError::Protocol(
        "Chat Completions response omitted message",
    ))?;
    if message
        .get("refusal")
        .and_then(Value::as_str)
        .is_some_and(|refusal| !refusal.is_empty())
    {
        return Err(TranslationError::Protocol(
            "Chat Completions response contains refusal",
        ));
    }
    let content = message.get("content").ok_or(TranslationError::Protocol(
        "Chat Completions response omitted message content",
    ))?;
    if let Some(text) = content.as_str() {
        return Ok(text.to_owned());
    }
    let Some(parts) = content.as_array() else {
        return Err(TranslationError::Protocol(
            "Chat Completions content has no text",
        ));
    };
    let mut output = String::new();
    let mut saw_output = false;
    for part in parts {
        let part_type = part.get("type").and_then(Value::as_str);
        if matches!(part_type, Some("refusal")) {
            return Err(TranslationError::Protocol(
                "Chat Completions response contains refusal",
            ));
        }
        if matches!(part_type, Some("reasoning" | "tool_call" | "function_call")) {
            continue;
        }
        if part_type.is_some_and(|kind| kind != "text" && kind != "output_text") {
            continue;
        }
        let Some(text) = part.get("text").and_then(Value::as_str) else {
            return Err(TranslationError::Protocol(
                "Chat Completions text part is invalid",
            ));
        };
        saw_output = true;
        output.push_str(text);
    }
    if saw_output {
        Ok(output)
    } else {
        Err(TranslationError::Protocol(
            "Chat Completions content has no text",
        ))
    }
}

fn validate_response_status(value: &Value) -> Result<(), TranslationError> {
    if value.get("error").is_some() {
        return Err(TranslationError::Protocol(
            "provider response contains an error",
        ));
    }
    match value.get("status").and_then(Value::as_str) {
        None | Some("completed") => Ok(()),
        Some("incomplete") => Err(TranslationError::Protocol(
            "provider response is incomplete",
        )),
        Some(_) => Err(TranslationError::Protocol(
            "provider response has invalid status",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(provider: TranslationProvider) -> TranslationProfile {
        TranslationProfile {
            profile_id: "test".into(),
            provider,
            base_url: provider.default_base_url().into(),
            model: "model-x".into(),
            secret_reference: "test.secret".into(),
            ..Default::default()
        }
    }

    fn header<'a>(call: &'a HttpCall, name: &str) -> Option<&'a str> {
        call.headers
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .and_then(|(_, value)| value.to_str().ok())
    }

    #[test]
    fn openai_requests_use_bearer_auth_and_selected_api() {
        let mut openai = profile(TranslationProvider::OpenAi);
        let call = translation_call(&openai, "key", "prompt").unwrap();
        assert_eq!(call.url.as_str(), "https://api.openai.com/v1/responses");
        assert_eq!(header(&call, "authorization"), Some("Bearer key"));
        assert!(call.headers.iter().all(|(_, value)| value.is_sensitive()));
        assert_eq!(call.body.as_ref().unwrap()["input"], "prompt");

        openai.openai_api = OpenAiApi::ChatCompletions;
        openai.base_url = "http://127.0.0.1:11434/v1/".into();
        let call = translation_call(&openai, "key", "prompt").unwrap();
        assert_eq!(
            call.url.as_str(),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(
            call.body.as_ref().unwrap()["messages"][0]["content"],
            "prompt"
        );
        let models = models_call(&openai, "key").unwrap();
        assert_eq!(models.url.as_str(), "http://127.0.0.1:11434/v1/models");
        assert!(models.body.is_none());
    }

    #[test]
    fn anthropic_request_has_version_header_and_bounded_tokens() {
        let anthropic = profile(TranslationProvider::Anthropic);
        let call = translation_call(&anthropic, "key", "prompt").unwrap();
        assert_eq!(call.url.as_str(), "https://api.anthropic.com/v1/messages");
        assert_eq!(header(&call, "x-api-key"), Some("key"));
        assert_eq!(header(&call, "anthropic-version"), Some("2023-06-01"));
        assert_eq!(header(&call, "authorization"), None);
        let body = serde_json::to_string(call.body.as_ref().unwrap()).unwrap();
        assert!(body.contains(r#""max_tokens":1024"#));
        assert_eq!(
            call.body.unwrap(),
            serde_json::json!({
                "model": "model-x",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "prompt"}],
            })
        );
        let models = models_call(&anthropic, "key").unwrap();
        assert_eq!(models.url.path(), "/v1/models");
        assert_eq!(header(&models, "anthropic-version"), Some("2023-06-01"));
    }

    #[test]
    fn anthropic_text_blocks_are_concatenated() {
        let value = serde_json::json!({"content":[{"type":"text","text":"你好"}]});
        assert_eq!(parse_anthropic_output(&value).unwrap(), "你好");
        let mixed = serde_json::json!({"content":[
            {"type":"thinking","thinking":"ignore"},
            {"type":"text","text":"你"},
            {"type":"text","text":"好"}
        ]});
        assert_eq!(parse_anthropic_output(&mixed).unwrap(), "你好");
        for invalid in [
            serde_json::json!({"content":[]}),
            serde_json::json!({"type":"error","error":{"message":"private"}}),
            serde_json::json!({"content":"text"}),
        ] {
            assert!(matches!(
                parse_anthropic_output(&invalid),
                Err(TranslationError::Protocol(_))
            ));
        }
    }

    #[test]
    fn google_request_targets_generate_content_with_api_key_header() {
        let mut google = profile(TranslationProvider::Google);
        google.model = "models/gemini-2.5-flash".into();
        let call = translation_call(&google, "key", "prompt").unwrap();
        assert_eq!(
            call.url.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        );
        assert_eq!(header(&call, "x-goog-api-key"), Some("key"));
        assert_eq!(
            call.body.unwrap(),
            serde_json::json!({"contents":[{"parts":[{"text":"prompt"}]}]})
        );
    }

    #[test]
    fn google_candidate_parts_are_concatenated() {
        let value =
            serde_json::json!({"candidates":[{"content":{"parts":[{"text":"A"},{"text":"B"}]}}]});
        assert_eq!(parse_google_output(&value).unwrap(), "AB");
        for invalid in [
            serde_json::json!({"candidates":[]}),
            serde_json::json!({"candidates":[{"content":{"parts":[]}}]}),
            serde_json::json!({"error":{"message":"private"}}),
        ] {
            assert!(matches!(
                parse_google_output(&invalid),
                Err(TranslationError::Protocol(_))
            ));
        }
    }

    #[test]
    fn model_lists_are_filtered_sorted_and_bounded() {
        let google = serde_json::json!({"models":[
            {"name":"models/gemini-b","supportedGenerationMethods":["generateContent","countTokens"]},
            {"name":"models/embedding","supportedGenerationMethods":["embedContent"]},
            {"name":"models/gemini-a","supportedGenerationMethods":["generateContent"]}
        ]});
        assert_eq!(
            parse_models(TranslationProvider::Google, &google).unwrap(),
            ["gemini-a", "gemini-b"]
        );
        let anthropic = serde_json::json!({"data":[{"id":"claude-b"},{"id":"claude-a"}]});
        assert_eq!(
            parse_models(TranslationProvider::Anthropic, &anthropic).unwrap(),
            ["claude-a", "claude-b"]
        );
        let many = serde_json::json!({
            "data": (0..250).map(|index| serde_json::json!({"id": format!("m{index:03}")})).collect::<Vec<_>>()
        });
        let models = parse_models(TranslationProvider::OpenAi, &many).unwrap();
        assert_eq!(models.len(), MAX_LISTED_MODELS);
        assert_eq!(models.first().map(String::as_str), Some("m000"));
        assert!(models.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(parse_models(TranslationProvider::OpenAi, &serde_json::json!({})).is_err());
    }

    #[test]
    fn response_shapes_are_parsed_without_exposing_body() {
        let responses = serde_json::json!({"output_text":"你好"});
        assert_eq!(parse_responses_output(&responses).unwrap(), "你好");
        let nested = serde_json::json!({"output":[{"content":[{"text":"你好"}]}]});
        assert_eq!(parse_responses_output(&nested).unwrap(), "你好");
        let chat = serde_json::json!({"choices":[{"message":{"content":"你好"}}]});
        assert_eq!(parse_chat_output(&chat).unwrap(), "你好");
        let malformed = serde_json::json!({"error":{"message":"private body"}});
        let error = parse_chat_output(&malformed).unwrap_err();
        assert!(!error.to_string().contains("private body"));
    }

    #[test]
    fn responses_concatenate_output_text_and_skip_reasoning_and_tools() {
        let value = serde_json::json!({
            "status": "completed",
            "output": [
                {"type": "reasoning", "summary": [{"text": "ignore"}]},
                {"type": "message", "content": [
                    {"type": "output_text", "text": "你"},
                    {"type": "output_text", "text": "好"}
                ]},
                {"type": "function_call", "name": "ignore"}
            ]
        });
        assert_eq!(parse_responses_output(&value).unwrap(), "你好");
    }

    #[test]
    fn responses_reject_refusal_and_incomplete_results() {
        let refusal = serde_json::json!({
            "status": "completed",
            "output": [{"type": "message", "content": [{
                "type": "refusal", "refusal": "no"
            }]}]
        });
        assert!(matches!(
            parse_responses_output(&refusal),
            Err(TranslationError::Protocol(
                "provider response contains refusal"
            ))
        ));
        let incomplete = serde_json::json!({"status": "incomplete", "output": []});
        assert!(matches!(
            parse_responses_output(&incomplete),
            Err(TranslationError::Protocol(
                "provider response is incomplete"
            ))
        ));
    }

    #[test]
    fn chat_content_parts_are_concatenated_and_tool_or_refusal_is_rejected() {
        let value = serde_json::json!({
            "choices": [{"finish_reason": "stop", "message": {"content": [
                {"type": "text", "text": "你"},
                {"type": "reasoning", "text": "ignore"},
                {"type": "text", "text": "好"}
            ]}}]
        });
        assert_eq!(parse_chat_output(&value).unwrap(), "你好");

        let tool = serde_json::json!({
            "choices": [{"finish_reason": "tool_calls", "message": {"content": null}}]
        });
        assert!(matches!(
            parse_chat_output(&tool),
            Err(TranslationError::Protocol(
                "Chat Completions response contains tool call"
            ))
        ));
        let refusal = serde_json::json!({
            "choices": [{"finish_reason": "stop", "message": {
                "refusal": "no", "content": null
            }}]
        });
        assert!(matches!(
            parse_chat_output(&refusal),
            Err(TranslationError::Protocol(
                "Chat Completions response contains refusal"
            ))
        ));
    }
}
