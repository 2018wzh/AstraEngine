use astra_core::Diagnostic;

use crate::{parse_astra_source, ParsedAstraSource, TextSpan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptSymbol {
    pub id: String,
    pub kind: String,
    pub span: TextSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptHover {
    pub span: TextSpan,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    pub kind: String,
    pub span: TextSpan,
}

#[derive(Debug, Clone)]
pub struct ScriptLanguageService {
    parsed: ParsedAstraSource,
}

impl ScriptLanguageService {
    pub fn new(path: impl Into<String>, source: &str) -> Self {
        Self {
            parsed: parse_astra_source(path, source),
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.parsed.diagnostics
    }

    pub fn symbols(&self) -> Vec<ScriptSymbol> {
        self.parsed
            .ast
            .commands()
            .filter_map(|command| {
                Some(ScriptSymbol {
                    id: command.source_id()?.to_string(),
                    kind: command.keyword().to_string(),
                    span: command.source_id_span()?,
                })
            })
            .collect()
    }

    pub fn definition(&self, id: &str) -> Option<TextSpan> {
        self.symbols()
            .into_iter()
            .find(|symbol| symbol.id == id)
            .map(|symbol| symbol.span)
    }

    pub fn references(&self, id: &str) -> Vec<TextSpan> {
        self.parsed
            .ast
            .commands()
            .flat_map(|command| {
                command.arguments().chain(
                    command
                        .attributes()
                        .map(|attribute| (attribute.value(), attribute.value_span)),
                )
            })
            .filter_map(|(value, span)| (value == id).then_some(span))
            .collect()
    }

    pub fn hover(&self, offset: u32) -> Option<ScriptHover> {
        let offset = text_size::TextSize::from(offset);
        self.parsed.ast.commands().find_map(|command| {
            let span = command.keyword_span();
            (span.start <= offset && offset < span.end).then(|| ScriptHover {
                span,
                markdown: format!("Astra command `{}`", command.keyword()),
            })
        })
    }

    pub fn semantic_tokens(&self) -> Vec<SemanticToken> {
        let mut tokens = Vec::new();
        for command in self.parsed.ast.commands() {
            tokens.push(SemanticToken {
                kind: "keyword".to_string(),
                span: command.keyword_span(),
            });
            if let Some(span) = command.source_id_span() {
                tokens.push(SemanticToken {
                    kind: "source_id".to_string(),
                    span,
                });
            }
            tokens.extend(command.attributes().flat_map(|attribute| {
                [
                    SemanticToken {
                        kind: "attribute".to_string(),
                        span: attribute.key_span,
                    },
                    SemanticToken {
                        kind: "value".to_string(),
                        span: attribute.value_span,
                    },
                ]
            }));
        }
        tokens
    }
}
