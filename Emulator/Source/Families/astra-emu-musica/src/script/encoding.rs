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
    /// Source branch policy: compare strict per-line decoding, preferring the
    /// configured primary on ties. Parsing still rejects any remaining errors.
    pub fn detect(bytes: &[u8], primary: Self) -> Self {
        let invalid = |encoding: Self| {
            bytes
                .split_inclusive(|byte| *byte == b'\n')
                .filter(|line| {
                    let logical = line
                        .strip_suffix(b"\r\n")
                        .or_else(|| line.strip_suffix(b"\n"))
                        .unwrap_or(line);
                    encoding.decode(logical).is_none()
                })
                .count()
        };
        let primary_errors = invalid(primary);
        let alternate = match primary {
            Self::ShiftJis => Self::Gbk,
            Self::Gbk => Self::ShiftJis,
        };
        let selected = if primary_errors > 0 && invalid(alternate) < primary_errors {
            alternate
        } else {
            primary
        };
        tracing::debug!(
            event = "astra.emu.musica.script.encoding",
            ?primary,
            ?selected,
            primary_errors
        );
        selected
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_file_detection_keeps_primary_on_ties_and_rejects_remaining_errors() {
        let (japanese, _, _) = SHIFT_JIS.encode(".message 1   ｱ\r\n.end\r\n");
        let (chinese, _, _) = GBK.encode(".message 1  姓名 中文剧情\r\n.end\r\n");
        assert_eq!(
            ScriptEncoding::detect(&japanese, ScriptEncoding::Gbk),
            ScriptEncoding::ShiftJis
        );
        assert_eq!(
            ScriptEncoding::detect(&chinese, ScriptEncoding::ShiftJis),
            ScriptEncoding::Gbk
        );
        for primary in [ScriptEncoding::ShiftJis, ScriptEncoding::Gbk] {
            assert_eq!(ScriptEncoding::detect(b".end\r\n", primary), primary);
            let bad = b".message 1   \x81\r\n.end\r\n";
            let selected = ScriptEncoding::detect(bad, primary);
            assert!(crate::parse_sc_with_encoding(
                bad,
                &crate::ScOpcodeCatalog::observed_musica(),
                selected
            )
            .is_err());
        }
    }
}
