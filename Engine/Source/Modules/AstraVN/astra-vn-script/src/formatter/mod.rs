use crate::{parse_astra_source, VnError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormatOptions {
    pub ensure_final_newline: bool,
}

pub fn format_astra_source(
    path: impl Into<String>,
    source: &str,
    options: FormatOptions,
) -> Result<String, VnError> {
    let path = path.into();
    let parsed = parse_astra_source(path, source);
    if let Some(diagnostic) = parsed.diagnostics.iter().find(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "ASTRA_VN_PARSE_QUOTE" | "ASTRA_VN_PARSE_INDENT" | "ASTRA_VN_SOURCE_ID"
        )
    }) {
        return Err(VnError::Diagnostic(diagnostic.clone()));
    }
    let had_final_newline = source.ends_with('\n') || source.ends_with('\r');
    let mut lines = source.lines().map(str::trim_end).collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let mut formatted = lines.join("\n");
    if had_final_newline || options.ensure_final_newline {
        formatted.push('\n');
    }
    Ok(formatted)
}
