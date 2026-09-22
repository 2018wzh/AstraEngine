use astra_core::SchemaVersion;
use astra_plugin_abi::{GameRuntimeSessionId, NATIVE_VN_RUNTIME_ID};
#[cfg(test)]
use astra_runtime::RuntimeWorld;
use astra_runtime::{
    ActorId, BlackboardValue, ComponentId, ComponentRecord, EngineSession, EventPayload,
    OrderedTickIngress, PackageHandle, RuntimeComponentPayload, RuntimeConfig, RuntimeError,
    RuntimeSnapshot, SaveBlob, SaveRequest, TickIngress, TickInput, TickIntegrityMode, TickRequest,
};
use astra_vn_core::*;
use astra_vn_core::{
    CompiledStory as CoreCompiledStory, VnError as CoreVnError,
    VnPlayerCommand as CoreVnPlayerCommand, VnRuntime as CoreVnRuntime,
    VnRuntimeIndex as CoreVnRuntimeIndex,
};
use std::{collections::BTreeMap, sync::Arc};
mod native_open;
mod native_session;
mod native_session_save;
mod native_session_step;
mod native_step;
mod native_view;
mod restore;
#[cfg(test)]
mod restore_tests;
pub use native_open::VnSessionConfig;
pub use native_session::VnSession;
pub use native_step::{NativeVnStepCommand, NativeVnStepInput, NativeVnStepOutput};
pub use native_view::NativeVnStateView;
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VnStepComplexityMetrics {
    pub schema: String,
    pub previous_backlog_count: usize,
    pub appended_backlog_entries: usize,
    pub state_cache_hit: bool,
    pub materialized_history_entries: usize,
    pub history_component_writes: usize,
    pub encoded_hot_state_bytes: usize,
    pub mutation_journal_entries: usize,
}

fn materialize_session_state(session: &VnSession) -> Result<VnRuntimeState, CoreVnError> {
    Ok(session.runtime.state().clone())
}
fn materialized_save_snapshot(session: &VnSession) -> Result<RuntimeSnapshot, CoreVnError> {
    let state = materialize_session_state(session)?;
    let mut snapshot = session
        .engine
        .world()
        .snapshot()
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    let mut id_probe = snapshot.id_source.clone();
    let component_id = loop {
        let candidate = ComponentId(id_probe.next_id());
        if snapshot.actors.component(candidate).is_none() {
            break candidate;
        }
    };
    let payload = RuntimeComponentPayload::typed(
        VN_RUNTIME_STATE_SCHEMA,
        SchemaVersion::new(VN_RUNTIME_STATE_SCHEMA_MAJOR, 0, 0),
        state,
    );
    if !snapshot.actors.attach_component(ComponentRecord {
        component_id,
        actor_id: session.owner,
        payload,
    }) {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_SAVE_OWNER_MISSING",
            "VN save materialization owner is missing from the Runtime snapshot",
        ));
    }
    Ok(snapshot)
}
fn command_resolves_wait(
    command: &CoreVnPlayerCommand,
    wait: Option<VnWaitKind>,
    reading_mode: astra_vn_core::ReadingMode,
    compiled: &astra_vn_core::CompiledStory,
) -> bool {
    if matches!(command, CoreVnPlayerCommand::Advance)
        && matches!(wait, Some(VnWaitKind::Dialogue | VnWaitKind::Input))
    {
        return reading_mode != astra_vn_core::ReadingMode::Hidden;
    }
    matches!(
        (command, wait),
        (CoreVnPlayerCommand::Choose { .. }, Some(VnWaitKind::Choice))
            | (
                CoreVnPlayerCommand::ReturnSystem,
                Some(VnWaitKind::SystemPage)
            )
            | (
                CoreVnPlayerCommand::SwitchSystemPage { .. },
                Some(VnWaitKind::SystemPage)
            )
            | (
                CoreVnPlayerCommand::SetReadingMode {
                    mode: astra_vn_core::ReadingMode::FastForward,
                },
                Some(VnWaitKind::Dialogue | VnWaitKind::Input)
            )
            | (
                CoreVnPlayerCommand::StartReplay { .. }
                    | CoreVnPlayerCommand::JumpRoute { .. }
                    | CoreVnPlayerCommand::JumpBacklog { .. },
                Some(VnWaitKind::SystemPage)
            )
            | (
                CoreVnPlayerCommand::CompleteWait { .. },
                Some(
                    VnWaitKind::Fence
                        | VnWaitKind::Timer
                        | VnWaitKind::TimelineComplete
                        | VnWaitKind::MovieEnd
                        | VnWaitKind::VoiceEnd
                )
            )
    ) || matches!(
        (command, wait),
        (
            CoreVnPlayerCommand::InvokeSystemAction { action_id },
            Some(VnWaitKind::SystemPage)
        ) if compiled
            .system_story_manifest
            .actions
            .get(action_id)
            .is_some_and(|action| action.effects.iter().any(|effect| matches!(
                effect,
                astra_vn_core::SystemActionEffect::Jump { .. }
                    | astra_vn_core::SystemActionEffect::SwitchSystemPage { .. }
                    | astra_vn_core::SystemActionEffect::ReturnSystem
            )))
    )
}

use astra_vn_policy::VnPolicyState;
