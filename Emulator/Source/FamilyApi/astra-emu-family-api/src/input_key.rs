//! Canonical input key vocabulary for the legacy family ABI (v5).
//!
//! Starting with `astra.emu.family_abi.v5`, `LegacyInputEdge.control` carries a
//! canonical key name instead of a semantic action. The semantic vocabulary
//! (`confirm`/`cancel`/`up`/...) is deprecated: the Manager's input mapping layer
//! translates every device (keyboard, gamepad) into these key names, and each
//! family injects the named key directly into its engine. Pointer and wheel
//! controls are not key names and are preserved unchanged.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Non-key input controls that keep their pre-v5 spelling.
pub const POINTER_CONTROLS: [&str; 5] = [
    "pointer.x",
    "pointer.y",
    "pointer.primary",
    "pointer.secondary",
    "wheel",
];

/// A canonical input key recognized by the legacy family ABI.
///
/// Named keys serialize to their lowercase snake_case name; `Character` and
/// `Function` serialize as `character:<c>` and `function:<n>` respectively.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum InputKey {
    Enter,
    Escape,
    Space,
    Tab,
    Backspace,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    Shift,
    Control,
    Alt,
    /// A single lowercase alphanumeric character (`a`-`z`, `0`-`9`).
    Character(char),
    /// A function key (`1`-`12`).
    Function(u8),
}

impl InputKey {
    /// The canonical key-name spelling of this key.
    pub fn as_str(&self) -> String {
        match self {
            InputKey::Enter => "enter".to_owned(),
            InputKey::Escape => "escape".to_owned(),
            InputKey::Space => "space".to_owned(),
            InputKey::Tab => "tab".to_owned(),
            InputKey::Backspace => "backspace".to_owned(),
            InputKey::ArrowUp => "arrow_up".to_owned(),
            InputKey::ArrowDown => "arrow_down".to_owned(),
            InputKey::ArrowLeft => "arrow_left".to_owned(),
            InputKey::ArrowRight => "arrow_right".to_owned(),
            InputKey::PageUp => "page_up".to_owned(),
            InputKey::PageDown => "page_down".to_owned(),
            InputKey::Home => "home".to_owned(),
            InputKey::End => "end".to_owned(),
            InputKey::Insert => "insert".to_owned(),
            InputKey::Delete => "delete".to_owned(),
            InputKey::Shift => "shift".to_owned(),
            InputKey::Control => "control".to_owned(),
            InputKey::Alt => "alt".to_owned(),
            InputKey::Character(character) => format!("character:{character}"),
            InputKey::Function(index) => format!("function:{index}"),
        }
    }
}

/// Parse a canonical key name into an [`InputKey`].
///
/// Returns `None` for anything that is not a valid key name, including the
/// deprecated semantic actions and pointer/wheel controls.
pub fn parse_input_key(name: &str) -> Option<InputKey> {
    let named = match name {
        "enter" => InputKey::Enter,
        "escape" => InputKey::Escape,
        "space" => InputKey::Space,
        "tab" => InputKey::Tab,
        "backspace" => InputKey::Backspace,
        "arrow_up" => InputKey::ArrowUp,
        "arrow_down" => InputKey::ArrowDown,
        "arrow_left" => InputKey::ArrowLeft,
        "arrow_right" => InputKey::ArrowRight,
        "page_up" => InputKey::PageUp,
        "page_down" => InputKey::PageDown,
        "home" => InputKey::Home,
        "end" => InputKey::End,
        "insert" => InputKey::Insert,
        "delete" => InputKey::Delete,
        "shift" => InputKey::Shift,
        "control" => InputKey::Control,
        "alt" => InputKey::Alt,
        _ => {
            if let Some(character) = name.strip_prefix("character:") {
                let mut chars = character.chars();
                let candidate = chars.next()?;
                if chars.next().is_some()
                    || !candidate.is_ascii_lowercase() && !candidate.is_ascii_digit()
                {
                    return None;
                }
                InputKey::Character(candidate)
            } else {
                let index = name.strip_prefix("function:")?;
                let index: u8 = index.parse().ok()?;
                if !(1..=12).contains(&index) {
                    return None;
                }
                InputKey::Function(index)
            }
        }
    };
    Some(named)
}

