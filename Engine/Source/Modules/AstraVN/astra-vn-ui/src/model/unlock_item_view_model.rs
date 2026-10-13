use super::*;

impl UnlockItemViewModel {
    pub(super) fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("item_id", UiValue::String(self.item_id.clone())),
            ("label_key", UiValue::String(self.label_key.clone())),
            ("thumbnail_asset", optional_string(&self.thumbnail_asset)),
            ("has_thumbnail", UiValue::Bool(self.has_thumbnail)),
            ("unlocked", UiValue::Bool(self.unlocked)),
        ])
    }
}

impl RouteNodeViewModel {
    pub(super) fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("node_id", UiValue::String(self.node_id.clone())),
            ("label_key", UiValue::String(self.label_key.clone())),
            ("terminal", UiValue::Bool(self.terminal)),
            ("reached", UiValue::Bool(self.reached)),
            ("x_milli", UiValue::Integer(i64::from(self.x_milli))),
            ("y_milli", UiValue::Integer(i64::from(self.y_milli))),
        ])
    }
}

impl TextInputViewModel {
    pub(super) fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("input_id", UiValue::String(self.input_id.clone())),
            ("value", UiValue::String(self.value.clone())),
            ("multiline", UiValue::Bool(self.multiline)),
            (
                "max_graphemes",
                UiValue::Integer(i64::from(self.max_graphemes)),
            ),
            (
                "character_policy",
                UiValue::String(self.character_policy.clone()),
            ),
        ])
    }
}

impl ConfigViewModel {
    pub(super) fn to_ui_value(&self) -> UiValue {
        ui_map([
            (
                "reading_hidden",
                UiValue::Bool(self.reading_mode == ReadingMode::Hidden),
            ),
            (
                "reading_manual",
                UiValue::Bool(self.reading_mode == ReadingMode::Manual),
            ),
            (
                "reading_fast_forward",
                UiValue::Bool(self.reading_mode == ReadingMode::FastForward),
            ),
            ("audio_enabled", UiValue::Bool(self.audio_enabled)),
            ("audio_disabled", UiValue::Bool(!self.audio_enabled)),
            ("master_volume", UiValue::Integer(self.master_volume)),
            ("text_speed", UiValue::Integer(self.text_speed)),
            ("auto_delay_ms", UiValue::Integer(self.auto_delay_ms)),
            ("high_contrast", UiValue::Bool(self.high_contrast)),
            ("locale", UiValue::String(self.locale.clone())),
            (
                "available_locales",
                UiValue::List(
                    self.available_locales
                        .iter()
                        .cloned()
                        .map(UiValue::String)
                        .collect(),
                ),
            ),
            ("player_name", self.player_name.to_ui_value()),
        ])
    }
}

impl LocalizationEntryViewModel {
    pub(super) fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("entry_id", UiValue::String(self.entry_id.clone())),
            ("text_key", UiValue::String(self.text_key.clone())),
        ])
    }
}

pub(super) fn ui_map<const N: usize>(entries: [(&str, UiValue); N]) -> UiValue {
    UiValue::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

pub(super) fn optional_string(value: &Option<String>) -> UiValue {
    value
        .as_ref()
        .cloned()
        .map(UiValue::String)
        .unwrap_or(UiValue::Null)
}

pub(super) fn skip_mode_name(value: SkipMode) -> &'static str {
    match value {
        SkipMode::None => "none",
        SkipMode::Read => "read",
        SkipMode::All => "all",
    }
}
