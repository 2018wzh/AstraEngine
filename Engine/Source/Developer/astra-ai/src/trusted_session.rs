//! TrustedSession：AI copilot 对项目文件的受信写入流程。
//!
//! 所有写入操作必须先通过 `prepare_write` 生成 `TrustedWriteReview`，
//! 再由 Editor 用户确认后调用 `apply_write`，或拒绝调用 `reject_write`。
//! 不支持跳过 review 步骤直接写入。

use std::collections::HashMap;
use std::time::SystemTime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// TrustedSession 授权的操作类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrustedOperation {
    /// 写入 `.astra` story source。
    WriteStorySource,
    /// 写入 Luau policy 文件。
    WriteLuauPolicy,
    /// 写入 Graph 布局元数据。
    WriteGraphLayout,
    /// 写入 Timeline clip 元数据。
    WriteTimelineClip,
    /// 写入项目元数据（非 payload 字段）。
    WriteProjectMetadata,
}

/// TrustedSession scope，定义 session 边界和授权操作集合。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrustedSessionScope {
    /// 唯一 scope id。
    pub scope_id: Uuid,
    /// 关联的项目 session id（字符串，不跨 ABI 传 Uuid）。
    pub project_session_id: String,
    /// 绑定的 provider id。
    pub provider_id: String,
    /// 授权时间戳。
    pub granted_at: u64,
    /// 被授权的操作列表。
    pub granted_operations: Vec<TrustedOperation>,
}

/// 审计事件，记录写入来源和操作摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuditEvent {
    /// 唯一事件 id。
    pub event_id: Uuid,
    /// 发起操作的 provider id。
    pub provider_id: String,
    /// 操作描述字符串。
    pub operation: String,
    /// Unix epoch 秒级时间戳。
    pub timestamp: u64,
    /// 可选 trace context（例如 request id 或 span id）。
    pub trace: String,
}

/// 受信写入请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrustedWriteRequest {
    /// 唯一请求 id。
    pub request_id: Uuid,
    /// 关联的 scope id。
    pub scope_id: Uuid,
    /// 本次写入的操作类型。
    pub operation: TrustedOperation,
    /// 文本 patch（例如 unified diff 或完整新内容）。
    pub patch: String,
    /// 可选 graph 结构 diff（序列化为 JSON）。
    pub graph_diff: Option<serde_json::Value>,
    /// 审计事件。
    pub audit_event: AuditEvent,
}

/// 写入 review，由 `prepare_write` 产生，供 Editor 用户确认。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrustedWriteReview {
    /// 对应的请求 id。
    pub request_id: Uuid,
    /// 展示给用户的 patch 内容。
    pub patch: String,
    /// 可选 graph diff。
    pub graph_diff: Option<serde_json::Value>,
    /// 审计事件。
    pub audit_event: AuditEvent,
    /// 若用户拒绝写入时可回退的 undo checkpoint id。
    pub undo_checkpoint_id: Uuid,
    /// Release check 摘要列表（来自 release gate 预检）。
    pub release_check_summary: Vec<String>,
}

/// 写入操作结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum TrustedWriteOutcome {
    /// 写入已应用，checkpoint_id 可用于 undo。
    Applied { checkpoint_id: Uuid },
    /// 写入被拒绝，reason 说明原因。
    Rejected { reason: String },
}

/// TrustedSession 管理 AI copilot 的受信写入流程。
pub struct TrustedSession {
    scope: TrustedSessionScope,
    pending_reviews: HashMap<Uuid, TrustedWriteReview>,
}

impl TrustedSession {
    /// 创建新 TrustedSession。
    pub fn new(scope: TrustedSessionScope) -> Self {
        Self {
            scope,
            pending_reviews: HashMap::new(),
        }
    }

    /// 准备写入：校验 operation 是否在授权范围内，返回 [`TrustedWriteReview`]。
    pub fn prepare_write(&mut self, request: TrustedWriteRequest) -> TrustedWriteReview {
        let undo_checkpoint_id = Uuid::new_v4();
        let release_check_summary = if self.scope.granted_operations.contains(&request.operation) {
            vec!["operation authorized".to_string()]
        } else {
            vec![format!(
                "operation {:?} is not in granted_operations",
                request.operation
            )]
        };
        let review = TrustedWriteReview {
            request_id: request.request_id,
            patch: request.patch,
            graph_diff: request.graph_diff,
            audit_event: request.audit_event,
            undo_checkpoint_id,
            release_check_summary,
        };
        self.pending_reviews.insert(request.request_id, review.clone());
        review
    }

    /// 应用写入：从 pending reviews 取出 review，执行写入逻辑（stub）。
    pub fn apply_write(&mut self, review_id: Uuid) -> TrustedWriteOutcome {
        match self.pending_reviews.remove(&review_id) {
            Some(review) => {
                // TODO: 执行实际 patch 写入；此处为 stub
                tracing::info!(
                    request_id = %review.request_id,
                    checkpoint_id = %review.undo_checkpoint_id,
                    "trusted_session.apply_write"
                );
                TrustedWriteOutcome::Applied {
                    checkpoint_id: review.undo_checkpoint_id,
                }
            }
            None => TrustedWriteOutcome::Rejected {
                reason: format!("review {review_id} not found or already consumed"),
            },
        }
    }

    /// 拒绝写入：移除 pending review，记录拒绝原因。
    pub fn reject_write(&mut self, review_id: Uuid, reason: String) -> TrustedWriteOutcome {
        self.pending_reviews.remove(&review_id);
        tracing::info!(%review_id, %reason, "trusted_session.reject_write");
        TrustedWriteOutcome::Rejected { reason }
    }

    /// 返回 scope 引用。
    pub fn scope(&self) -> &TrustedSessionScope {
        &self.scope
    }
}
