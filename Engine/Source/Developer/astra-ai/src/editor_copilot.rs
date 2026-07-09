//! Editor Copilot：管理 AI assistant 对话 session 和 inline hint。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Copilot 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CopilotMessageRole {
    User,
    Assistant,
    System,
    Error,
}

/// Copilot 对话消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CopilotMessage {
    /// 唯一消息 id。
    pub message_id: Uuid,
    /// 消息角色。
    pub role: CopilotMessageRole,
    /// 消息内容。
    pub content: String,
    /// Unix epoch 毫秒时间戳。
    pub timestamp: u64,
    /// 可选来源引用（例如文件路径、命令 id）。
    pub source_ref: Option<String>,
}

/// Copilot session。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CopilotSession {
    /// 唯一 session id。
    pub session_id: Uuid,
    /// 绑定的 provider id。
    pub provider_id: String,
    /// 消息历史，按时间顺序排列。
    pub messages: Vec<CopilotMessage>,
}

impl CopilotSession {
    /// 创建新 session。
    pub fn new(provider_id: &str) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            provider_id: provider_id.to_owned(),
            messages: Vec::new(),
        }
    }

    /// 添加用户消息。
    pub fn add_user_message(&mut self, content: String) -> &CopilotMessage {
        let msg = CopilotMessage {
            message_id: Uuid::new_v4(),
            role: CopilotMessageRole::User,
            content,
            timestamp: unix_now_millis(),
            source_ref: None,
        };
        self.messages.push(msg);
        self.messages.last().unwrap()
    }

    /// 添加 assistant 消息。
    pub fn add_assistant_message(&mut self, content: String) -> &CopilotMessage {
        let msg = CopilotMessage {
            message_id: Uuid::new_v4(),
            role: CopilotMessageRole::Assistant,
            content,
            timestamp: unix_now_millis(),
            source_ref: None,
        };
        self.messages.push(msg);
        self.messages.last().unwrap()
    }

    /// 返回消息列表。
    pub fn messages(&self) -> &[CopilotMessage] {
        &self.messages
    }

    /// 清空消息历史。
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

/// Inline hint 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InlineHintKind {
    Completion,
    Diagnostic,
    Reference,
}

/// Inline hint，展示在 story editor 的指定行旁边。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InlineHint {
    /// 唯一 hint id。
    pub hint_id: Uuid,
    /// 目标行（1-indexed）。
    pub position_line: u32,
    /// Hint 内容。
    pub content: String,
    /// Hint 类型。
    pub kind: InlineHintKind,
}

/// Copilot session 标识符。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct CopilotSessionId(pub Uuid);

impl CopilotSessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CopilotSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CopilotSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Editor Copilot：管理多个 copilot session 和 inline hint 生成。
#[derive(Debug, Default)]
pub struct EditorCopilot {
    sessions: HashMap<CopilotSessionId, CopilotSession>,
}

impl EditorCopilot {
    /// 创建新 EditorCopilot。
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建新 copilot session，返回 session id。
    pub fn create_session(&mut self, provider_id: &str) -> CopilotSessionId {
        let session = CopilotSession::new(provider_id);
        let id = CopilotSessionId(session.session_id);
        self.sessions.insert(id.clone(), session);
        id
    }

    /// 获取 session 引用。
    pub fn session(&self, id: &CopilotSessionId) -> Option<&CopilotSession> {
        self.sessions.get(id)
    }

    /// 获取 session 可变引用。
    pub fn session_mut(&mut self, id: &CopilotSessionId) -> Option<&mut CopilotSession> {
        self.sessions.get_mut(id)
    }

    /// 根据当前上下文和光标行生成 inline hint 列表。
    ///
    /// TODO: 调用 AiProvider 生成建议；此处为 stub。
    pub fn generate_inline_hint(&self, _context: &str, cursor_line: u32) -> Vec<InlineHint> {
        vec![InlineHint {
            hint_id: Uuid::new_v4(),
            position_line: cursor_line,
            content: String::new(),
            kind: InlineHintKind::Completion,
        }]
    }
}

fn unix_now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
