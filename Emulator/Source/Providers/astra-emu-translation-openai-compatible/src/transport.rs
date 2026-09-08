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
}
