use std::collections::BTreeMap;

use astra_core::{Hash128, SourceRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommandSourceSpan {
    pub command: SourceRef,
    pub keyword: SourceRef,
    pub source_id: Option<SourceRef>,
    pub attributes: BTreeMap<String, SourceRef>,
    pub arguments: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommandSourceMap {
    pub hash: Hash128,
    pub entries: BTreeMap<String, CommandSourceSpan>,
}

impl Default for CommandSourceMap {
    fn default() -> Self {
        Self {
            hash: Hash128::from_bytes([0; 16]),
            entries: BTreeMap::new(),
        }
    }
}

impl CommandSourceMap {
    pub fn insert(&mut self, id: String, source: SourceRef) -> Option<SourceRef> {
        self.entries
            .insert(
                id,
                CommandSourceSpan {
                    command: source.clone(),
                    keyword: source,
                    source_id: None,
                    attributes: BTreeMap::new(),
                    arguments: Vec::new(),
                },
            )
            .map(|entry| entry.command)
    }

    pub fn get(&self, id: &str) -> Option<&SourceRef> {
        self.entries.get(id).map(|entry| &entry.command)
    }

    pub fn span(&self, id: &str) -> Option<&CommandSourceSpan> {
        self.entries.get(id)
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn refresh_hash(&mut self) {
        let bytes = postcard::to_allocvec(&self.entries)
            .expect("command source-map entries must serialize for hashing");
        self.hash = Hash128::from_blake3(&bytes);
    }
}
