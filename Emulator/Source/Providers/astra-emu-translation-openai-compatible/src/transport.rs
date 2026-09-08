use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde_json::Value;

use crate::{
    model::{
        ConnectionTestResult, SecretResolver, TranslationError, TranslationProfile,
        TranslationProtocol, TranslationRequest, TranslationResult,
    },
    prompt::{build_prompt, prompt_segment_count},
    MAX_CONTEXT_CHARS, MAX_RESPONSE_BYTES,
};

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
        let bytes = read_bounded_body(response).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| TranslationError::Protocol("provider response is not JSON"))?;
        match self.profile.protocol {
            TranslationProtocol::Responses => parse_responses_output(&value),
            TranslationProtocol::ChatCompletions => parse_chat_output(&value),
        }
    }
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
