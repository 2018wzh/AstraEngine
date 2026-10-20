//! Configurable device-to-key input mapping.
//!
//! The Manager owns a generic remapping layer: every device input (gamepad
//! button, stick direction, trigger) is translated to a canonical key name
//! before it is passed to the active Family. [`InputMapping`] is the
//! persisted, user-editable mapping; [`default_vn_preset`] provides a
//! general-purpose visual-novel layout for engines without native gamepad
//! support.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A mappable gamepad input.
///
/// Covers face buttons, start/select, the directional pad, the four left-stick
/// directions (resolved with hysteresis), shoulders and triggers. Stick
/// directions and triggers are not physical buttons but are treated as
/// mappable inputs so the whole device can be remapped uniformly.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GamepadInput {
    South,
    East,
    North,
    West,
    Start,
    Select,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    LeftThumb,
    RightThumb,
}

impl GamepadInput {
    /// All mappable inputs in a stable order for the settings UI.
    pub const DISPLAY_ORDER: [GamepadInput; 20] = [
        GamepadInput::South,
        GamepadInput::East,
        GamepadInput::North,
        GamepadInput::West,
        GamepadInput::Start,
        GamepadInput::Select,
        GamepadInput::DpadUp,
        GamepadInput::DpadDown,
        GamepadInput::DpadLeft,
        GamepadInput::DpadRight,
        GamepadInput::LeftStickUp,
        GamepadInput::LeftStickDown,
        GamepadInput::LeftStickLeft,
        GamepadInput::LeftStickRight,
        GamepadInput::LeftShoulder,
        GamepadInput::RightShoulder,
        GamepadInput::LeftTrigger,
        GamepadInput::RightTrigger,
        GamepadInput::LeftThumb,
        GamepadInput::RightThumb,
    ];

    /// The stable snake_case identifier used by the settings UI.
    pub fn as_str(self) -> &'static str {
        match self {
            GamepadInput::South => "south",
            GamepadInput::East => "east",
            GamepadInput::North => "north",
            GamepadInput::West => "west",
            GamepadInput::Start => "start",
            GamepadInput::Select => "select",
            GamepadInput::DpadUp => "dpad_up",
            GamepadInput::DpadDown => "dpad_down",
            GamepadInput::DpadLeft => "dpad_left",
            GamepadInput::DpadRight => "dpad_right",
            GamepadInput::LeftStickUp => "left_stick_up",
            GamepadInput::LeftStickDown => "left_stick_down",
            GamepadInput::LeftStickLeft => "left_stick_left",
            GamepadInput::LeftStickRight => "left_stick_right",
            GamepadInput::LeftShoulder => "left_shoulder",
            GamepadInput::RightShoulder => "right_shoulder",
            GamepadInput::LeftTrigger => "left_trigger",
            GamepadInput::RightTrigger => "right_trigger",
            GamepadInput::LeftThumb => "left_thumb",
            GamepadInput::RightThumb => "right_thumb",
        }
    }

    /// Parse a settings identifier into a gamepad input.
    pub fn parse(value: &str) -> Option<GamepadInput> {
        match value {
            "south" => Some(GamepadInput::South),
            "east" => Some(GamepadInput::East),
            "north" => Some(GamepadInput::North),
            "west" => Some(GamepadInput::West),
            "start" => Some(GamepadInput::Start),
            "select" => Some(GamepadInput::Select),
            "dpad_up" => Some(GamepadInput::DpadUp),
            "dpad_down" => Some(GamepadInput::DpadDown),
            "dpad_left" => Some(GamepadInput::DpadLeft),
            "dpad_right" => Some(GamepadInput::DpadRight),
            "left_stick_up" => Some(GamepadInput::LeftStickUp),
            "left_stick_down" => Some(GamepadInput::LeftStickDown),
            "left_stick_left" => Some(GamepadInput::LeftStickLeft),
            "left_stick_right" => Some(GamepadInput::LeftStickRight),
            "left_shoulder" => Some(GamepadInput::LeftShoulder),
            "right_shoulder" => Some(GamepadInput::RightShoulder),
            "left_trigger" => Some(GamepadInput::LeftTrigger),
            "right_trigger" => Some(GamepadInput::RightTrigger),
            "left_thumb" => Some(GamepadInput::LeftThumb),
            "right_thumb" => Some(GamepadInput::RightThumb),
            _ => None,
        }
    }

    /// Human-readable label for the settings UI.
    pub fn label(self) -> &'static str {
        match self {
            GamepadInput::South => "A / Cross (South)",
            GamepadInput::East => "B / Circle (East)",
            GamepadInput::North => "X / Triangle (North)",
            GamepadInput::West => "Y / Square (West)",
            GamepadInput::Start => "Start",
            GamepadInput::Select => "Select / Back",
            GamepadInput::DpadUp => "D-Pad Up",
            GamepadInput::DpadDown => "D-Pad Down",
            GamepadInput::DpadLeft => "D-Pad Left",
            GamepadInput::DpadRight => "D-Pad Right",
            GamepadInput::LeftStickUp => "Left Stick Up",
            GamepadInput::LeftStickDown => "Left Stick Down",
            GamepadInput::LeftStickLeft => "Left Stick Left",
            GamepadInput::LeftStickRight => "Left Stick Right",
            GamepadInput::LeftShoulder => "Left Shoulder (L1)",
            GamepadInput::RightShoulder => "Right Shoulder (R1)",
            GamepadInput::LeftTrigger => "Left Trigger (L2)",
            GamepadInput::RightTrigger => "Right Trigger (R2)",
            GamepadInput::LeftThumb => "Left Thumb (L3)",
            GamepadInput::RightThumb => "Right Thumb (R3)",
        }
    }
}

