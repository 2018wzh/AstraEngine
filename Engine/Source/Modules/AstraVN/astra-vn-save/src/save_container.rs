use astra_core::SchemaMigrationRegistry;
use astra_package::{ContainerError, SectionPayload};
use astra_runtime::{read_runtime_save_section, RuntimeError, SaveBlob};
use astra_vn_policy::VnPolicyState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::VnRuntimeState;

pub const VN_RUNTIME_STATE_SECTION_ID: &str = "vn.runtime_state";
pub const VN_POLICY_STATE_SECTION_ID: &str = "vn.policy_state";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnRuntimeStateSave {
    pub schema: String,
    pub state: VnRuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyStateSave {
    pub schema: String,
    pub state: VnPolicyState,
}

pub fn runtime_state_save_section(
    state: &VnRuntimeState,
) -> Result<SectionPayload, ContainerError> {
    tracing::debug!(
        event = "vn.save.runtime_state.encode",
        "AstraVN runtime state save section encoded"
    );
    let save = VnRuntimeStateSave {
        schema: "astra.vn.runtime_state_save.v2".to_string(),
        state: state.clone(),
    };
    SectionPayload::postcard(
        VN_RUNTIME_STATE_SECTION_ID,
        "astra.vn.runtime_state_save.v2",
        &save,
    )
}

pub fn policy_state_save_section(state: &VnPolicyState) -> Result<SectionPayload, ContainerError> {
    tracing::debug!(
        event = "vn.save.policy_state.encode",
        "AstraVN policy state save section encoded"
    );
    let save = VnPolicyStateSave {
        schema: "astra.vn.policy_state_save.v2".to_string(),
        state: state.clone(),
    };
    SectionPayload::postcard(
        VN_POLICY_STATE_SECTION_ID,
        "astra.vn.policy_state_save.v2",
        &save,
    )
}

pub fn read_runtime_save_vn_state(save: &SaveBlob) -> Result<VnRuntimeStateSave, RuntimeError> {
    read_runtime_save_section(
        save,
        VN_RUNTIME_STATE_SECTION_ID,
        &SchemaMigrationRegistry::default(),
    )
}

pub fn read_runtime_save_policy_state(save: &SaveBlob) -> Result<VnPolicyStateSave, RuntimeError> {
    read_runtime_save_section(
        save,
        VN_POLICY_STATE_SECTION_ID,
        &SchemaMigrationRegistry::default(),
    )
}
