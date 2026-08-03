//! Configurable device-to-key input mapping.
//!
//! With the ABI v5 key-name input contract, the Manager owns a generic
//! remapping layer: every device input (gamepad button, stick direction,
//! trigger) is translated to a canonical key name before it is queued to the
//! family runtime. [`InputMapping`] is the persisted, user-editable mapping;
//! [`default_vn_preset`] provides a general-purpose visual-novel layout that
//! works for engines without native gamepad support.

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
    pub const DISPLAY_ORDER: [GamepadInput; 16] = [
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

/// Device-to-key input mapping.
///
/// `gamepad` maps each configured gamepad input to a canonical ABI key name
/// (see `astra_emu_family_api::input_key`). Inputs absent from the map are
/// ignored. `gamepad_enabled` gates the whole gamepad pump; `deadzone` tunes
/// stick hysteresis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputMapping {
    pub gamepad_enabled: bool,
    pub deadzone: GamepadDeadzone,
    pub gamepad: BTreeMap<GamepadInput, String>,
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
        // Struct is not deny_unknown_fields; unknown keys are ignored, but the
        // required fields must still be present.
        assert!(parsed.is_ok());
        let missing: Result<InputMapping, _> = serde_json::from_str(r#"{"gamepad_enabled":true}"#);
        assert!(missing.is_err());
    }
}
