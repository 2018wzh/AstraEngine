use encoding_rs::{GBK, SHIFT_JIS};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Explicit source encoding. Never uses replacement decoding or the OS code page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScriptEncoding {
    ShiftJis,
    Gbk,
}

impl ScriptEncoding {
    pub(crate) fn decode(self, bytes: &[u8]) -> Option<Cow<'_, str>> {
        match self {
            Self::ShiftJis => SHIFT_JIS,
            Self::Gbk => GBK,
        }
        .decode_without_bom_handling_and_without_replacement(bytes)
    }

    pub(crate) fn language(self) -> char {
        match self {
            Self::ShiftJis => 'j',
            Self::Gbk => 'e',
        }
    }
}
