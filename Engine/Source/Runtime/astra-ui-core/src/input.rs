use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_string, UiValidationError, ValidateUi};

pub const MAX_UI_VIEWPORT_DIMENSION: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiRect {
    pub min: UiPoint,
    pub max: UiPoint,
}

impl UiRect {
    pub fn is_finite_and_ordered(self) -> bool {
        self.min.x.is_finite()
            && self.min.y.is_finite()
            && self.max.x.is_finite()
            && self.max.y.is_finite()
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiViewport {
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f32,
    pub font_scale: f32,
    pub safe_area_points: UiInsets,
}

impl ValidateUi for UiViewport {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.physical_width == 0 || self.physical_height == 0 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIEWPORT_EMPTY",
                "viewport dimensions must be positive",
            ));
        }
        if self.physical_width > MAX_UI_VIEWPORT_DIMENSION
            || self.physical_height > MAX_UI_VIEWPORT_DIMENSION
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIEWPORT_LIMIT",
                "viewport dimensions exceed the UI product limit",
            ));
        }
        if !(self.scale_factor.is_finite() && self.scale_factor > 0.0) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SCALE_FACTOR",
                "scale factor must be finite and positive",
            ));
        }
        if !(self.font_scale.is_finite() && (0.5..=4.0).contains(&self.font_scale)) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_FONT_SCALE",
                "font scale must be finite and within 0.5..=4.0",
            ));
        }
        let insets = [
            self.safe_area_points.left,
            self.safe_area_points.top,
            self.safe_area_points.right,
            self.safe_area_points.bottom,
        ];
        if insets
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_SAFE_AREA",
                "safe-area insets must be finite and non-negative",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiButtonState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiPointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
    Other(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiTouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiNavigationAction {
    Next,
    Previous,
    Up,
    Down,
    Left,
    Right,
    Activate,
    Cancel,
    PagePrevious,
    PageNext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiInputEventKind {
    FixedTime {
        time_ns: u64,
    },
    Focus {
        focused: bool,
    },
    Resize {
        viewport: UiViewport,
    },
    PointerMove {
        position: UiPoint,
    },
    PointerButton {
        position: UiPoint,
        button: UiPointerButton,
        state: UiButtonState,
    },
    Wheel {
        delta_points: UiPoint,
    },
    Keyboard {
        logical_key: String,
        physical_key: String,
        state: UiButtonState,
        repeat: bool,
        modifiers: u32,
    },
    ImePreedit {
        text: String,
        cursor_start: Option<u32>,
        cursor_end: Option<u32>,
    },
    ImeCommit {
        text: String,
    },
    Touch {
        device_id: u64,
        contact_id: u64,
        position: UiPoint,
        phase: UiTouchPhase,
    },
    Navigation {
        action: UiNavigationAction,
    },
    AccessibilityAction {
        semantic_id: String,
        action: String,
        value: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiInputEvent {
    pub sequence: u64,
    pub kind: UiInputEventKind,
}

impl ValidateUi for UiInputEvent {
    fn validate(&self) -> Result<(), UiValidationError> {
        match &self.kind {
            UiInputEventKind::Resize { viewport } => viewport.validate(),
            UiInputEventKind::PointerMove { position }
            | UiInputEventKind::PointerButton { position, .. }
            | UiInputEventKind::Touch { position, .. } => validate_point(*position),
            UiInputEventKind::Wheel { delta_points } => validate_point(*delta_points),
            UiInputEventKind::Keyboard {
                logical_key,
                physical_key,
                ..
            } => {
                validate_string("input.logical_key", logical_key)?;
                validate_string("input.physical_key", physical_key)
            }
            UiInputEventKind::ImePreedit {
                text,
                cursor_start,
                cursor_end,
            } => {
                validate_string("input.ime_preedit", text)?;
                if let (Some(start), Some(end)) = (cursor_start, cursor_end) {
                    if start > end || *end as usize > text.len() {
                        return Err(UiValidationError::invalid(
                            "ASTRA_UI_IME_CURSOR",
                            "IME cursor range is outside the preedit string",
                        ));
                    }
                }
                Ok(())
            }
            UiInputEventKind::ImeCommit { text } => validate_string("input.ime_commit", text),
            UiInputEventKind::AccessibilityAction {
                semantic_id,
                action,
                value,
            } => {
                crate::validate_id("input.semantic_id", semantic_id)?;
                crate::validate_id("input.accessibility_action", action)?;
                if let Some(value) = value {
                    validate_string("input.accessibility_value", value)?;
                }
                Ok(())
            }
            UiInputEventKind::FixedTime { .. }
            | UiInputEventKind::Focus { .. }
            | UiInputEventKind::Navigation { .. } => Ok(()),
        }
    }
}

fn validate_point(point: UiPoint) -> Result<(), UiValidationError> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_POINT_NON_FINITE",
            "input point must be finite",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiInputFrame {
    pub schema: String,
    pub events: Vec<UiInputEvent>,
}

impl ValidateUi for UiInputFrame {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_input_frame.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_INPUT_SCHEMA",
                "input frame schema must be astra.ui_input_frame.v1",
            ));
        }
        let mut previous = None;
        for event in &self.events {
            event.validate()?;
            if previous.is_some_and(|sequence| sequence >= event.sequence) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_INPUT_SEQUENCE",
                    "input event sequences must be strictly increasing",
                ));
            }
            previous = Some(event.sequence);
        }
        crate::validate_serialized_size(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiInputDispositionKind {
    Consumed,
    Bubble,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiInputDisposition {
    pub sequence: u64,
    pub disposition: UiInputDispositionKind,
    pub semantic_target_id: Option<String>,
}