/// Normalize a key name to its canonical spelling, rejecting invalid names.
pub fn normalize_input_key(name: &str) -> Option<String> {
    parse_input_key(name).map(|key| key.as_str())
}

/// Whether `name` is one of the preserved pointer/wheel controls.
pub fn is_pointer_control(name: &str) -> bool {
    POINTER_CONTROLS.contains(&name)
}

/// Whether `name` is a valid `LegacyInputEdge.control` value: either a canonical
/// key name or a preserved pointer/wheel control.
pub fn is_valid_input_control(name: &str) -> bool {
    parse_input_key(name).is_some() || is_pointer_control(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_round_trip_through_canonical_spellings() {
        for key in [
            InputKey::Enter,
            InputKey::Escape,
            InputKey::Space,
            InputKey::Tab,
            InputKey::Backspace,
            InputKey::ArrowUp,
            InputKey::ArrowDown,
            InputKey::ArrowLeft,
            InputKey::ArrowRight,
            InputKey::PageUp,
            InputKey::PageDown,
            InputKey::Home,
            InputKey::End,
            InputKey::Insert,
            InputKey::Delete,
            InputKey::Shift,
            InputKey::Control,
            InputKey::Alt,
        ] {
            assert_eq!(parse_input_key(&key.as_str()), Some(key));
        }
    }

    #[test]
    fn character_and_function_keys_round_trip() {
        assert_eq!(
            parse_input_key("character:a"),
            Some(InputKey::Character('a'))
        );
        assert_eq!(
            parse_input_key("character:z"),
            Some(InputKey::Character('z'))
        );
        assert_eq!(
            parse_input_key("character:0"),
            Some(InputKey::Character('0'))
        );
        assert_eq!(
            parse_input_key("character:9"),
            Some(InputKey::Character('9'))
        );
        assert_eq!(parse_input_key("function:1"), Some(InputKey::Function(1)));
        assert_eq!(parse_input_key("function:12"), Some(InputKey::Function(12)));
        assert_eq!(InputKey::Character('q').as_str(), "character:q");
        assert_eq!(InputKey::Function(7).as_str(), "function:7");
    }

    #[test]
    fn invalid_key_names_are_rejected() {
        for invalid in [
            "",
            "confirm",
            "cancel",
            "up",
            "down",
            "Enter",
            "character:",
            "character:ab",
            "character:A",
            "character:-",
            "function:0",
            "function:13",
            "function:",
            "function:x",
            "unknown_key",
            "pointer.x",
            "wheel",
        ] {
            assert_eq!(
                parse_input_key(invalid),
                None,
                "{invalid} should be rejected"
            );
        }
    }

    #[test]
    fn pointer_controls_are_recognized_but_not_keys() {
        for control in POINTER_CONTROLS {
            assert!(is_pointer_control(control));
            assert!(!is_pointer_control("bogus"));
            assert_eq!(parse_input_key(control), None);
            assert!(is_valid_input_control(control));
        }
    }

    #[test]
    fn valid_input_control_accepts_keys_and_pointers_only() {
        assert!(is_valid_input_control("enter"));
        assert!(is_valid_input_control("character:b"));
        assert!(is_valid_input_control("function:3"));
        assert!(is_valid_input_control("wheel"));
        assert!(!is_valid_input_control("confirm"));
        assert!(!is_valid_input_control(""));
        assert!(!is_valid_input_control("bogus"));
    }

    #[test]
    fn normalize_returns_canonical_spelling() {
        assert_eq!(normalize_input_key("enter").as_deref(), Some("enter"));
        assert_eq!(
            normalize_input_key("character:c").as_deref(),
            Some("character:c")
        );
        assert_eq!(normalize_input_key("confirm"), None);
    }
}