/// Analog stick deadzone preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GamepadDeadzone {
    Low,
    Medium,
    High,
}

impl GamepadDeadzone {
    /// The (press, release) magnitude thresholds for stick hysteresis.
    pub fn thresholds(self) -> (f32, f32) {
        match self {
            GamepadDeadzone::Low => (0.35, 0.20),
            GamepadDeadzone::Medium => (0.55, 0.35),
            GamepadDeadzone::High => (0.75, 0.55),
        }
    }

    /// The stable lowercase identifier used by the settings UI.
    pub fn as_str(self) -> &'static str {
        match self {
            GamepadDeadzone::Low => "low",
            GamepadDeadzone::Medium => "medium",
            GamepadDeadzone::High => "high",
        }
    }

    /// Parse a settings identifier into a deadzone preset.
    pub fn parse(value: &str) -> Option<GamepadDeadzone> {
        match value {
            "low" => Some(GamepadDeadzone::Low),
            "medium" => Some(GamepadDeadzone::Medium),
            "high" => Some(GamepadDeadzone::High),
            _ => None,
        }
    }
}

/// Device-to-key input mapping. Inputs absent from the map are ignored.
/// `gamepad_enabled` gates the gamepad pump; `deadzone` tunes stick
/// hysteresis. Key names are validated by the selected Family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputMapping {
    pub gamepad_enabled: bool,
    pub deadzone: GamepadDeadzone,
    pub gamepad: BTreeMap<GamepadInput, String>,
}

impl InputMapping {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .gamepad
            .values()
            .any(|key| input_key_code(key).is_none())
        {
            return Err("ASTRA_EMU_INPUT_KEY_INVALID".into());
        }
        Ok(())
    }
}

impl Default for InputMapping {
    fn default() -> Self {
        default_vn_preset()
    }
}

/// The general-purpose visual-novel gamepad preset.
///
/// Targets engines without native gamepad support by mapping the device onto
/// the keyboard keys those engines already understand: face buttons and
/// start/select produce confirm/cancel, the pad and left stick drive menu
/// navigation. Shoulders and triggers are left unbound by default.
pub fn default_vn_preset() -> InputMapping {
    let mut gamepad = BTreeMap::new();
    gamepad.insert(GamepadInput::South, "enter".to_owned());
    gamepad.insert(GamepadInput::East, "escape".to_owned());
    gamepad.insert(GamepadInput::North, "space".to_owned());
    gamepad.insert(GamepadInput::Start, "enter".to_owned());
    gamepad.insert(GamepadInput::Select, "escape".to_owned());
    gamepad.insert(GamepadInput::DpadUp, "arrow_up".to_owned());
    gamepad.insert(GamepadInput::DpadDown, "arrow_down".to_owned());
    gamepad.insert(GamepadInput::DpadLeft, "arrow_left".to_owned());
    gamepad.insert(GamepadInput::DpadRight, "arrow_right".to_owned());
    gamepad.insert(GamepadInput::LeftStickUp, "arrow_up".to_owned());
    gamepad.insert(GamepadInput::LeftStickDown, "arrow_down".to_owned());
    gamepad.insert(GamepadInput::LeftStickLeft, "arrow_left".to_owned());
    gamepad.insert(GamepadInput::LeftStickRight, "arrow_right".to_owned());
    InputMapping {
        gamepad_enabled: true,
        deadzone: GamepadDeadzone::Medium,
        gamepad,
    }
}

