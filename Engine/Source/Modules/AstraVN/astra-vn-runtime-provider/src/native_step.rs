use super::*;

/// In-process NativeVN work; strings are interpreted only by legacy ABI adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeVnStepCommand {
    LaunchDefault,
    Execute(CoreVnPlayerCommand),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeVnStepInput {
    pub session_id: GameRuntimeSessionId,
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: RuntimeStepMode,
    pub command: NativeVnStepCommand,
}

impl NativeVnRuntimeProvider {
    pub fn step_native(
        &mut self,
        input: NativeVnStepInput,
    ) -> Result<RuntimeStepOutput, CoreVnError> {
        tracing::trace!(
            event = "vn.provider.session.step",
            fixed_step = input.fixed_step,
            "AstraVN runtime session step started"
        );
        let command = match input.command {
            NativeVnStepCommand::Execute(command) => command,
            NativeVnStepCommand::LaunchDefault => {
                let session = self.session(&input.session_id)?;
                CoreVnRuntime::from_shared_state_indexed(
                    Arc::clone(&session.compiled),
                    Arc::clone(&session.runtime_index),
                    materialize_session_state(session)?,
                )?
                .default_launch_command()
                .ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                        "compiled story has no launchable state",
                    )
                })?
            }
        };
        self.apply_command_at_step(
            input.session_id,
            command,
            input.fixed_step,
            input.delta_ns,
            input.session_seed,
            input.mode,
        )
    }
}
