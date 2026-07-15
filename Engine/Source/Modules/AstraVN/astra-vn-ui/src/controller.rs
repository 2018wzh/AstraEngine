use std::collections::BTreeMap;

use astra_ui_core::{UiValue, ValidateUi, MAX_EFFECTS_PER_CALL, MAX_SESSION_STATE_BYTES};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::VnUiAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnUiControllerManifest {
    pub schema: String,
    pub id: String,
    pub view: String,
    pub model_schema: String,
    pub snapshot: VnUiControllerSnapshot,
}

impl VnUiControllerManifest {
    pub fn validate(&self) -> Result<(), VnUiControllerError> {
        if self.schema != "astra.vn.ui_controller.v1"
            || self.id.trim().is_empty()
            || self.view.trim().is_empty()
            || self.model_schema.trim().is_empty()
        {
            return Err(VnUiControllerError::Manifest);
        }
        astra_ui_core::validate_serialized_size(self).map_err(|_| VnUiControllerError::Manifest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnUiControllerSnapshot {
    None,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnUiControllerUpdate {
    pub fixed_time_ns: u64,
    pub delta_ns: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnUiControllerEffect {
    Forward {
        action: VnUiAction,
    },
    OpenModal {
        view_id: String,
        model: UiValue,
    },
    CloseModal,
    Focus {
        semantic_id: String,
    },
    SetSessionState {
        key: String,
        value: UiValue,
    },
    Animation {
        target_id: String,
        preset_id: String,
    },
    Trace {
        event: String,
        fields: BTreeMap<String, String>,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VnUiControllerError {
    #[error("ASTRA_VN_UI_CONTROLLER_MANIFEST: controller manifest is invalid")]
    Manifest,
    #[error("ASTRA_VN_UI_CONTROLLER_EFFECT_LIMIT: controller returned too many effects")]
    EffectLimit,
    #[error("ASTRA_VN_UI_CONTROLLER_STATE_LIMIT: controller session state exceeds 1 MiB")]
    StateLimit,
    #[error("ASTRA_VN_UI_CONTROLLER_VALUE: controller returned an invalid value")]
    InvalidValue,
}

#[derive(Debug, Default, Clone)]
pub struct VnUiSessionState {
    values: BTreeMap<String, UiValue>,
}

impl VnUiSessionState {
    pub fn values(&self) -> &BTreeMap<String, UiValue> {
        &self.values
    }

    pub fn apply(&mut self, effects: &[VnUiControllerEffect]) -> Result<(), VnUiControllerError> {
        if effects.len() > MAX_EFFECTS_PER_CALL {
            return Err(VnUiControllerError::EffectLimit);
        }
        let mut next = self.values.clone();
        for effect in effects {
            if let VnUiControllerEffect::SetSessionState { key, value } = effect {
                value
                    .validate()
                    .map_err(|_| VnUiControllerError::InvalidValue)?;
                next.insert(key.clone(), value.clone());
            }
        }
        UiValue::Map(next.clone())
            .validate()
            .map_err(|_| VnUiControllerError::InvalidValue)?;
        let bytes = postcard::to_allocvec(&next).map_err(|_| VnUiControllerError::InvalidValue)?;
        if bytes.len() > MAX_SESSION_STATE_BYTES {
            return Err(VnUiControllerError::StateLimit);
        }
        self.values = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[astra_headless_test::test]
    fn session_state_application_is_atomic() {
        let mut state = VnUiSessionState::default();
        let effects = vec![
            VnUiControllerEffect::SetSessionState {
                key: "valid".into(),
                value: UiValue::Bool(true),
            },
            VnUiControllerEffect::SetSessionState {
                key: String::new(),
                value: UiValue::Bool(false),
            },
        ];
        assert_eq!(
            state.apply(&effects),
            Err(VnUiControllerError::InvalidValue)
        );
        assert!(state.values().is_empty());
    }
}
