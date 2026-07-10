use std::collections::BTreeMap;

use astra_core::SourceRef;

use crate::AstraSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedLine {
    pub source: String,
    pub line: usize,
    pub column: usize,
    pub indent: usize,
    pub keyword: String,
    pub args: Vec<String>,
    pub attrs: BTreeMap<String, String>,
    pub source_id: Option<String>,
}

impl ParsedLine {
    pub fn source_ref(&self) -> SourceRef {
        SourceRef {
            source: self.source.clone(),
            line: self.line as u32,
            column: self.column as u32,
            length: self.keyword.len() as u32,
        }
    }

    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }

    pub fn stable_id(&self) -> String {
        self.source_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}:{}", self.source, self.line, self.keyword))
    }
}

pub(crate) fn lower_sources_from_cst(sources: &[AstraSource]) -> Vec<ParsedLine> {
    sources
        .iter()
        .flat_map(|source| {
            let parsed = crate::parse_astra_source(source.path.clone(), &source.text);
            parsed
                .ast
                .commands()
                .map(|command| ParsedLine {
                    source: source.path.clone(),
                    line: command.line(),
                    column: command.column(),
                    indent: command.indent(),
                    keyword: command.keyword().to_string(),
                    args: command
                        .arguments()
                        .map(|(argument, _)| argument.to_string())
                        .collect(),
                    attrs: command
                        .attributes()
                        .map(|attribute| {
                            (attribute.key().to_string(), attribute.value().to_string())
                        })
                        .collect(),
                    source_id: command.source_id().map(str::to_string),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
