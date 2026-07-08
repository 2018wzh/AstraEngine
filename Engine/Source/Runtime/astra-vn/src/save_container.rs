use astra_core::{Hash128, SchemaMigrationRegistry};
use astra_package::{ContainerError, SectionPayload};
use astra_runtime::{read_runtime_save_section, RuntimeError, SaveBlob};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{VnPolicyState, VnRuntimeState};

pub const VN_RUNTIME_STATE_SECTION_ID: &str = "vn.runtime_state";
pub const VN_POLICY_STATE_SECTION_ID: &str = "vn.policy_state";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnRuntimeStateSave {
    pub schema: String,
    pub state_hash: Hash128,
    pub state: VnRuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyStateSave {
    pub schema: String,
    pub state_hash: Hash128,
    pub state: VnPolicyState,
}

pub fn runtime_state_save_section(
    state: &VnRuntimeState,
) -> Result<SectionPayload, ContainerError> {
    let save = VnRuntimeStateSave {
        schema: "astra.vn.runtime_state_save.v1".to_string(),
        state_hash: vn_runtime_state_hash(state),
        state: state.clone(),
    };
    SectionPayload::postcard(
        VN_RUNTIME_STATE_SECTION_ID,
        "astra.vn.runtime_state_save.v1",
        &save,
    )
}

pub fn policy_state_save_section(state: &VnPolicyState) -> Result<SectionPayload, ContainerError> {
    let save = VnPolicyStateSave {
        schema: "astra.vn.policy_state_save.v1".to_string(),
        state_hash: vn_policy_state_hash(state),
        state: state.clone(),
    };
    SectionPayload::postcard(
        VN_POLICY_STATE_SECTION_ID,
        "astra.vn.policy_state_save.v1",
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

fn vn_runtime_state_hash(state: &VnRuntimeState) -> Hash128 {
    Hash128::from_blake3(
        &postcard::to_allocvec(state).expect("AstraVN runtime state must serialize for hashing"),
    )
}

fn vn_policy_state_hash(state: &VnPolicyState) -> Hash128 {
    Hash128::from_blake3(
        &postcard::to_allocvec(state).expect("AstraVN policy state must serialize for hashing"),
    )
}
