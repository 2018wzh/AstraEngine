use super::*;

/// In-process NativeVN work; strings are interpreted only by legacy ABI adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeVnStepCommand {
    LaunchDefault,
    Execute(CoreVnPlayerCommand),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeVnStepInput {
    pub timing: TickInput,
    pub mode: astra_runtime::TickMode,
    pub command: NativeVnStepCommand,
}

impl VnSession {
    pub fn step(&mut self, input: NativeVnStepInput) -> Result<NativeVnStepOutput, CoreVnError> {
        if self.failed {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_FAILED",
                "failed session requires a successful restore before another step",
            ));
        }
        // Set before any fallible execution, including panic unwinding.
        self.failed = true;
        let mut failure_scope = StepFailureScope(Some(self.engine.world().task_scope()));
        tracing::trace!(
            event = "vn.provider.session.step",
            fixed_step = input.timing.fixed_step,
            "AstraVN runtime session step started"
        );
        self.engine
            .validate_step(input.timing, input.mode)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let command = match input.command {
            NativeVnStepCommand::Execute(command) => command,
            NativeVnStepCommand::LaunchDefault => {
                self.runtime.default_launch_command().ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                        "compiled story has no launchable state",
                    )
                })?
            }
        };
        let output = self.apply_command_at_step(command, input.timing, input.mode)?;
        validate_audio_order(&output.presentations, &output.audio)?;
        failure_scope.0 = None;
        self.failed = false;
        Ok(output)
    }
}

// Cancels outstanding work on both Result errors and panic unwinding.
struct StepFailureScope(Option<astra_runtime::TaskScope>);

impl Drop for StepFailureScope {
    fn drop(&mut self) {
        if let Some(scope) = &self.0 {
            scope.cancel();
        }
    }
}

/// Owned in-process presentation output with a typed display projection.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeVnStepOutput {
    pub fixed_step: u64,
    pub vn_state: NativeVnStateView,
    pub presentations: Vec<PresentationCommand>,
    pub audio: Vec<VnAudioCommand>,
    pub timeline: Vec<astra_vn_core::VnTimelineTask>,
    pub coverage_reached: Vec<String>,
}

fn validate_audio_order(
    presentations: &[PresentationCommand],
    audio: &[VnAudioCommand],
) -> Result<(), CoreVnError> {
    let mut audio = audio.iter();
    for command in presentations {
        if let PresentationCommand::Stage(StageCommand::Audio(cue)) = command {
            let output = audio.next().ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_AUDIO_ORDER_MISSING",
                    "audio presentation has no matching output",
                )
            })?;
            if &output.cue != cue {
                return Err(CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_AUDIO_ORDER_MISMATCH",
                    "audio output differs from its presentation",
                ));
            }
        }
    }
    if audio.next().is_some() {
        return Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_AUDIO_ORDER_EXTRA",
            "audio output has no matching presentation",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_audio_matches_interleaved_presentations_and_rejects_missing_extra_or_reordered_cues() {
        let cue = |asset: &str| astra_vn_core::AudioCue {
            id: asset.into(),
            bus: VnAudioBus::Bgm,
            asset: asset.into(),
            looped: true,
            fade_ms: 120,
            sync: VnAudioSync::Fence("music.ready".into()),
        };
        let first = cue("music.first");
        let second = cue("music.second");
        let presentations = vec![
            PresentationCommand::Stage(StageCommand::Audio(first.clone())),
            PresentationCommand::Dialogue {
                key: "line.one".into(),
                speaker: None,
                voice: None,
                window: None,
            },
            PresentationCommand::Stage(StageCommand::Audio(second.clone())),
        ];
        let audio = vec![
            VnAudioCommand {
                command_id: "first".into(),
                cue: first,
            },
            VnAudioCommand {
                command_id: "second".into(),
                cue: second,
            },
        ];
        validate_audio_order(&presentations, &audio).unwrap();
        assert!(validate_audio_order(&presentations, &audio[..1])
            .unwrap_err()
            .to_string()
            .contains("ORDER_MISSING"));
        assert!(validate_audio_order(&presentations[..1], &audio)
            .unwrap_err()
            .to_string()
            .contains("ORDER_EXTRA"));
        let mut reversed = audio;
        reversed.reverse();
        assert!(validate_audio_order(&presentations, &reversed)
            .unwrap_err()
            .to_string()
            .contains("ORDER_MISMATCH"));
    }
}
