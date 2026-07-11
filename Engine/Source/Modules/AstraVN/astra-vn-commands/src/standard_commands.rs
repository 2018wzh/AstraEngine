use std::collections::{BTreeMap, BTreeSet};

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CompiledCommand, CompiledStory, PresentationCommand};

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
            schema: "astra.vn.standard_command_manifest.v1".to_string(),
            provider_id: "astra.vn.standard_commands".to_string(),
            commands: vec![
                descriptor("background", "astra.command.background.v1", &["asset"]),
                descriptor("show", "astra.command.show.v1", &[]),
                descriptor("hide", "astra.command.hide.v1", &[]),
                descriptor("move", "astra.command.move.v1", &[]),
                descriptor("camera", "astra.command.camera.v1", &[]),
                descriptor("transition", "astra.command.transition.v1", &[]),
                descriptor("shake", "astra.command.shake.v1", &[]),
                descriptor("movie", "astra.command.movie.v1", &["asset", "layer"]),
                descriptor("voice", "astra.command.voice.v1", &["asset"]),
                descriptor("bgm", "astra.command.bgm.v1", &["asset"]),
                descriptor("se", "astra.command.se.v1", &["asset"]),
                descriptor(
                    "audio",
                    "astra.command.audio_control.v1",
                    &["action", "target"],
                ),
                descriptor("stage", "astra.command.stage.v1", &[]),
                descriptor("layer", "astra.command.layer.v1", &[]),
                descriptor("timeline", "astra.command.timeline.v1", &[]),
                descriptor("task", "astra.command.task.v1", &[]),
                descriptor("effect", "astra.command.effect.v1", &[]),
                descriptor("fence", "astra.command.fence.v1", &[]),
                descriptor("command", "astra.command.custom_binding.v1", &[]),
                descriptor("bind_setting", "astra.command.bind_setting.v1", &[]),
                descriptor("source", "astra.command.source.v1", &[]),
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
        if self.schema != "astra.vn.standard_command_manifest.v1" {
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
        for required in [
            "show", "hide", "move", "camera", "movie", "voice", "bgm", "se", "audio",
        ] {
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
                command:
                    PresentationCommand::Stage {
                        command,
                        attributes,
                    },
            } = command
            else {
                continue;
            };
            checked_usage_count += 1;
            let Some(descriptor) = descriptors.get(command.as_str()) else {
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
            for required in &descriptor.required_attrs {
                if !attributes.contains_key(required) {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_VN_STANDARD_COMMAND_ATTR",
                            "standard command usage is missing a required attribute",
                        )
                        .with_field("command_id", id)
                        .with_field("command", command)
                        .with_field("attribute", required),
                    );
                }
            }
            if command == "movie"
                && attributes
                    .get("end")
                    .is_some_and(|value| value.eq_ignore_ascii_case("wait"))
                && !attributes.contains_key("fallback")
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_STANDARD_COMMAND_FALLBACK",
                        "movie end:wait requires a deterministic fallback frame",
                    )
                    .with_field("command_id", id),
                );
            }
            if command == "audio"
                && attributes
                    .get("action")
                    .is_some_and(|action| !matches!(action.as_str(), "pause" | "resume" | "stop"))
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_AUDIO_CONTROL_ACTION",
                        "audio control action must be pause, resume, or stop",
                    )
                    .with_field("command_id", id),
                );
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
    pub required_attrs: BTreeSet<String>,
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
    required_attrs: &[&str],
) -> VnStandardCommandDescriptor {
    VnStandardCommandDescriptor {
        command: command.into(),
        schema: schema.into(),
        provider_id: "astra.vn.standard_commands".to_string(),
        required_attrs: required_attrs
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        release_checks: vec![
            "vn.standard_commands".to_string(),
            "vn.presentation_provider".to_string(),
        ],
    }
}
