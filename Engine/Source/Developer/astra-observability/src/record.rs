use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LOG_EVENT_SCHEMA: &str = "astra.log_event.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanContextV1 {
    pub name: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEventV1 {
    pub schema: String,
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub event: String,
    pub session_id: String,
    pub process_role: String,
    pub thread_label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub span_stack: Vec<SpanContextV1>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
}

pub(crate) fn sanitize_field(key: &str, value: Value) -> Value {
    if is_forbidden_key(key) {
        return Value::String("[redacted]".to_string());
    }
    match value {
        Value::String(value) if contains_private_location(&value) => {
            Value::String("[redacted]".to_string())
        }
        other => other,
    }
}

fn is_forbidden_key(key: &str) -> bool {
    key.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| {
            matches!(
                part.to_ascii_lowercase().as_str(),
                "payload"
                    | "secret"
                    | "password"
                    | "token"
                    | "handle"
                    | "bytecode"
                    | "content"
                    | "commercial"
                    | "path"
                    | "root"
            )
        })
}

fn contains_private_location(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with('\\')
        || value.contains("://")
        || bytes.windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && matches!(window[2], b'\\' | b'/')
        })
}
