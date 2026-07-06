use abi_stable::std_types::RVec;
use astra_core::Diagnostic;
use astra_runtime::{
    ActionCallRequest, ActionCallResult, ActionDescriptor, ActionTrace, DeterministicActionContext,
    RuntimeAction, RuntimeError, RuntimeWorld,
};
use std::collections::BTreeMap;

use crate::{FfiActionInvoke, FfiActionRegistration, PluginError};

#[derive(Clone)]
pub struct LoadedFfiAction {
    provider_id: String,
    descriptor: ActionDescriptor,
    invoke: FfiActionInvoke,
}

impl LoadedFfiAction {
    pub fn from_registration(registration: FfiActionRegistration) -> Self {
        Self {
            provider_id: registration.provider_id.to_string(),
            descriptor: ActionDescriptor {
                id: registration.action_id.to_string(),
                input_schema: registration.input_schema.to_string(),
                output_schema: registration.output_schema.to_string(),
            },
            invoke: registration.invoke,
        }
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn install(&self, world: &mut RuntimeWorld) {
        world.register_action(
            self.provider_id.clone(),
            FfiRuntimeAction {
                descriptor: self.descriptor.clone(),
                invoke: self.invoke,
            },
        );
    }
}

pub fn install_actions(
    actions: &[LoadedFfiAction],
    world: &mut RuntimeWorld,
) -> Result<(), PluginError> {
    for action in actions {
        action.install(world);
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
        let request = ActionCallRequest {
            step: ctx.step(),
            action_id: self.descriptor.id.clone(),
            input: input.clone(),
        };
        let request = postcard::to_allocvec(&request)
            .map_err(|err| RuntimeError::message(format!("encode ffi action request: {err}")))?;
        let response = (self.invoke)(RVec::from(request));
        let response: Vec<u8> = response.into_iter().collect();
        let result: ActionCallResult = postcard::from_bytes(&response)
            .map_err(|err| RuntimeError::message(format!("decode ffi action result: {err}")))?;
        match result {
            ActionCallResult::Ok { trace, effects } => {
                for effect in effects {
                    ctx.apply_effect(effect)?;
                }
                Ok(trace)
            }
            ActionCallResult::Err { code, message } => Err(RuntimeError::diagnostic(
                Diagnostic::blocking(code, message),
            )),
        }
    }
}
