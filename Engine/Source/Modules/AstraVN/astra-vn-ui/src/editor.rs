use std::collections::VecDeque;
use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use thiserror::Error;

pub const MAX_TEXT_INPUT_BYTES: usize = 1 << 20;
pub const MAX_TEXT_INPUT_GRAPHEMES: usize = 65_536;
pub const MAX_TEXT_INPUT_UNDO: usize = 128;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct TextInputError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextInputMode {
    SingleLine,
    MultiLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextCharacterPolicy {
    Printable,
    PrintableWithoutPrivateUse,
    Ascii,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextInputPolicy {
    pub mode: TextInputMode,
    pub characters: TextCharacterPolicy,
    pub max_bytes: usize,
    pub max_graphemes: usize,
}

impl TextInputPolicy {
    pub fn validate(&self) -> Result<(), TextInputError> {
        if self.max_bytes == 0
            || self.max_bytes > MAX_TEXT_INPUT_BYTES
            || self.max_graphemes == 0
            || self.max_graphemes > MAX_TEXT_INPUT_GRAPHEMES
        {
            return Err(TextInputError(
                "ASTRA_VN_UI_TEXT_LIMIT: invalid text input bounds".into(),
            ));
        }
        Ok(())
    }

    fn validate_insert(&self, text: &str) -> Result<(), TextInputError> {
        if self.mode == TextInputMode::SingleLine
            && text
                .chars()
                .any(|character| matches!(character, '\r' | '\n'))
        {
            return Err(TextInputError(
                "ASTRA_VN_UI_TEXT_LINE_BREAK: single-line input rejected a line break".into(),
            ));
        }
        for character in text.chars() {
            let allowed = match self.characters {
                TextCharacterPolicy::Printable => !character.is_control(),
                TextCharacterPolicy::PrintableWithoutPrivateUse => {
                    !character.is_control() && !is_private_use(character)
                }
                TextCharacterPolicy::Ascii => character.is_ascii() && !character.is_control(),
            };
            if !allowed {
                return Err(TextInputError(
                    "ASTRA_VN_UI_TEXT_CHARACTER: character policy rejected input".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImePreeditState {
    pub text: String,
    pub cursor: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct EditSnapshot {
    text: String,
    anchor: usize,
    cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlainTextEditor {
    policy: TextInputPolicy,
    text: String,
    anchor: usize,
    cursor: usize,
    preedit: Option<ImePreeditState>,
    undo: VecDeque<EditSnapshot>,
    redo: VecDeque<EditSnapshot>,
}

impl PlainTextEditor {
    pub fn new(policy: TextInputPolicy, text: String) -> Result<Self, TextInputError> {
        policy.validate()?;
        policy.validate_insert(&text)?;
        validate_size(&policy, &text)?;
        let cursor = text.len();
        Ok(Self {
            policy,
            text,
            anchor: cursor,
            cursor,
            preedit: None,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    pub fn preedit(&self) -> Option<&ImePreeditState> {
        self.preedit.as_ref()
    }

    pub fn set_selection(&mut self, anchor: usize, cursor: usize) -> Result<(), TextInputError> {
        validate_boundary(&self.text, anchor)?;
        validate_boundary(&self.text, cursor)?;
        self.anchor = anchor;
        self.cursor = cursor;
        Ok(())
    }

    pub fn move_graphemes(&mut self, delta: isize, extend: bool) {
        let boundaries = grapheme_boundaries(&self.text);
        let current = boundaries.partition_point(|boundary| *boundary < self.cursor);
        let target = current
            .saturating_add_signed(delta)
            .min(boundaries.len().saturating_sub(1));
        self.cursor = boundaries[target];
        if !extend {
            self.anchor = self.cursor;
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn copy(&self) -> String {
        self.selected_text().to_owned()
    }

    pub fn cut(&mut self) -> Result<String, TextInputError> {
        let copied = self.copy();
        if !copied.is_empty() {
            self.replace_selection("")?;
        }
        Ok(copied)
    }

    pub fn paste(&mut self, text: &str) -> Result<(), TextInputError> {
        self.replace_selection(text)
    }

    pub fn backspace(&mut self) -> Result<(), TextInputError> {
        if self.selection().is_empty() && self.cursor > 0 {
            self.move_graphemes(-1, true);
        }
        self.replace_selection("")
    }

    pub fn delete_forward(&mut self) -> Result<(), TextInputError> {
        if self.selection().is_empty() && self.cursor < self.text.len() {
            self.move_graphemes(1, true);
        }
        self.replace_selection("")
    }

    pub fn set_preedit(
        &mut self,
        text: String,
        cursor: Option<Range<usize>>,
    ) -> Result<(), TextInputError> {
        self.policy.validate_insert(&text)?;
        if text.len() > self.policy.max_bytes {
            return Err(TextInputError(
                "ASTRA_VN_UI_IME_LIMIT: preedit exceeds configured byte limit".into(),
            ));
        }
        if let Some(range) = &cursor {
            if range.start > range.end || range.end > text.len() {
                return Err(TextInputError(
                    "ASTRA_VN_UI_IME_CURSOR: preedit cursor is out of range".into(),
                ));
            }
            validate_boundary(&text, range.start)?;
            validate_boundary(&text, range.end)?;
        }
        self.preedit = (!text.is_empty()).then_some(ImePreeditState { text, cursor });
        Ok(())
    }

    pub fn commit_preedit(&mut self, text: &str) -> Result<(), TextInputError> {
        self.replace_selection(text)?;
        self.preedit = None;
        Ok(())
    }

    pub fn cancel_preedit(&mut self) {
        self.preedit = None;
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop_back() else {
            return false;
        };
        let current = self.snapshot();
        push_bounded(&mut self.redo, current);
        self.restore(snapshot);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop_back() else {
            return false;
        };
        let current = self.snapshot();
        push_bounded(&mut self.undo, current);
        self.restore(snapshot);
        true
    }

    fn replace_selection(&mut self, replacement: &str) -> Result<(), TextInputError> {
        self.policy.validate_insert(replacement)?;
        let selection = self.selection();
        let mut next = String::with_capacity(
            self.text.len() - (selection.end - selection.start) + replacement.len(),
        );
        next.push_str(&self.text[..selection.start]);
        next.push_str(replacement);
        next.push_str(&self.text[selection.end..]);
        validate_size(&self.policy, &next)?;
        let snapshot = self.snapshot();
        self.text = next;
        self.cursor = selection.start + replacement.len();
        self.anchor = self.cursor;
        self.preedit = None;
        push_bounded(&mut self.undo, snapshot);
        self.redo.clear();
        Ok(())
    }

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            cursor: self.cursor,
        }
    }

    fn restore(&mut self, snapshot: EditSnapshot) {
        self.text = snapshot.text;
        self.anchor = snapshot.anchor;
        self.cursor = snapshot.cursor;
        self.preedit = None;
    }
}

fn validate_boundary(text: &str, offset: usize) -> Result<(), TextInputError> {
    if offset > text.len()
        || !text.is_char_boundary(offset)
        || !grapheme_boundaries(text).contains(&offset)
    {
        return Err(TextInputError(
            "ASTRA_VN_UI_TEXT_BOUNDARY: cursor is not on a grapheme boundary".into(),
        ));
    }
    Ok(())
}

fn validate_size(policy: &TextInputPolicy, text: &str) -> Result<(), TextInputError> {
    if text.len() > policy.max_bytes || text.graphemes(true).count() > policy.max_graphemes {
        return Err(TextInputError(
            "ASTRA_VN_UI_TEXT_LIMIT: edit exceeds configured bounds".into(),
        ));
    }
    Ok(())
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    text.grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect()
}

fn push_bounded(queue: &mut VecDeque<EditSnapshot>, snapshot: EditSnapshot) {
    if queue.len() == MAX_TEXT_INPUT_UNDO {
        queue.pop_front();
    }
    queue.push_back(snapshot);
}

fn is_private_use(character: char) -> bool {
    matches!(character as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(mode: TextInputMode) -> TextInputPolicy {
        TextInputPolicy {
            mode,
            characters: TextCharacterPolicy::PrintableWithoutPrivateUse,
            max_bytes: 128,
            max_graphemes: 16,
        }
    }

    #[astra_headless_test::test]
    fn editing_preserves_grapheme_boundaries_and_history() {
        let mut editor =
            PlainTextEditor::new(policy(TextInputMode::MultiLine), "a🇯🇵é".into()).expect("editor");
        editor.move_graphemes(-1, false);
        editor.backspace().expect("backspace");
        assert_eq!(editor.text(), "aé");
        assert!(editor.undo());
        assert_eq!(editor.text(), "a🇯🇵é");
        assert!(editor.redo());
        assert_eq!(editor.text(), "aé");
    }

    #[astra_headless_test::test]
    fn ime_commit_and_character_policy_fail_fast() {
        let mut editor =
            PlainTextEditor::new(policy(TextInputMode::SingleLine), String::new()).expect("editor");
        editor
            .set_preedit("終".into(), Some(0..3))
            .expect("preedit");
        editor.commit_preedit("終").expect("commit");
        assert_eq!(editor.text(), "終");
        assert!(editor.paste("\n").is_err());
        assert_eq!(editor.text(), "終");
    }
}
