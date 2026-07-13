use std::collections::BTreeMap;

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CompiledCommand, CompiledStory, PresentationCommand, StageCommand, TimelineCommand,
    VnMovieEndBehavior,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnStandardCommandManifest {
    pub schema: String,
    pub provider_id: String,
    pub commands: Vec<VnStandardCommandDescriptor>,
}

impl VnStandardCommandManifest {
    pub fn standard() -> Self {
        tracing::info!(
            event = "vn.commands.registry.create",
            "AstraVN standard command registry created"
        );
        Self {
            schema: "astra.vn.standard_command_manifest.v2".to_string(),
            provider_id: "astra.vn.standard_commands".to_string(),
            commands: vec![
                descriptor("background", "astra.command.background.v2"),
                descriptor("show", "astra.command.show.v2"),
                descriptor("hide", "astra.command.hide.v2"),
                descriptor("move", "astra.command.move.v2"),
                descriptor("camera", "astra.command.camera.v2"),
                descriptor("transition", "astra.command.transition.v2"),
                descriptor("shake", "astra.command.shake.v2"),
                descriptor("movie", "astra.command.movie.v2"),
                descriptor("voice", "astra.command.voice.v2"),
                descriptor("bgm", "astra.command.bgm.v2"),
                descriptor("se", "astra.command.se.v2"),
                descriptor("stage", "astra.command.stage.v2"),
                descriptor("layer", "astra.command.layer.v2"),
                descriptor("timeline", "astra.command.timeline.v2"),
                descriptor("effect", "astra.command.effect.v2"),
            ],
        }
    }

    pub fn validate_usage(&self, compiled: &CompiledStory) -> VnStandardCommandValidationReport {
        tracing::debug!(
            event = "vn.commands.validate.start",
            command_count = self.commands.len(),
            state_count = compiled.states.len(),
            "AstraVN command validation started"
        );
        let mut diagnostics = Vec::new();
        if self.schema != "astra.vn.standard_command_manifest.v2" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_STANDARD_COMMAND_SCHEMA",
                "standard command manifest schema is invalid",
            ));
        }
        if self.provider_id != "astra.vn.standard_commands" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_STANDARD_COMMAND_PROVIDER",
                "standard command manifest must use astra.vn.standard_commands",
            ));
        }

        let descriptors = self
            .commands
            .iter()
            .map(|descriptor| (descriptor.command.as_str(), descriptor))
            .collect::<BTreeMap<_, _>>();
        for required in ["show", "hide", "camera", "movie", "voice", "bgm", "se"] {
            if !descriptors.contains_key(required) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_STANDARD_COMMAND_REQUIRED",
                        "standard command manifest is missing a required command",
                    )
                    .with_field("command", required),
                );
            }
        }

        let mut checked_usage_count = 0usize;
        for command in compiled
            .states
            .values()
            .flat_map(|state| &state.scenes)
            .flat_map(|scene| &scene.commands)
        {
            let CompiledCommand::Presentation {
                id,
                command: PresentationCommand::Stage(stage),
            } = command
            else {
                continue;
            };
            checked_usage_count += 1;
            let command = stage.kind();
            let Some(_descriptor) = descriptors.get(command) else {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_STANDARD_COMMAND_UNKNOWN",
                        "compiled story uses a presentation command without a standard descriptor",
                    )
                    .with_field("command_id", id)
                    .with_field("command", command),
                );
                continue;
            };
            match stage {
                StageCommand::Movie {
                    end: VnMovieEndBehavior::Wait,
                    fence,
                    fallback,
                    ..
                } if fence.is_none() || fallback.is_none() => diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_STANDARD_COMMAND_FALLBACK",
                        "movie end:wait requires a fence and deterministic fallback frame",
                    )
                    .with_field("command_id", id),
                ),
                StageCommand::Timeline(TimelineCommand::Start(timeline))
                    if timeline.tracks.is_empty()
                        || timeline
                            .tracks
                            .iter()
                            .any(|track| track.keyframes.len() < 2) =>
                {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_STANDARD_TIMELINE_TRACK",
                            "timeline requires typed tracks with at least two keyframes",
                        )
                        .with_field("command_id", id),
                    );
                }
                _ => {}
            }
        }

        VnStandardCommandValidationReport {
            passed: diagnostics.is_empty(),
            diagnostics,
            command_count: self.commands.len(),
            checked_usage_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnStandardCommandDescriptor {
    pub command: String,
    pub schema: String,
    pub provider_id: String,
    pub release_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnStandardCommandValidationReport {
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub command_count: usize,
    pub checked_usage_count: usize,
}

fn descriptor(
    command: impl Into<String>,
    schema: impl Into<String>,
) -> VnStandardCommandDescriptor {
    VnStandardCommandDescriptor {
        command: command.into(),
        schema: schema.into(),
        provider_id: "astra.vn.standard_commands".to_string(),
        release_checks: vec![
            "vn.standard_commands".to_string(),
            "vn.presentation_provider".to_string(),
        ],
    }
}
