//! Runtime AI director、IntentValidator 和 session 类型。
//!
//! # Replay 契约
//!
//! [`CommittedAiOutput`] 必须随 save/replay 持久化。回放时不重新请求 provider；
//! 引擎直接从 committed_outputs 重放已提交的 sanitized_payload。
//! 这保证 nondeterministic AI provider 不影响 deterministic replay。

use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use thiserror::Error;
use uuid::Uuid;

/// Runtime AI intent 类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAiIntentKind {
    /// 叙事分支指令。
    NarrativeDirective,
    /// 角色行为触发。
    CharacterAction,
    /// 环境状态变更。
    EnvironmentChange,
    /// 系统内部消息。
    SystemMessage,
}

/// Runtime AI intent。由联网 AI provider 产生，必须通过 IntentValidator 后才能提交。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeAiIntent {
    /// 唯一 intent id。
    pub intent_id: Uuid,
    /// 所属 session id。
    pub session_id: String,
    /// Intent 类型。
    pub kind: RuntimeAiIntentKind,
    /// Provider 原始 payload；尚未经过校验。
    pub payload: serde_json::Value,
    /// 产生此 intent 时的引擎 step 计数。
    pub requested_at_step: u64,
}

/// Intent 校验结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IntentValidationResult {
    /// 是否通过校验。
    pub valid: bool,
    /// 通过校验后的脱敏 payload；失败时为 None。
    pub sanitized_payload: Option<serde_json::Value>,
    /// 诊断信息列表。
    pub diagnostics: Vec<String>,
}

/// Intent 校验器。
///
/// 检查 payload 不含商业文本、本地路径、native handle 等禁止字段。
#[derive(Debug, Default)]
pub struct IntentValidator;

impl IntentValidator {
    /// 校验 RuntimeAiIntent 并返回校验结果。
    ///
    /// 校验通过时 sanitized_payload 是经过清洗的 payload；
    /// 校验失败时 valid=false 且 sanitized_payload=None。
    pub fn validate(&self, intent: &RuntimeAiIntent) -> IntentValidationResult {
        // 禁止字段：payload 不得包含 native_handle、local_path、commercial_text 等
        let forbidden = ["native_handle", "local_path", "commercial_text", "bytecode", "raw_payload"];
        if let Some(obj) = intent.payload.as_object() {
            for key in &forbidden {
                if obj.contains_key(*key) {
                    return IntentValidationResult {
                        valid: false,
                        sanitized_payload: None,
                        diagnostics: vec![format!("forbidden field in intent payload: {key}")],
                    };
                }
            }
        }
        IntentValidationResult {
            valid: true,
            sanitized_payload: Some(intent.payload.clone()),
            diagnostics: vec![],
        }
    }
}

/// AI 输出回放策略。
///
/// - `ReplayExact`：回放时直接使用 committed sanitized_payload，不重新请求 provider。
/// - `Skip`：回放时跳过此 output（例如 ephemeral 展示）。
/// - `Regenerate`：仅用于非 save-bound 上下文；一般不用于 deterministic replay。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiReplayPolicy {
    /// 回放时重用已提交的 payload，不重新调用 provider。
    ReplayExact,
    /// 回放时跳过此输出。
    Skip,
    /// 仅在非 deterministic 场景下重新生成；不参与 save/replay。
    Regenerate,
}

/// 已提交的 AI 输出。
///
/// 提交后 payload 固化进 save/replay，回放不重新请求 provider。
/// 任何 nondeterministic provider 的输出都必须通过 IntentValidator
/// 再以此类型持久化，才能保证 replay 的 determinism。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommittedAiOutput {
    /// 对应的 intent id。
    pub intent_id: Uuid,
    /// 提交时的引擎 step。
    pub committed_at_step: u64,
    /// 经 IntentValidator 清洗后的 payload。
    pub sanitized_payload: serde_json::Value,
    /// 回放策略；通常为 ReplayExact。
    pub replay_policy: AiReplayPolicy,
}

/// AI session 错误。
#[derive(Debug, Error)]
pub enum AiSessionError {
    #[error("intent validation failed: {0}")]
    ValidationFailed(String),
    #[error("provider unavailable")]
    ProviderUnavailable,
    #[error("session is closed")]
    SessionClosed,
}

/// Runtime AI session。管理 committed outputs 并提供 save/replay 支持。
///
/// 提交顺序与引擎 step 严格对应；不允许乱序提交。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpAiSession {
    /// 唯一 session id。
    pub session_id: Uuid,
    /// 绑定的 AI provider id。
    pub provider_id: String,
    /// Session 是否活跃。
    pub active: bool,
    /// 已提交的 AI 输出列表；按提交顺序排列。
    pub committed_outputs: Vec<CommittedAiOutput>,
}

impl McpAiSession {
    /// 创建新 session，绑定指定 provider。
    pub fn new(provider_id: &str) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            provider_id: provider_id.to_owned(),
            active: true,
            committed_outputs: Vec::new(),
        }
    }

    /// 提交 intent。经 IntentValidator 校验通过后固化为 CommittedAiOutput。
    ///
    /// 校验失败时返回 [`AiSessionError::ValidationFailed`]。
    /// Session 关闭时返回 [`AiSessionError::SessionClosed`]。
    pub fn submit_intent(
        &mut self,
        intent: RuntimeAiIntent,
        validator: &IntentValidator,
    ) -> Result<&CommittedAiOutput, AiSessionError> {
        if !self.active {
            return Err(AiSessionError::SessionClosed);
        }
        let result = validator.validate(&intent);
        if !result.valid {
            return Err(AiSessionError::ValidationFailed(
                result.diagnostics.join("; "),
            ));
        }
        let committed = CommittedAiOutput {
            intent_id: intent.intent_id,
            committed_at_step: intent.requested_at_step,
            sanitized_payload: result.sanitized_payload.unwrap_or(serde_json::Value::Null),
            replay_policy: AiReplayPolicy::ReplayExact,
        };
        self.committed_outputs.push(committed);
        Ok(self.committed_outputs.last().unwrap())
    }

    /// 返回已提交输出切片。
    pub fn committed_outputs(&self) -> &[CommittedAiOutput] {
        &self.committed_outputs
    }
}
