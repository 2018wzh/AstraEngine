use abi_stable::std_types::RVec;
use astra_core::Diagnostic;
use astra_runtime::{
    ActionCallRequest, ActionCallResult, ActionDescriptor, ActionEffect, ActionResourceKey,
    ActionTrace, DeterministicActionContext, RuntimeAction, RuntimeError, RuntimeWorld,
};
use std::collections::BTreeMap;
use tracing::{debug, warn};

use crate::{FfiActionInvoke, FfiActionRegistration, PluginError};

#[derive(Clone)]
pub struct LoadedFfiAction {
    provider_id: String,
    descriptor: ActionDescriptor,
    invoke: FfiActionInvoke,
}

impl LoadedFfiAction {
    pub fn from_registration(registration: FfiActionRegistration) -> Result<Self, PluginError> {
        if registration.abi_version != astra_plugin_abi::ACTION_PLUGIN_ABI_VERSION {
            return Err(PluginError::Load(format!(
                "ASTRA_PLUGIN_ACTION_ABI_VERSION: action {} uses ABI {}, expected {}",
                registration.action_id,
                registration.abi_version,
                astra_plugin_abi::ACTION_PLUGIN_ABI_VERSION
            )));
        }
        let descriptor: ActionDescriptor =
            serde_json::from_slice(registration.descriptor_json.as_slice()).map_err(|error| {
                PluginError::Load(format!(
                    "ASTRA_PLUGIN_ACTION_DESCRIPTOR_DECODE: action {}: {error}",
                    registration.action_id
                ))
            })?;
        if descriptor.id != registration.action_id.as_str()
            || descriptor.input_schema != registration.input_schema.as_str()
            || descriptor.output_schema != registration.output_schema.as_str()
        {
            return Err(PluginError::Load(
                "ASTRA_PLUGIN_ACTION_DESCRIPTOR_IDENTITY: action registration metadata does not match descriptor"
                    .to_string(),
            ));
        }
        Ok(Self {
            provider_id: registration.provider_id.to_string(),
            descriptor,
            invoke: registration.invoke,
        })
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn install(&self, world: &mut RuntimeWorld) -> Result<(), PluginError> {
        debug!(
            provider_id = %self.provider_id,
            action_id = %self.descriptor.id,
            "plugin.action.register"
        );
        world
            .register_action(
                self.provider_id.clone(),
                FfiRuntimeAction {
                    descriptor: self.descriptor.clone(),
                    invoke: self.invoke,
                },
            )
            .map_err(|err| PluginError::Load(err.to_string()))
    }
}

pub fn install_actions(
    actions: &[LoadedFfiAction],
    world: &mut RuntimeWorld,
) -> Result<(), PluginError> {
    for action in actions {
        action.install(world)?;
    }
    Ok(())
}

struct FfiRuntimeAction {
    descriptor: ActionDescriptor,
    invoke: FfiActionInvoke,
}

impl RuntimeAction for FfiRuntimeAction {
    fn descriptor(&self) -> ActionDescriptor {
        self.descriptor.clone()
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, astra_runtime::BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        debug!(
            step = ctx.step(),
            action_id = %self.descriptor.id,
            "plugin.action.invoke"
        );
        let request = ActionCallRequest {
            step: ctx.step(),
            action_id: self.descriptor.id.clone(),
            input: input.clone(),
            trigger_event: ctx.trigger_event().cloned(),
        };
        let request = postcard::to_allocvec(&request)
            .map_err(|err| RuntimeError::message(format!("encode ffi action request: {err}")))?;
        let response = (self.invoke)(RVec::from(request));
        let response: Vec<u8> = response.into_iter().collect();
        let result: ActionCallResult = postcard::from_bytes(&response)
            .map_err(|err| RuntimeError::message(format!("decode ffi action result: {err}")))?;
        match result {
            ActionCallResult::Ok { trace, effects } => {
                debug!(
                    step = ctx.step(),
                    action_id = %trace.action_id,
                    effect_count = effects.len(),
                    "plugin.action.ok"
                );
                for effect in effects {
                    validate_effect_access(&self.descriptor, &effect)?;
                    ctx.apply_effect(effect)?;
                }
                Ok(trace)
            }
            ActionCallResult::Err { code, message } => Err(RuntimeError::diagnostic({
                warn!(
                    step = ctx.step(),
                    action_id = %self.descriptor.id,
                    diagnostic_code = %code,
                    "plugin.action.err"
                );
                Diagnostic::blocking(code, message)
            })),
        }
    }
}

fn validate_effect_access(
    descriptor: &ActionDescriptor,
    effect: &ActionEffect,
) -> Result<(), RuntimeError> {
    let required = match effect {
        ActionEffect::SetBlackboard { .. } => vec![ActionResourceKey::Blackboard],
        ActionEffect::CreateActor { .. } | ActionEffect::AttachComponent { .. } => vec![
            ActionResourceKey::ActorStore,
            ActionResourceKey::StableIdSource,
        ],
        ActionEffect::ReplaceComponent { .. }
        | ActionEffect::PatchComponentMap { .. }
        | ActionEffect::RemoveActor { .. }
        | ActionEffect::DetachComponent { .. } => vec![
            ActionResourceKey::ActorStore,
            ActionResourceKey::MutationLog,
        ],
        ActionEffect::EmitEvent { .. } => vec![
            ActionResourceKey::EventQueue,
            ActionResourceKey::StableIdSource,
        ],
        ActionEffect::Presentation { .. } => vec![ActionResourceKey::Presentation],
        ActionEffect::Await { .. } => vec![ActionResourceKey::AwaitQueue],
        ActionEffect::ScheduleDelayedEvent { .. } => vec![
            ActionResourceKey::DelayedEventQueue,
            ActionResourceKey::StableIdSource,
        ],
        ActionEffect::CancelDelayedEvent { .. } => {
            vec![ActionResourceKey::DelayedEventQueue]
        }
    };
    if let Some(resource) = required
        .into_iter()
        .find(|resource| !descriptor.access.writes.contains(resource))
    {
        return Err(RuntimeError::diagnostic(
            Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTION_ACCESS_UNDECLARED",
                "plugin action returned an effect outside its declared write set",
            )
            .with_field("action_id", &descriptor.id)
            .with_field("resource", format!("{resource:?}")),
        ));
    }
    Ok(())
}
