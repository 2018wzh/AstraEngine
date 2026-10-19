use crate::{CoreError, ScCommand};
use astra_core::Hash256;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Original DLL's 131-rolling identity. Embedded NUL bytes in table records are skipped.
pub fn musica_display_hash(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .filter(|byte| **byte != 0)
        .fold(0u32, |hash, byte| {
            hash.wrapping_mul(131).wrapping_add(u32::from(*byte))
        })
        & 0x7fff_ffff
}
/// The display endpoint receives a C string, so runtime input ends at its first NUL.
pub fn musica_display_cstr_hash(bytes: &[u8]) -> u32 {
    musica_display_hash(bytes.split(|byte| *byte == 0).next().unwrap_or_default())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranslationMode {
    #[default]
    Replace,
    Overlay,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    source_hash: u32,
    source_sha256: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    source_encoding: String,
    hash_collision_policy: String,
    names: Vec<Record>,
    dialogue: Vec<Record>,
}
pub struct MusicaTranslationTable {
    identity: Hash256,
    document: Document,
    names: BTreeMap<u32, usize>,
    dialogue: BTreeMap<u32, usize>,
}
impl MusicaTranslationTable {
    pub fn from_json(bytes: &[u8]) -> Result<Self, CoreError> {
        let document: Document = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if document.version != 2
            || !matches!(document.source_encoding.as_str(), "gbk" | "cp932")
            || document.hash_collision_policy != "first_record"
        {
            return Err(invalid());
        }
        let index = |records: &[Record]| -> Result<BTreeMap<u32, usize>, CoreError> {
            let mut indices = BTreeMap::new();
            for (index, record) in records.iter().enumerate() {
                if record.source_hash > 0x7fff_ffff
                    || record.source_sha256.len() != 64
                    || !record
                        .source_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                    || record.text.len() > 64 * 1024
                    || record.text.contains('\0')
                {
                    return Err(invalid());
                }
                // The SHA is audit metadata, never a disambiguator. DLL unordered_map::insert
                // leaves the FIRST record in effect even for a real hash collision.
                indices.entry(record.source_hash).or_insert(index);
            }
            Ok(indices)
        };
        let names = index(&document.names)?;
        let dialogue = index(&document.dialogue)?;
        Ok(Self {
            identity: Hash256::from_sha256(bytes),
            document,
            names,
            dialogue,
        })
    }
    pub fn load(root: &Path, path: &str) -> Result<Self, CoreError> {
        Self::from_json(&crate::resources::read_resource(
            root,
            path,
            64 * 1024 * 1024,
        )?)
    }
    pub fn identity(&self) -> Hash256 {
        self.identity
    }
    /// Names precede and bypass dialogue at the original display endpoint.
    pub fn lookup(&self, hash: u32) -> Option<(&str, bool)> {
        if let Some(index) = self.names.get(&hash) {
            return Some((&self.document.names[*index].text, true));
        }
        self.dialogue
            .get(&hash)
            .map(|index| (self.document.dialogue[*index].text.as_str(), false))
    }
}
fn invalid() -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_MUSICA_TRANSLATION_TABLE",
        "translation table must use schema v2 ordered first_record UTF-8 records",
    )
}

impl ScCommand {
    /// Identity comes from the original operand byte tokens BEFORE any encoding decode.
    pub(crate) fn message_display_hashes(&self) -> Option<(u32, Option<u32>)> {
        if self.opcode != "message" {
            return None;
        }
        let mut tokens = self.raw_operands.split(|byte| matches!(byte, b' ' | b'\t'));
        tokens.next()?;
        tokens.next()?;
        let speaker = tokens.next()?;
        let speaker = (!speaker.is_empty()).then(|| musica_display_cstr_hash(speaker));
        let first = tokens.next()?;
        let mut hash = 0u32;
        let mut ended = false;
        for (index, token) in std::iter::once(first).chain(tokens).enumerate() {
            if ended {
                break;
            }
            if index != 0 {
                hash = hash.wrapping_mul(131).wrapping_add(u32::from(b' '));
            }
            for byte in token {
                if *byte == 0 {
                    ended = true;
                    break;
                }
                hash = hash.wrapping_mul(131).wrapping_add(u32::from(*byte));
            }
        }
        Some((hash & 0x7fff_ffff, speaker))
    }
    pub(crate) fn choice_display_hashes(&self) -> Vec<u32> {
        if self.opcode != "select" {
            return Vec::new();
        }
        self.raw_operands
            .split(|byte| matches!(byte, b' ' | b'\t'))
            .filter(|token| !token.is_empty())
            .map(|token| {
                musica_display_cstr_hash(
                    token.split(|byte| *byte == b':').next().unwrap_or_default(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(hash: u32, text: &str, sha: &str) -> serde_json::Value {
        serde_json::json!({"source_hash":hash,"source_sha256":sha.repeat(64),"text":text})
    }
    #[test]
    fn raw_gbk_identity_is_not_utf8_and_cstrings_stop_at_nul() {
        assert_ne!(
            musica_display_hash(&[0xd6, 0xd0]),
            musica_display_hash("中".as_bytes())
        );
        assert_eq!(musica_display_hash(b"a\0b"), musica_display_hash(b"ab"));
        assert_eq!(musica_display_cstr_hash(b"a\0b"), musica_display_hash(b"a"));
        let script = crate::parse_sc_with_encoding(
            b".message 1  \xd6\xd0 \xd6\xd0\r\n.end\r\n",
            &crate::ScOpcodeCatalog::observed_musica(),
            crate::ScriptEncoding::Gbk,
        )
        .unwrap();
        let crate::ScLineKind::Command { command } = &script.lines[0].kind else {
            panic!()
        };
        assert_eq!(
            command.message_display_hashes(),
            Some((
                musica_display_hash(&[0xd6, 0xd0]),
                Some(musica_display_hash(&[0xd6, 0xd0]))
            ))
        );
    }
    #[test]
    fn first_hash_wins_including_different_audit_sha_and_names_precedence() {
        let bytes = serde_json::to_vec(&serde_json::json!({"version":2,"source_encoding":"gbk","hash_collision_policy":"first_record", "names":[record(7,"first","0"),record(7,"duplicate","0"),record(7,"collision","1")], "dialogue":[record(7,"dialogue","2"),record(8,"first dialogue","3"),record(8,"later","4")]})).unwrap();
        let table = MusicaTranslationTable::from_json(&bytes).unwrap();
        assert_eq!(table.lookup(7), Some(("first", true)));
        assert_eq!(table.lookup(8), Some(("first dialogue", false)));
        assert_eq!(table.document.names.len(), 3);
        assert_eq!(table.document.dialogue.len(), 3);
        assert_eq!(table.lookup(9), None);
        assert!(MusicaTranslationTable::from_json(b"{\"version\":1}").is_err());
    }
    #[test]
    fn hashes_authored_controls_before_display_markup_and_keeps_source_bytes() {
        let bytes = b".message 1  Speaker A\\a\\v\r\n.end\r\n";
        let script = crate::parse_sc(bytes, &crate::ScOpcodeCatalog::observed_musica()).unwrap();
        let crate::ScLineKind::Command { command } = &script.lines[0].kind else {
            panic!()
        };
        assert_eq!(
            command.message_display_hashes().unwrap().0,
            musica_display_hash(b"A\\a\\v")
        );
        assert_ne!(
            command.message_display_hashes().unwrap().0,
            musica_display_hash(b"A")
        );
        assert_eq!(crate::encode_sc(&script).unwrap(), bytes);
        assert_eq!(musica_display_hash(&[1, 132]), musica_display_hash(&[2, 1]));
    }
}
