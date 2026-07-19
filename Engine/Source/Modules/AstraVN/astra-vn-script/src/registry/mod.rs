use std::collections::BTreeMap;

use astra_core::Diagnostic;

use crate::{ExtensionCommandDescriptor, VnError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandProvider {
    Core,
    Standard,
    Extension(ExtensionCommandDescriptor),
}

#[derive(Debug, Clone)]
pub struct CommandRegistry {
    commands: BTreeMap<String, CommandProvider>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        let mut registry = Self {
            commands: BTreeMap::new(),
        };
        for command in [
            "story",
            "state",
            "scene",
            "text",
            "choice",
            "option",
            "jump",
            "branch",
            "call",
            "return",
            "mutate",
            "system_page",
            "wait",
            "input_wait",
        ] {
            registry
                .commands
                .insert(command.into(), CommandProvider::Core);
        }
        for command in [
            "preload",
            "background",
            "show",
            "hide",
            "clear_layer",
            "layer_visibility",
            "backdrop",
            "shade",
            "skip_allowed",
            "move",
            "camera",
            "movie",
            "voice",
            "bgm",
            "se",
            "audio",
            "transition",
            "shake",
            "stage",
            "layer",
            "timeline",
            "effect",
        ] {
            registry
                .commands
                .insert(command.into(), CommandProvider::Standard);
        }
        registry
    }
}

impl CommandRegistry {
    pub fn bind_extension(
        &mut self,
        descriptor: ExtensionCommandDescriptor,
    ) -> Result<(), VnError> {
        descriptor.validate().map_err(VnError::Diagnostic)?;
        if self.commands.contains_key(&descriptor.command) {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_COMMAND_BINDING_CONFLICT",
                    "command already has an authoritative provider binding",
                )
                .with_field("command", &descriptor.command)
                .with_field("provider", &descriptor.provider_id),
            ));
        }
        self.commands.insert(
            descriptor.command.clone(),
            CommandProvider::Extension(descriptor),
        );
        Ok(())
    }

    pub fn provider(&self, command: &str) -> Option<&CommandProvider> {
        self.commands.get(command)
    }

    pub fn is_known(&self, command: &str) -> bool {
        self.commands.contains_key(command)
    }
}
