use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandProvider {
    Core,
    Standard,
    Extension(String),
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
            "call",
            "return",
            "mutate",
            "system_page",
            "wait",
        ] {
            registry
                .commands
                .insert(command.into(), CommandProvider::Core);
        }
        for command in [
            "background",
            "show",
            "hide",
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
            "task",
            "effect",
            "fence",
            "command",
            "bind_setting",
            "source",
        ] {
            registry
                .commands
                .insert(command.into(), CommandProvider::Standard);
        }
        registry
    }
}

impl CommandRegistry {
    pub fn bind_extension(&mut self, command: impl Into<String>, provider: impl Into<String>) {
        self.commands
            .insert(command.into(), CommandProvider::Extension(provider.into()));
    }

    pub fn provider(&self, command: &str) -> Option<&CommandProvider> {
        self.commands.get(command)
    }

    pub fn is_known(&self, command: &str) -> bool {
        self.commands.contains_key(command)
    }
}