pub fn input_key_code(control: &str) -> Option<crate::model::KeyCode> {
    use crate::model::KeyCode;
    Some(match control {
        "a" => KeyCode::A,
        "b" => KeyCode::B,
        "c" => KeyCode::C,
        "d" => KeyCode::D,
        "e" => KeyCode::E,
        "f" => KeyCode::F,
        "g" => KeyCode::G,
        "h" => KeyCode::H,
        "i" => KeyCode::I,
        "j" => KeyCode::J,
        "k" => KeyCode::K,
        "l" => KeyCode::L,
        "m" => KeyCode::M,
        "n" => KeyCode::N,
        "o" => KeyCode::O,
        "p" => KeyCode::P,
        "q" => KeyCode::Q,
        "r" => KeyCode::R,
        "s" => KeyCode::S,
        "t" => KeyCode::T,
        "u" => KeyCode::U,
        "v" => KeyCode::V,
        "w" => KeyCode::W,
        "x" => KeyCode::X,
        "y" => KeyCode::Y,
        "z" => KeyCode::Z,
        "enter" | "return" | "confirm" => KeyCode::Enter,
        "escape" | "esc" | "cancel" => KeyCode::Escape,
        "space" => KeyCode::Space,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "arrow_left" | "left" => KeyCode::ArrowLeft,
        "arrow_right" | "right" => KeyCode::ArrowRight,
        "arrow_up" | "up" => KeyCode::ArrowUp,
        "arrow_down" | "down" => KeyCode::ArrowDown,
        "page_up" => KeyCode::PageUp,
        "page_down" => KeyCode::PageDown,
        "delete" => KeyCode::Delete,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "control" | "control_left" => KeyCode::ControlLeft,
        "control_right" => KeyCode::ControlRight,
        "shift" | "shift_left" => KeyCode::ShiftLeft,
        "shift_right" => KeyCode::ShiftRight,
        "alt" | "alt_left" => KeyCode::AltLeft,
        "alt_right" => KeyCode::AltRight,
        "f1" => KeyCode::F1,
        "f2" => KeyCode::F2,
        "f3" => KeyCode::F3,
        "f4" => KeyCode::F4,
        "f5" => KeyCode::F5,
        "f6" => KeyCode::F6,
        "f7" => KeyCode::F7,
        "f8" => KeyCode::F8,
        "f9" => KeyCode::F9,
        "f10" => KeyCode::F10,
        "f11" => KeyCode::F11,
        "f12" => KeyCode::F12,
        _ => return None,
    })
}

