use std::collections::BTreeMap;

use astra_core::{Diagnostic, SourceRef};
use rowan::NodeOrToken;
use text_size::TextSize;

use super::{SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSpan {
    pub start: TextSize,
    pub end: TextSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstAttribute {
    key: String,
    value: String,
    pub key_span: TextSpan,
    pub value_span: TextSpan,
}

impl AstAttribute {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstCommand {
    keyword: String,
    keyword_span: TextSpan,
    line: usize,
    column: usize,
    indent: usize,
    source_id: Option<String>,
    source_id_span: Option<TextSpan>,
    arguments: Vec<(String, TextSpan)>,
    attributes: BTreeMap<String, AstAttribute>,
}

impl AstCommand {
    pub fn keyword(&self) -> &str {
        &self.keyword
    }

    pub fn keyword_span(&self) -> TextSpan {
        self.keyword_span
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn column(&self) -> usize {
        self.column
    }

    pub fn indent(&self) -> usize {
        self.indent
    }

    pub fn source_id(&self) -> Option<&str> {
        self.source_id.as_deref()
    }

    pub fn source_id_span(&self) -> Option<TextSpan> {
        self.source_id_span
    }

    pub fn arguments(&self) -> impl Iterator<Item = (&str, TextSpan)> {
        self.arguments
            .iter()
            .map(|(value, span)| (value.as_str(), *span))
    }

    pub fn attribute(&self, key: &str) -> Option<&AstAttribute> {
        self.attributes.get(key)
    }

    pub fn attributes(&self) -> impl Iterator<Item = &AstAttribute> {
        self.attributes.values()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AstraAst {
    commands: Vec<AstCommand>,
}

impl AstraAst {
    pub fn commands(&self) -> impl Iterator<Item = &AstCommand> {
        self.commands.iter()
    }
}

pub(crate) fn parse_typed_ast(path: &str, cst: &SyntaxNode) -> (AstraAst, Vec<Diagnostic>) {
    let mut commands = Vec::new();
    let mut diagnostics = Vec::new();
    let source = cst.to_string();
    for node in cst.descendants().filter(|node| {
        matches!(
            node.kind(),
            SyntaxKind::Story | SyntaxKind::State | SyntaxKind::Scene | SyntaxKind::Command
        )
    }) {
        let raw = node
            .children_with_tokens()
            .filter_map(|element| match element {
                NodeOrToken::Token(token) => Some(token.text().to_string()),
                NodeOrToken::Node(_) => None,
            })
            .collect::<String>();
        let offset = u32::from(node.text_range().start()) as usize;
        let line = source[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let command_line = raw.lines().next().unwrap_or_default();
        parse_line(
            path,
            command_line,
            line,
            offset,
            &mut commands,
            &mut diagnostics,
        );
    }
    (AstraAst { commands }, diagnostics)
}

fn parse_line(
    path: &str,
    raw: &str,
    line: usize,
    offset: usize,
    commands: &mut Vec<AstCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if raw.trim().is_empty()
        || raw.trim_start().starts_with('#') && !raw.trim_start().starts_with("#@id")
    {
        return;
    }
    let indent = raw.bytes().take_while(|byte| *byte == b' ').count();
    if raw[..raw.len().min(indent + 1)].contains('\t') || indent % 2 != 0 {
        diagnostics.push(diagnostic(
            path,
            line,
            1,
            "ASTRA_VN_PARSE_INDENT",
            "indentation must use two-space levels",
        ));
    }
    let body = &raw[indent..];
    let mut cursor = 0_usize;
    let Some((keyword, keyword_local)) =
        next_token(body, &mut cursor, diagnostics, path, line, indent)
    else {
        return;
    };
    let mut command = AstCommand {
        keyword,
        keyword_span: span(
            offset + indent + keyword_local.start,
            offset + indent + keyword_local.end,
        ),
        line,
        column: indent + 1,
        indent,
        source_id: None,
        source_id_span: None,
        arguments: Vec::new(),
        attributes: BTreeMap::new(),
    };
    while let Some((token, token_span)) =
        next_token(body, &mut cursor, diagnostics, path, line, indent)
    {
        if token == "#@id" {
            if let Some((id, id_span)) =
                next_token(body, &mut cursor, diagnostics, path, line, indent)
            {
                command.source_id = Some(id);
                command.source_id_span = Some(span(
                    offset + indent + id_span.start,
                    offset + indent + id_span.end,
                ));
            } else {
                diagnostics.push(diagnostic(
                    path,
                    line,
                    indent + token_span.start + 1,
                    "ASTRA_VN_SOURCE_ID",
                    "source id is missing",
                ));
            }
            break;
        }
        if token.starts_with('#') {
            break;
        }
        if token == "->" {
            let target_cursor = cursor;
            if let Some((target, target_span)) =
                next_token(body, &mut cursor, diagnostics, path, line, indent)
            {
                if target.starts_with('#') {
                    cursor = target_cursor;
                    diagnostics.push(diagnostic(
                        path,
                        line,
                        indent + token_span.start + 1,
                        "ASTRA_VN_PARSE_ARROW",
                        "route arrow target is missing",
                    ));
                } else {
                    command.attributes.insert(
                        "target".into(),
                        AstAttribute {
                            key: "target".into(),
                            value: target,
                            key_span: span(
                                offset + indent + token_span.start,
                                offset + indent + token_span.end,
                            ),
                            value_span: span(
                                offset + indent + target_span.start,
                                offset + indent + target_span.end,
                            ),
                        },
                    );
                }
            } else {
                diagnostics.push(diagnostic(
                    path,
                    line,
                    indent + token_span.start + 1,
                    "ASTRA_VN_PARSE_ARROW",
                    "route arrow target is missing",
                ));
            }
            continue;
        }
        if let Some(colon) = token.find(':') {
            let key = &token[..colon];
            let value = unquote(&token[colon + 1..]);
            let attribute = AstAttribute {
                key: key.to_string(),
                value,
                key_span: span(
                    offset + indent + token_span.start,
                    offset + indent + token_span.start + colon,
                ),
                value_span: span(
                    offset + indent + token_span.start + colon + 1,
                    offset + indent + token_span.end,
                ),
            };
            if command
                .attributes
                .insert(key.to_string(), attribute)
                .is_some()
            {
                diagnostics.push(diagnostic(
                    path,
                    line,
                    indent + token_span.start + 1,
                    "ASTRA_VN_ATTR_DUPLICATE",
                    "attribute is duplicated",
                ));
            }
        } else {
            command.arguments.push((
                unquote(&token),
                span(
                    offset + indent + token_span.start,
                    offset + indent + token_span.end,
                ),
            ));
        }
    }
    commands.push(command);
}

fn next_token(
    body: &str,
    cursor: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    line: usize,
    indent: usize,
) -> Option<(String, std::ops::Range<usize>)> {
    while body
        .as_bytes()
        .get(*cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *cursor += 1;
    }
    if *cursor >= body.len() {
        return None;
    }
    let start = *cursor;
    let mut quoted = false;
    let mut escaped = false;
    while *cursor < body.len() {
        let byte = body.as_bytes()[*cursor];
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte.is_ascii_whitespace() && !quoted {
            break;
        }
        *cursor += 1;
    }
    if quoted {
        diagnostics.push(diagnostic(
            path,
            line,
            indent + start + 1,
            "ASTRA_VN_PARSE_QUOTE",
            "quoted token is not closed",
        ));
    }
    Some((body[start..*cursor].to_string(), start..*cursor))
}

fn unquote(value: &str) -> String {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                output.push(escaped);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn span(start: usize, end: usize) -> TextSpan {
    TextSpan {
        start: TextSize::from(start as u32),
        end: TextSize::from(end as u32),
    }
}

fn diagnostic(path: &str, line: usize, column: usize, code: &str, message: &str) -> Diagnostic {
    Diagnostic::warning(code, message).with_source(SourceRef {
        source: path.to_string(),
        line: line as u32,
        column: column as u32,
        length: 1,
    })
}
