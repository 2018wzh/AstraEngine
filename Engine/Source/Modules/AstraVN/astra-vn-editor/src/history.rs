use crate::{document::MAX_DOCUMENT_BYTES, EditError, TextEdit};
use std::borrow::Cow;

pub(crate) struct PreparedEdit {
    pub text: String,
    pub inverse: Vec<TextEdit>,
    pub reserved_bytes: usize,
}

fn ordered<'a>(
    source: &str,
    edits: &'a [TextEdit],
) -> Result<(Vec<&'a TextEdit>, usize), EditError> {
    if edits.is_empty() {
        return Err(EditError::EmptyBatch);
    }
    let mut ordered: Vec<_> = edits.iter().collect();
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
        if previous.is_some_and(|prior| prior.end > edit.start || prior.start == edit.start) {
            return Err(EditError::OverlappingEdits);
        }
        size = size
            .checked_sub(edit.end - edit.start)
            .and_then(|size| size.checked_add(edit.replacement.len()))
            .ok_or(EditError::Limit)?;
        if size > MAX_DOCUMENT_BYTES {
            return Err(EditError::Limit);
        }
        previous = Some(edit);
    }
    Ok((ordered, size))
}

fn apply_ordered(source: &str, ordered: &[&TextEdit], size: usize) -> String {
    let mut text = String::with_capacity(size);
    let mut cursor = 0;
    for edit in ordered {
        text.push_str(&source[cursor..edit.start]);
        text.push_str(&edit.replacement);
        cursor = edit.end;
    }
    text.push_str(&source[cursor..]);
    text
}

pub(crate) fn apply_text(source: &str, edits: &[TextEdit]) -> Result<String, EditError> {
    let (ordered, size) = ordered(source, edits)?;
    Ok(apply_ordered(source, &ordered, size))
}

/// The differing span in each string, excluding equal UTF-8 prefixes/suffixes.
fn difference(before: &str, after: &str) -> (usize, usize, usize) {
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = before[prefix..]
        .bytes()
        .rev()
        .zip(after[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    (prefix, before.len() - suffix, after.len() - suffix)
}

pub(crate) fn prepare(
    source: &str,
    edits: Cow<'_, [TextEdit]>,
    budget: usize,
) -> Result<PreparedEdit, EditError> {
    let (ordered, size) = ordered(source, &edits)?;
    let mut inverse: Vec<TextEdit> = Vec::new();
    let mut reserved_bytes = 0usize;
    let mut source_cursor = 0;
    let mut target_cursor = 0;
    for edit in &ordered {
        target_cursor += edit.start - source_cursor;
        let before = &source[edit.start..edit.end];
        let (prefix, before_end, after_end) = difference(before, &edit.replacement);
        let removed = &before[prefix..before_end];
        // Reserve either direction, including String/Vec growth. Undo/redo can
        // move an entry without enlarging the admitted history budget.
        let text_bytes = removed.len().max(after_end - prefix).max(8);
        let charge = text_bytes
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(4 * std::mem::size_of::<TextEdit>()))
            .ok_or(EditError::HistoryLimit)?;
        reserved_bytes = reserved_bytes
            .checked_add(charge)
            .ok_or(EditError::HistoryLimit)?;
        if reserved_bytes > budget {
            return Err(EditError::HistoryLimit);
        }
        let start = target_cursor + prefix;
        let end = target_cursor + after_end;
        if let Some(previous) = inverse.last_mut().filter(|previous| previous.end == start) {
            // Adjacent deletions become insertions at one position; combine
            // them so undo still satisfies the non-overlap input contract.
            previous.end = end;
            previous.replacement.push_str(removed);
        } else {
            inverse.push(TextEdit {
                start,
                end,
                replacement: removed.into(),
            });
        }
        target_cursor += edit.replacement.len();
        source_cursor = edit.end;
    }
    let whole = ordered.len() == 1 && ordered[0].start == 0 && ordered[0].end == source.len();
    let text = if whole && matches!(edits, Cow::Owned(_)) {
        drop(ordered);
        match edits {
            Cow::Owned(mut edits) => {
                edits
                    .pop()
                    .expect("one replacement checked above")
                    .replacement
            }
            Cow::Borrowed(_) => unreachable!("owned replacement checked above"),
        }
    } else {
        apply_ordered(source, &ordered, size)
    };
    Ok(PreparedEdit {
        text,
        inverse,
        reserved_bytes,
    })
}
