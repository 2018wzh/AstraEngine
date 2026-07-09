//! AI provider contract types and trait.
//!
//! 定义 AiProvider trait、provider descriptor 和 completion 请求/响应类型。
//! Provider 实现位于具体 provider crate；此模块只定义公共契约。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// AI provider 类型分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    /// 多轮对话 chat completion。
    Chat,
    /// 单轮文本 completion。
    Completion,
    /// 图像生成。
    ImageGen,
    /// 本地 ONNX 推理。
    Onnx,
    /// 引擎内嵌模型。
    Embedded,
}

/// AI provider 元数据描述符。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiProviderDescriptor {
    /// 唯一 provider 标识，例如 `"openai.gpt4o"`。
    pub provider_id: String,
    /// Provider 类型。
    pub kind: AiProviderKind,
    /// 展示名称。
    pub display_name: String,
    /// 实际模型 id 或版本。
    pub model_id: String,
    /// 可选 API 端点 URL。不含认证信息。
    pub endpoint_url: Option<String>,
    /// 是否需要 API key 才能调用。
    pub requires_api_key: bool,
    /// 是否支持流式输出。
    pub supports_streaming: bool,
    /// 最大上下文 token 数。
    pub max_context_tokens: Option<u32>,
}

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiMessageRole {
    System,
    User,
    Assistant,
}

/// 单条消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AiMessage {
    pub role: AiMessageRole,
    pub content: String,
}

/// Token 使用量统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// AI completion 响应。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiCompletionResponse {
    pub content: String,
    pub finish_reason: String,
    pub token_usage: Option<TokenUsage>,
}

/// AI completion 请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiCompletionRequest {
    pub messages: Vec<AiMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop_sequences: Vec<String>,
}

/// AI provider 错误。
#[derive(Debug, Error)]
pub enum AiProviderError {
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("provider is unavailable")]
    Unavailable,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("token limit exceeded")]
    TokenLimitExceeded,
    #[error("authentication failed")]
    AuthFailed,
}

/// Result type for AI completions.
pub type AiCompletionResult = Result<AiCompletionResponse, AiProviderError>;

/// AI provider trait.
///
/// Implementations must be Send + Sync 以支持跨线程调用。
pub trait AiProvider: Send + Sync {
    /// 返回唯一 provider id。
    fn provider_id(&self) -> &str;

    /// 返回 provider 类型。
    fn provider_kind(&self) -> AiProviderKind;

    /// 执行 completion 请求。
    fn complete(&self, request: AiCompletionRequest) -> AiCompletionResult;

    /// 检查 provider 是否可用（例如 API key 已配置、网络可达）。
    fn is_available(&self) -> bool;
}
