use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{parse_astra_source, AuthoringWorkspace};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEdit {
    /// UTF-8 byte offsets in the specified document version, end exclusive.
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentEdits {
    pub path: String,
    pub version: u64,
    pub edits: Vec<TextEdit>,
}

impl DocumentEdits {
    pub(crate) fn apply_to(&self, source: &str) -> Result<String, EditError> {
        if self.edits.is_empty() {
            return Err(EditError::EmptyBatch);
        }
        let mut ordered = self.edits.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|edit| (edit.start, edit.end));
        let mut previous: Option<&TextEdit> = None;
        let mut size = source.len();
        for edit in &ordered {
            if edit.start > edit.end
                || !source.is_char_boundary(edit.start)
                || !source.is_char_boundary(edit.end)
            {
                return Err(EditError::InvalidRange);
            }
            if previous.is_some_and(|p| p.end > edit.start || p.start == edit.start) {
                return Err(EditError::OverlappingEdits);
            }
            size = size
                .checked_sub(edit.end - edit.start)
                .and_then(|s| s.checked_add(edit.replacement.len()))
                .ok_or(EditError::Limit)?;
            if size > 16 * 1024 * 1024 {
                return Err(EditError::Limit);
            }
            previous = Some(edit);
        }
        let mut output = source.to_string();
        for edit in ordered.into_iter().rev() {
            output.replace_range(edit.start..edit.end, &edit.replacement);
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditBatch {
    pub documents: Vec<DocumentEdits>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    NotOpen,
    AlreadyOpen,
    InvalidPath,
    Conflict,
    InvalidRange,
    OverlappingEdits,
    DuplicateDocument,
    EmptyBatch,
    NoHistory,
    Limit,
    MissingAttribute,
    InvalidValue,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ASTRA_EDITOR_{self:?}")
    }
}
impl std::error::Error for EditError {}

impl AuthoringWorkspace {
    /// Graph, Inspector and Timeline update the existing CST span, preserving trivia.
    pub fn attribute_edit(
        &self,
        path: &str,
        command_id: &str,
        attribute: &str,
        value: &str,
    ) -> Result<EditBatch, EditError> {
        let document = self.document(path)?;
        let parsed = parse_astra_source(path, &document.text);
        let command = parsed
            .ast
            .commands()
            .find(|c| c.source_id() == Some(command_id))
            .ok_or(EditError::MissingAttribute)?;
        let span = command
            .attribute(attribute)
            .ok_or(EditError::MissingAttribute)?
            .value_span;
        let replacement = if value
            .chars()
            .all(|c| c.is_alphanumeric() || "_./-".contains(c))
            && !value.is_empty()
        {
            value.to_string()
        } else {
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        };
        let batch = EditBatch {
            documents: vec![DocumentEdits {
                path: path.to_string(),
                version: document.version,
                edits: vec![TextEdit {
                    start: u32::from(span.start) as usize,
                    end: u32::from(span.end) as usize,
                    replacement,
                }],
            }],
        };
        // Reject values the current grammar cannot represent (including multiline injection).
        let candidate = batch.documents[0].apply_to(&document.text)?;
        let updated = parse_astra_source(path, &candidate);
        if !updated.diagnostics.is_empty()
            || updated
                .ast
                .commands()
                .find(|c| c.source_id() == Some(command_id))
                .and_then(|c| c.attribute(attribute))
                .map(|a| a.value())
                != Some(value)
        {
            return Err(EditError::InvalidValue);
        }
        Ok(batch)
    }
}
