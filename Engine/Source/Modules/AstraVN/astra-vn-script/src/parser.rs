use std::collections::BTreeMap;

use astra_core::{Diagnostic, SourceRef};

use crate::{AstraSource, VnError};

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

pub(crate) fn parse_sources<I>(sources: I) -> Result<Vec<ParsedLine>, VnError>
where
    I: IntoIterator<Item = AstraSource>,
{
    let mut parsed = Vec::new();
    for source in sources {
        for (index, raw_line) in source.text.lines().enumerate() {
            let Some(line) = parse_line(&source.path, index + 1, raw_line)? else {
                continue;
            };
            parsed.push(line);
        }
    }
    Ok(parsed)
}

fn parse_line(source: &str, line_number: usize, raw: &str) -> Result<Option<ParsedLine>, VnError> {
    let trimmed_end = raw.trim_end();
    if trimmed_end.trim().is_empty() {
        return Ok(None);
    }
    let leading_whitespace = trimmed_end
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<Vec<_>>();
    if leading_whitespace.iter().any(|ch| *ch != ' ') {
        return Err(parse_diagnostic(
            source,
            line_number,
            1,
            "ASTRA_VN_PARSE_INDENT",
            "indentation must use spaces only",
        ));
    }
    let indent = leading_whitespace.len();
    if indent % 2 != 0 {
        return Err(parse_diagnostic(
            source,
            line_number,
            1,
            "ASTRA_VN_PARSE_INDENT",
            "indentation must use two-space levels",
        ));
    }
    let column = indent + 1;
    let body = &trimmed_end[indent..];
    let (body, source_id) = split_source_id(body);
    if source_id
        .as_deref()
        .is_some_and(|id| id.is_empty() || id.chars().any(char::is_whitespace))
    {
        return Err(parse_diagnostic(
            source,
            line_number,
            column,
            "ASTRA_VN_SOURCE_ID",
            "source id must be a non-empty identifier without whitespace",
        ));
    }
    let tokens = tokenize(body)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let keyword = tokens[0].clone();
    let mut args = Vec::new();
    let mut attrs = BTreeMap::new();
    let mut index = 1;
    while index < tokens.len() {
        if tokens[index] == "->" {
            if let Some(target) = tokens.get(index + 1) {
                attrs.insert("target".to_string(), target.clone());
                index += 2;
                continue;
            }
            return Err(VnError::diagnostic(
                "ASTRA_VN_PARSE_ARROW",
                "arrow target is missing",
            ));
        }
        if let Some((key, value)) = tokens[index].split_once(':') {
            if attrs.insert(key.to_string(), value.to_string()).is_some() {
                return Err(parse_diagnostic(
                    source,
                    line_number,
                    column,
                    "ASTRA_VN_ATTR_DUPLICATE",
                    format!("attribute {key} is duplicated"),
                ));
            }
        } else {
            args.push(tokens[index].clone());
        }
        index += 1;
    }
    Ok(Some(ParsedLine {
        source: source.to_string(),
        line: line_number,
        column,
        indent,
        keyword,
        args,
        attrs,
        source_id,
    }))
}

fn parse_diagnostic(
    source: &str,
    line: usize,
    column: usize,
    code: &str,
    message: impl Into<String>,
) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message).with_source(SourceRef {
        source: source.to_string(),
        line: line as u32,
        column: column as u32,
        length: 1,
    }))
}

fn split_source_id(body: &str) -> (&str, Option<String>) {
    if let Some((left, right)) = body.rsplit_once("#@id") {
        (left.trim_end(), Some(right.trim().to_string()))
    } else {
        (body, None)
    }
}

fn tokenize(input: &str) -> Result<Vec<String>, VnError> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                quoted = !quoted;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if quoted {
        return Err(VnError::diagnostic(
            "ASTRA_VN_PARSE_QUOTE",
            "quoted token is not closed",
        ));
    }
    if !current.is_empty() {
        out.push(current);
    }
    Ok(out)
}