/// Canonical name of a bindable key; the inverse of [`input_key_code`].
pub fn input_key_name(code: crate::model::KeyCode) -> Option<&'static str> {
    use crate::model::KeyCode;
    Some(match code {
        KeyCode::A => "a",
        KeyCode::B => "b",
        KeyCode::C => "c",
        KeyCode::D => "d",
        KeyCode::E => "e",
        KeyCode::F => "f",
        KeyCode::G => "g",
        KeyCode::H => "h",
        KeyCode::I => "i",
        KeyCode::J => "j",
        KeyCode::K => "k",
        KeyCode::L => "l",
        KeyCode::M => "m",
        KeyCode::N => "n",
        KeyCode::O => "o",
        KeyCode::P => "p",
        KeyCode::Q => "q",
        KeyCode::R => "r",
        KeyCode::S => "s",
        KeyCode::T => "t",
        KeyCode::U => "u",
        KeyCode::V => "v",
        KeyCode::W => "w",
        KeyCode::X => "x",
        KeyCode::Y => "y",
        KeyCode::Z => "z",
        KeyCode::Enter => "enter",
        KeyCode::Escape => "escape",
        KeyCode::Space => "space",
        KeyCode::Tab => "tab",
        KeyCode::Backspace => "backspace",
        KeyCode::ArrowLeft => "arrow_left",
        KeyCode::ArrowRight => "arrow_right",
        KeyCode::ArrowUp => "arrow_up",
        KeyCode::ArrowDown => "arrow_down",
        KeyCode::PageUp => "page_up",
        KeyCode::PageDown => "page_down",
        KeyCode::Delete => "delete",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::ControlLeft => "control_left",
        KeyCode::ControlRight => "control_right",
        KeyCode::ShiftLeft => "shift_left",
        KeyCode::ShiftRight => "shift_right",
        KeyCode::AltLeft => "alt_left",
        KeyCode::AltRight => "alt_right",
        KeyCode::F1 => "f1",
        KeyCode::F2 => "f2",
        KeyCode::F3 => "f3",
        KeyCode::F4 => "f4",
        KeyCode::F5 => "f5",
        KeyCode::F6 => "f6",
        KeyCode::F7 => "f7",
        KeyCode::F8 => "f8",
        KeyCode::F9 => "f9",
        KeyCode::F10 => "f10",
        KeyCode::F11 => "f11",
        KeyCode::F12 => "f12",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_maps_confirm_cancel_and_navigation() {
        let mapping = default_vn_preset();
        assert!(mapping.gamepad_enabled);
        assert_eq!(mapping.deadzone, GamepadDeadzone::Medium);
        assert_eq!(mapping.gamepad[&GamepadInput::South], "enter");
        assert_eq!(mapping.gamepad[&GamepadInput::East], "escape");
        assert_eq!(mapping.gamepad[&GamepadInput::North], "space");
        assert_eq!(mapping.gamepad[&GamepadInput::Start], "enter");
        assert_eq!(mapping.gamepad[&GamepadInput::Select], "escape");
        assert_eq!(mapping.gamepad[&GamepadInput::DpadUp], "arrow_up");
        assert_eq!(mapping.gamepad[&GamepadInput::LeftStickLeft], "arrow_left");
        // Shoulders/triggers are unbound by default.
        assert!(!mapping.gamepad.contains_key(&GamepadInput::LeftShoulder));
        assert!(!mapping.gamepad.contains_key(&GamepadInput::RightTrigger));
    }

    #[test]
    fn deadzone_thresholds_use_hysteresis_and_ordering() {
        let (low_press, low_release) = GamepadDeadzone::Low.thresholds();
        let (med_press, med_release) = GamepadDeadzone::Medium.thresholds();
        let (high_press, high_release) = GamepadDeadzone::High.thresholds();
        assert!(low_release < low_press);
        assert!(med_release < med_press);
        assert!(high_release < high_press);
        assert!(low_press < med_press && med_press < high_press);
    }

    #[test]
    fn mapping_round_trips_through_json() {
        let mapping = default_vn_preset();
        let json = serde_json::to_string(&mapping).unwrap();
        let restored: InputMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, mapping);
    }

    #[test]
    fn mapping_rejects_unknown_fields_to_keep_schema_strict() {
        let parsed: Result<InputMapping, _> = serde_json::from_str(
            r#"{"gamepad_enabled":true,"deadzone":"medium","gamepad":{},"bogus":1}"#,
        );
        assert!(parsed.is_err());
        let missing: Result<InputMapping, _> = serde_json::from_str(r#"{"gamepad_enabled":true}"#);
        assert!(missing.is_err());
    }

    macro_rules! every_key_code {
        ($($variant:ident),* $(,)?) => {{
            fn exhaustive(code: crate::model::KeyCode) {
                match code {
                    $(crate::model::KeyCode::$variant)|* => {}
                }
            }
            let _ = exhaustive;
            [$(crate::model::KeyCode::$variant),*]
        }};
    }

    #[test]
    fn key_names_round_trip_through_key_codes() {
        let all = every_key_code![
            Unknown,
            A,
            B,
            C,
            D,
            E,
            F,
            G,
            H,
            I,
            J,
            K,
            L,
            M,
            N,
            O,
            P,
            Q,
            R,
            S,
            T,
            U,
            V,
            W,
            X,
            Y,
            Z,
            Digit0,
            Digit1,
            Digit2,
            Digit3,
            Digit4,
            Digit5,
            Digit6,
            Digit7,
            Digit8,
            Digit9,
            Escape,
            Enter,
            Tab,
            Backspace,
            Space,
            Insert,
            Delete,
            Home,
            End,
            PageUp,
            PageDown,
            ArrowLeft,
            ArrowRight,
            ArrowUp,
            ArrowDown,
            ControlLeft,
            ControlRight,
            ShiftLeft,
            ShiftRight,
            AltLeft,
            AltRight,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            F11,
            F12,
            Numpad0,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad4,
            Numpad5,
            Numpad6,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadEnter,
            NumpadAdd,
            NumpadSubtract,
            NumpadMultiply,
            NumpadDivide,
            Minus,
            Equals,
            Comma,
            Period,
            Slash,
            Semicolon,
            Apostrophe,
            BracketLeft,
            BracketRight,
            Backslash,
            Grave,
        ];
        let mut named = 0;
        for code in all {
            if let Some(name) = input_key_name(code) {
                named += 1;
                assert_eq!(input_key_code(name), Some(code), "{name}");
            }
        }
        assert_eq!(named, 58);
        for name in default_vn_preset().gamepad.values() {
            assert_eq!(
                input_key_name(input_key_code(name).unwrap()),
                Some(name.as_str())
            );
        }
        assert_eq!(input_key_name(crate::model::KeyCode::Digit1), None);
        assert_eq!(input_key_name(crate::model::KeyCode::Insert), None);
    }
}
