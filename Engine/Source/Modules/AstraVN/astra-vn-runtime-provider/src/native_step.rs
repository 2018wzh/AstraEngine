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
    ) -> Result<NativeVnStepOutput, CoreVnError> {
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
        let output = self.apply_command_at_step(
            input.session_id,
            command,
            input.fixed_step,
            input.delta_ns,
            input.session_seed,
            input.mode,
        )?;
        validate_audio_order(&output.presentations, &output.audio)?;
        Ok(output)
    }
}

/// Owned in-process presentation output. The view currently retains the bounded ABI projection.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeVnStepOutput {
    pub session_id: GameRuntimeSessionId,
    pub fixed_step: u64,
    pub vn_state: astra_plugin_abi::RuntimeLiveVnState,
    pub presentations: Vec<PresentationCommand>,
    pub audio: Vec<VnAudioCommand>,
    pub timeline: Vec<astra_vn_core::VnTimelineTask>,
    pub coverage_reached: Vec<String>,
}

impl NativeVnStepOutput {
    pub(crate) fn into_abi(self) -> Result<RuntimeStepOutput, CoreVnError> {
        let presentation_count = self.presentations.len();
        let audio_command_count = self.audio.len();
        let mut presentations = Vec::with_capacity(presentation_count);
        let mut audio_cues = Vec::with_capacity(audio_command_count);
        let mut audio = self.audio.into_iter();
        for (presentation_index, command) in self.presentations.into_iter().enumerate() {
            let sequence = presentation_index
                .checked_add(1)
                .and_then(|index| u64::try_from(index).ok())
                .ok_or_else(|| CoreVnError::message("VN presentation sequence overflow"))?;
            let has_audio = matches!(&command, PresentationCommand::Stage(StageCommand::Audio(_)));
            if has_audio {
                let audio_command = audio.next().ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_AUDIO_ORDER_MISSING",
                        "typed audio presentation has no matching audio output",
                    )
                })?;
                audio_cues.push(runtime_live_audio_cue(sequence, &audio_command));
            }
            presentations.push(runtime_live_presentation(sequence, command));
        }
        if audio.next().is_some() {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_AUDIO_ORDER_EXTRA",
                "audio output has no matching typed presentation command",
            ));
        }
        let timeline = self
            .timeline
            .into_iter()
            .map(|task| astra_plugin_abi::RuntimeLiveTimelineTask {
                command_id: task.command_id,
                command: runtime_live_timeline(task.command),
            })
            .collect();
        let vn_step = astra_plugin_abi::RuntimeLiveVnStep {
            coverage_reached: self.coverage_reached,
        };
        Ok(RuntimeStepOutput {
            session_id: self.session_id,
            status: if presentation_count == 0 {
                "idle".to_string()
            } else {
                "blocked".to_string()
            },
            live: astra_plugin_abi::RuntimeLiveOutput {
                state_revision: self.fixed_step,
                coverage: RuntimeLiveCoverage {
                    presentation_commands: presentation_count as u64,
                    audio_commands: audio_command_count as u64,
                    ..RuntimeLiveCoverage::default()
                },
                audio_cues,
                presentations,
                timeline,
                vn_state: Some(self.vn_state),
                vn_step: Some(vn_step),
                ..astra_plugin_abi::RuntimeLiveOutput::default()
            },
            diagnostics: Vec::new(),
        })
    }
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
