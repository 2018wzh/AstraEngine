use std::collections::BTreeSet;

use astra_ui_core::{UiButtonState, UiInputEvent, UiInputEventKind, UiValidationError};
use unicode_segmentation::UnicodeSegmentation;

use crate::{MODIFIER_CONTROL, MODIFIER_META, MODIFIER_SHIFT};

const MAX_UNDO_ENTRIES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCharacterPolicy {
    Any,
    SingleLine,
    Identifier,
}

impl TextCharacterPolicy {
    pub fn parse(value: Option<&str>, multiline: bool) -> Result<Self, UiValidationError> {
        match value.unwrap_or(if multiline { "any" } else { "single_line" }) {
            "any" => Ok(Self::Any),
            "single_line" => Ok(Self::SingleLine),
            "identifier" => Ok(Self::Identifier),
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_TEXT_INPUT_POLICY",
                "text input character policy must be any, single_line, or identifier",
            )),
        }
    }

    fn accepts(self, grapheme: &str) -> bool {
        match self {
            Self::Any => true,
            Self::SingleLine => !grapheme.contains(['\r', '\n']),
            Self::Identifier => grapheme.chars().all(|character| {
                character.is_alphanumeric() || matches!(character, '_' | '-' | '.')
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSnapshot {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInputState {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
    composition: Option<String>,
    undo: Vec<TextSnapshot>,
    redo: Vec<TextSnapshot>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TextInputUpdate {
    pub changed: bool,
    pub submitted: bool,
    pub consumed_sequences: BTreeSet<u64>,
}

impl TextInputState {
    pub fn new(text: String, max_graphemes: usize) -> Result<Self, UiValidationError> {
        if text.graphemes(true).count() > max_graphemes {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_TEXT_INPUT_INITIAL_LIMIT",
                "initial text exceeds max_graphemes",
            ));
        }
        let cursor = text.graphemes(true).count();
        Ok(Self {
            text,
            cursor,
            selection_anchor: None,
            composition: None,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.selection_anchor
            .filter(|anchor| *anchor != self.cursor)
            .map(|anchor| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub fn composition(&self) -> Option<&str> {
        self.composition.as_deref()
    }

    pub fn update(
        &mut self,
        events: &[UiInputEvent],
        multiline: bool,
        max_graphemes: usize,
        policy: TextCharacterPolicy,
    ) -> TextInputUpdate {
        let mut update = TextInputUpdate::default();
        for event in events {
            let consumed = match &event.kind {
                UiInputEventKind::ImePreedit { text, .. } => {
                    self.composition = (!text.is_empty()).then(|| text.clone());
                    true
                }
                UiInputEventKind::ImeCommit { text } => {
                    self.composition = None;
                    update.changed |= self.insert(text, max_graphemes, policy);
                    true
                }
                UiInputEventKind::Keyboard {
                    logical_key,
                    physical_key,
                    state: UiButtonState::Pressed,
                    modifiers,
                    ..
                } => {
                    let command = modifiers & (MODIFIER_CONTROL | MODIFIER_META) != 0;
                    let shift = modifiers & MODIFIER_SHIFT != 0;
                    let key = if logical_key.is_empty() {
                        physical_key.as_str()
                    } else {
                        logical_key.as_str()
                    };
                    match key {
                        "ArrowLeft" => {
                            self.move_cursor(self.cursor.saturating_sub(1), shift);
                            true
                        }
                        "ArrowRight" => {
                            self.move_cursor((self.cursor + 1).min(self.grapheme_count()), shift);
                            true
                        }
                        "Home" => {
                            self.move_cursor(0, shift);
                            true
                        }
                        "End" => {
                            self.move_cursor(self.grapheme_count(), shift);
                            true
                        }
                        "Backspace" => {
                            update.changed |= self.backspace();
                            true
                        }
                        "Delete" => {
                            update.changed |= self.delete();
                            true
                        }
                        "Enter" | "NumpadEnter" if multiline => {
                            update.changed |= self.insert("\n", max_graphemes, policy);
                            true
                        }
                        "Enter" | "NumpadEnter" => {
                            update.submitted = true;
                            true
                        }
                        "a" | "A" | "KeyA" if command => {
                            self.selection_anchor = Some(0);
                            self.cursor = self.grapheme_count();
                            true
                        }
                        "z" | "Z" | "KeyZ" if command && shift => {
                            update.changed |= self.redo();
                            true
                        }
                        "z" | "Z" | "KeyZ" if command => {
                            update.changed |= self.undo();
                            true
                        }
                        "y" | "Y" | "KeyY" if command => {
                            update.changed |= self.redo();
                            true
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            if consumed {
                update.consumed_sequences.insert(event.sequence);
            }
        }
        update
    }

    fn grapheme_count(&self) -> usize {
        self.text.graphemes(true).count()
    }

    fn byte_offset(&self, grapheme: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(grapheme)
            .map_or(self.text.len(), |(offset, _)| offset)
    }

    fn snapshot(&self) -> TextSnapshot {
        TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
        }
    }

    fn push_undo(&mut self) {
        if self.undo.len() == MAX_UNDO_ENTRIES {
            self.undo.remove(0);
        }
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    fn restore(&mut self, snapshot: TextSnapshot) {
        self.text = snapshot.text;
        self.cursor = snapshot.cursor;
        self.selection_anchor = snapshot.selection_anchor;
        self.composition = None;
    }

    fn move_cursor(&mut self, cursor: usize, extend: bool) {
        if extend {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        self.cursor = cursor;
    }

    fn replace_selection(&mut self, replacement: &str) {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let start_byte = self.byte_offset(start);
        let end_byte = self.byte_offset(end);
        self.text.replace_range(start_byte..end_byte, replacement);
        self.cursor = start + replacement.graphemes(true).count();
        self.selection_anchor = None;
    }

    fn insert(&mut self, value: &str, max_graphemes: usize, policy: TextCharacterPolicy) -> bool {
        let filtered = value
            .graphemes(true)
            .filter(|grapheme| policy.accepts(grapheme))
            .collect::<String>();
        if filtered.is_empty() {
            return false;
        }
        let selected = self.selection().map_or(0, |(start, end)| end - start);
        let available = max_graphemes.saturating_sub(self.grapheme_count() - selected);
        let filtered = filtered.graphemes(true).take(available).collect::<String>();
        if filtered.is_empty() {
            return false;
        }
        self.push_undo();
        self.replace_selection(&filtered);
        true
    }

    fn backspace(&mut self) -> bool {
        if self.selection().is_none() && self.cursor == 0 {
            return false;
        }
        self.push_undo();
        if self.selection().is_some() {
            self.replace_selection("");
        } else {
            self.selection_anchor = Some(self.cursor - 1);
            self.replace_selection("");
        }
        true
    }

    fn delete(&mut self) -> bool {
        if self.selection().is_none() && self.cursor == self.grapheme_count() {
            return false;
        }
        self.push_undo();
        if self.selection().is_some() {
            self.replace_selection("");
        } else {
            self.selection_anchor = Some(self.cursor + 1);
            self.replace_selection("");
        }
        true
    }

    fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        if self.redo.len() == MAX_UNDO_ENTRIES {
            self.redo.remove(0);
        }
        self.redo.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        if self.undo.len() == MAX_UNDO_ENTRIES {
            self.undo.remove(0);
        }
        self.undo.push(self.snapshot());
        self.restore(snapshot);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_ui_core::UiInputEvent;

    fn commit(sequence: u64, text: &str) -> UiInputEvent {
        UiInputEvent {
            sequence,
            kind: UiInputEventKind::ImeCommit { text: text.into() },
        }
    }

    #[astra_headless_test::test]
    fn edits_by_grapheme_and_enforces_limit() {
        let mut state = TextInputState::new("你".into(), 2).expect("state");
        let update = state.update(
            &[commit(1, "好界")],
            false,
            2,
            TextCharacterPolicy::SingleLine,
        );
        assert!(update.changed);
        assert_eq!(state.text(), "你好");
        assert_eq!(state.cursor(), 2);
        assert!(update.consumed_sequences.contains(&1));
    }
}
