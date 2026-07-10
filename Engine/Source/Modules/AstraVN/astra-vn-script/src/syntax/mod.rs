mod ast;
mod lexer;

pub use ast::*;
pub use lexer::*;

use astra_core::{Diagnostic, SourceRef};

use crate::{AstraSource, CommandRegistry};

#[derive(Debug, Clone)]
pub struct ParsedAstraSource {
    pub path: String,
    pub cst: SyntaxNode,
    pub ast: AstraAst,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_astra_source(path: impl Into<String>, text: &str) -> ParsedAstraSource {
    let path = path.into();
    let (cst, lexical_diagnostics) = build_lossless_cst(&path, text);
    let (ast, mut diagnostics) = parse_typed_ast(&path, &cst);
    diagnostics.extend(lexical_diagnostics);
    let registry = CommandRegistry::default();
    for command in ast.commands() {
        if !registry.is_known(command.keyword()) {
            diagnostics.push(
                Diagnostic::warning(
                    "ASTRA_VN_UNKNOWN_COMMAND",
                    "command is preserved for editing but requires an explicit provider binding",
                )
                .with_source(SourceRef {
                    source: path.clone(),
                    line: command.line() as u32,
                    column: command.column() as u32,
                    length: command.keyword().len() as u32,
                })
                .with_field("command", command.keyword()),
            );
        }
    }
    diagnostics.sort_by(|left, right| {
        left.source
            .as_ref()
            .map(|source| (source.line, source.column))
            .cmp(
                &right
                    .source
                    .as_ref()
                    .map(|source| (source.line, source.column)),
            )
            .then_with(|| left.code.cmp(&right.code))
    });
    ParsedAstraSource {
        path,
        cst,
        ast,
        diagnostics,
    }
}

pub fn parse_astra_sources<I, S>(sources: I) -> Vec<ParsedAstraSource>
where
    I: IntoIterator<Item = S>,
    S: Into<AstraSource>,
{
    sources
        .into_iter()
        .map(Into::into)
        .map(|source: AstraSource| parse_astra_source(source.path, &source.text))
        .collect()
}
