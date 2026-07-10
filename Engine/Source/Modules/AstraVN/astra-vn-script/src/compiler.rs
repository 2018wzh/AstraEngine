use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, Hash128, SourceRef};

use crate::{
    parser::{parse_sources, ParsedLine},
    AstraSource, ChoiceOption, CommandManifest, CommandManifestEntry, CompiledCommand,
    CompiledStory, MutationOp, PresentationCommand, RouteEdge, RouteGraph, RouteNode, Scene, State,
    Story, StoryManifest, StoryManifestEntry, SystemPageKind, SystemStoryManifest,
    VariableManifest, VariableScopeManifest, VnError,
};

pub fn compile_astra_sources<I>(sources: I) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = AstraSource>,
{
    tracing::info!(event = "vn.compile.start", "AstraVN compilation started");
    let lines = parse_sources(sources)?;
    let mut builder = CompileBuilder::default();
    for line in &lines {
        builder.validate_structure(line)?;
        builder.record_source(line)?;
        match line.keyword.as_str() {
            "story" => builder.start_story(line)?,
            "state" => builder.start_state(line)?,
            "scene" => builder.start_scene(line)?,
            "text" => builder.push_dialogue(line)?,
            "choice" => builder.push_choice(line)?,
            "option" => builder.push_option(line)?,
            "jump" => builder.push_jump(line)?,
            "call" => builder.push_call(line)?,
            "return" => builder.push_return(line)?,
            "mutate" => builder.push_mutate(line)?,
            "system_page" => builder.push_system_page(line)?,
            "wait" => builder.push_wait(line)?,
            "background" | "show" | "hide" | "camera" | "movie" | "voice" | "bgm" | "se"
            | "transition" | "shake" | "stage" | "layer" | "timeline" | "task" | "effect"
            | "fence" | "command" | "bind_setting" | "source" => {
                builder.push_presentation(line)?;
            }
            _ => builder.push_presentation(line)?,
        }
    }
    let compiled = builder.finish()?;
    tracing::info!(
        event = "vn.compile.complete",
        story_count = compiled.stories.len(),
        state_count = compiled.states.len(),
        source_entry_count = compiled.source_map.len(),
        "AstraVN compilation completed"
    );
    Ok(compiled)
}

#[derive(Default)]
struct CompileBuilder {
    stories: Vec<Story>,
    states: BTreeMap<String, State>,
    current_story: Option<String>,
    current_state: Option<String>,
    current_scene: Option<String>,
    source_map: BTreeMap<String, SourceRef>,
    debug_symbols: BTreeMap<String, String>,
}

impl CompileBuilder {
    fn validate_structure(&self, line: &ParsedLine) -> Result<(), VnError> {
        let valid = match line.keyword.as_str() {
            "story" | "state" => line.indent == 0,
            "scene" => line.indent == 2,
            "option" => {
                let last_command = self.current_scene().and_then(|scene| scene.commands.last());
                (line.indent == 6 && matches!(last_command, Some(CompiledCommand::Choice { .. })))
                    || (line.indent == 4
                        && matches!(last_command, Some(CompiledCommand::SystemPage { .. })))
            }
            _ if self.current_scene.is_none() => true,
            _ => line.indent == 4,
        };
        if valid {
            return Ok(());
        }
        let expected = match line.keyword.as_str() {
            "story" | "state" => "0",
            "scene" => "2",
            "option" => "6 below choice, or 4 immediately after system_page",
            _ => "4",
        };
        Err(VnError::Diagnostic(
            Diagnostic::blocking(
                if line.keyword == "option" {
                    "ASTRA_VN_OPTION_CONTEXT"
                } else {
                    "ASTRA_VN_STRUCTURE_INDENT"
                },
                "source indentation does not match the canonical story structure",
            )
            .with_source(line.source_ref())
            .with_field("keyword", &line.keyword)
            .with_field("indent", line.indent)
            .with_field("expected_indent", expected),
        ))
    }

    fn record_source(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let id = line.stable_id();
        if let Some(first) = self.source_map.get(&id) {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking("ASTRA_VN_DUPLICATE_ID", "source id is duplicated")
                    .with_source(line.source_ref())
                    .with_field("id", &id)
                    .with_field("first_source", &first.source)
                    .with_field("first_line", first.line),
            ));
        }
        self.source_map.insert(id.clone(), line.source_ref());
        self.debug_symbols.insert(id, line.keyword.clone());
        Ok(())
    }

    fn start_story(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let name =
            line.args.first().cloned().ok_or_else(|| {
                VnError::diagnostic("ASTRA_VN_STORY_NAME", "story name is missing")
            })?;
        let id = line
            .source_id
            .clone()
            .unwrap_or_else(|| format!("story.{name}"));
        if self.stories.iter().any(|story| story.id == id) {
            return Err(duplicate_id_diagnostic(&id, line));
        }
        self.stories.push(Story {
            id: id.clone(),
            name,
            states: Vec::new(),
        });
        self.current_story = Some(id);
        self.current_state = None;
        self.current_scene = None;
        Ok(())
    }

    fn start_state(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let story_id = self.current_story()?;
        let name =
            line.args.first().cloned().ok_or_else(|| {
                VnError::diagnostic("ASTRA_VN_STATE_NAME", "state name is missing")
            })?;
        let id = line
            .source_id
            .clone()
            .unwrap_or_else(|| format!("state.{name}"));
        if self.states.contains_key(&id) {
            return Err(duplicate_id_diagnostic(&id, line));
        }
        self.states.insert(
            id.clone(),
            State {
                id: id.clone(),
                name,
                story_id: story_id.clone(),
                scenes: Vec::new(),
            },
        );
        if let Some(story) = self.stories.iter_mut().find(|story| story.id == story_id) {
            story.states.push(id.clone());
        }
        self.current_state = Some(id);
        self.current_scene = None;
        Ok(())
    }

    fn start_scene(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        if self.current_state.is_none() {
            let story_id = self.current_story()?;
            let story_name = self
                .stories
                .iter()
                .find(|story| story.id == story_id)
                .map(|story| story.name.clone())
                .unwrap_or_else(|| "system".to_string());
            let id = story_id.clone();
            self.states.entry(id.clone()).or_insert_with(|| State {
                id: id.clone(),
                name: story_name,
                story_id: story_id.clone(),
                scenes: Vec::new(),
            });
            if let Some(story) = self.stories.iter_mut().find(|story| story.id == story_id) {
                if !story.states.contains(&id) {
                    story.states.push(id.clone());
                }
            }
            self.current_state = Some(id);
        }
        let name =
            line.args.first().cloned().ok_or_else(|| {
                VnError::diagnostic("ASTRA_VN_SCENE_NAME", "scene name is missing")
            })?;
        let id = line
            .source_id
            .clone()
            .unwrap_or_else(|| format!("scene.{name}"));
        let state = self.current_state_mut()?;
        if state.scenes.iter().any(|scene| scene.id == id) {
            return Err(duplicate_id_diagnostic(&id, line));
        }
        state.scenes.push(Scene {
            id: id.clone(),
            name,
            commands: Vec::new(),
        });
        self.current_scene = Some(id);
        Ok(())
    }

    fn push_dialogue(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let key = required_attr(line, "key")?;
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Dialogue {
                id: line.stable_id(),
                key,
                speaker: line.attr("speaker").map(str::to_string),
                voice: line.attr("voice").map(str::to_string),
                window: line.attr("window").map(str::to_string),
            });
        Ok(())
    }

    fn push_choice(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let key = required_attr(line, "key")?;
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Choice {
                id: line.stable_id(),
                key,
                options: Vec::new(),
            });
        Ok(())
    }

    fn push_option(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let option = ChoiceOption {
            id: line.stable_id(),
            key: required_attr(line, "key")?,
            target: required_attr(line, "target")?,
        };
        let commands = &mut self.current_scene_mut()?.commands;
        if let Some(CompiledCommand::Choice { options, .. }) = commands.last_mut() {
            options.push(option);
            Ok(())
        } else if matches!(commands.last(), Some(CompiledCommand::SystemPage { .. })) {
            commands.push(CompiledCommand::Presentation {
                id: option.id.clone(),
                command: PresentationCommand::Stage {
                    command: "option".to_string(),
                    attributes: BTreeMap::from([
                        ("key".to_string(), option.key),
                        ("target".to_string(), option.target),
                    ]),
                },
            });
            Ok(())
        } else {
            Err(VnError::Diagnostic(
                Diagnostic::blocking("ASTRA_VN_OPTION_CONTEXT", "option must belong to a choice")
                    .with_source(line.source_ref()),
            ))
        }
    }

    fn push_jump(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let target = line
            .args
            .first()
            .map(String::as_str)
            .or_else(|| line.attr("target"))
            .map(str::to_string)
            .ok_or_else(|| VnError::diagnostic("ASTRA_VN_JUMP_TARGET", "jump target is missing"))?;
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Jump {
                id: line.stable_id(),
                target,
            });
        Ok(())
    }

    fn push_call(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let target = line
            .args
            .first()
            .map(String::as_str)
            .or_else(|| line.attr("target"))
            .map(str::to_string)
            .ok_or_else(|| VnError::diagnostic("ASTRA_VN_CALL_TARGET", "call target is missing"))?;
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Call {
                id: line.stable_id(),
                target,
            });
        Ok(())
    }

    fn push_return(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Return {
                id: line.stable_id(),
            });
        Ok(())
    }

    fn push_mutate(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let path = line
            .args
            .first()
            .ok_or_else(|| VnError::diagnostic("ASTRA_VN_MUTATE_PATH", "mutate path is missing"))?;
        let (scope, key) = path.split_once('.').ok_or_else(|| {
            VnError::diagnostic("ASTRA_VN_MUTATE_PATH", "mutate path needs scope.key")
        })?;
        if !is_allowed_variable_scope(scope) {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking("ASTRA_VN_VARIABLE_SCOPE", "variable scope is not allowed")
                    .with_source(line.source_ref())
                    .with_field("scope", scope)
                    .with_field("allowed", "project,global,temp,system"),
            ));
        }
        let op = match line.args.get(1).map(String::as_str) {
            Some("+=") => MutationOp::Add,
            Some("-=") => MutationOp::Sub,
            _ => MutationOp::Set,
        };
        let raw_value = line.args.get(2).ok_or_else(|| {
            VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_MUTATE_VALUE",
                    "mutate command requires an integer value",
                )
                .with_source(line.source_ref()),
            )
        })?;
        let value = raw_value.parse::<i64>().map_err(|_| {
            VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_MUTATE_VALUE",
                    "mutate value must be a valid integer",
                )
                .with_source(line.source_ref())
                .with_field("value", raw_value),
            )
        })?;
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Mutate {
                id: line.stable_id(),
                scope: scope.to_string(),
                key: key.to_string(),
                op,
                value,
                reason: line.attr("reason").map(str::to_string),
            });
        Ok(())
    }

    fn push_system_page(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::SystemPage {
                id: line.stable_id(),
                page: SystemPageKind::parse(line.attr("kind").unwrap_or("unknown")),
                policy: line.attr("policy").map(str::to_string),
            });
        Ok(())
    }

    fn push_wait(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Wait {
                id: line.stable_id(),
                fence: line
                    .attr("fence")
                    .or_else(|| line.attr("wait_for"))
                    .unwrap_or("timeline")
                    .to_string(),
            });
        Ok(())
    }

    fn push_presentation(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Presentation {
                id: line.stable_id(),
                command: PresentationCommand::Stage {
                    command: line.keyword.clone(),
                    attributes: line.attrs.clone(),
                },
            });
        Ok(())
    }

    fn finish(mut self) -> Result<CompiledStory, VnError> {
        self.validate_targets()?;
        self.validate_main_reachability()?;
        self.validate_text_keys()?;
        let story_manifest = self.story_manifest();
        let variable_manifest = self.variable_manifest();
        let command_manifest = self.command_manifest();
        let route_graph = self.route_graph();
        let mut compiled = CompiledStory {
            schema: "astra.vn.compiled_story.v1".to_string(),
            story_hash: Hash128::from_bytes([0; 16]),
            story_manifest,
            variable_manifest,
            command_manifest,
            system_story_manifest: SystemStoryManifest::empty(),
            stories: self.stories,
            states: self.states,
            route_graph,
            source_map: self.source_map,
            debug_symbols: self.debug_symbols,
        };
        compiled.system_story_manifest = SystemStoryManifest::from_compiled(&compiled)?;
        let bytes = postcard::to_allocvec(&compiled)?;
        compiled.story_hash = Hash128::from_blake3(&bytes);
        Ok(compiled)
    }

    fn story_manifest(&self) -> StoryManifest {
        StoryManifest {
            schema: "astra.vn.story_manifest.v1".to_string(),
            stories: self
                .stories
                .iter()
                .map(|story| StoryManifestEntry {
                    id: story.id.clone(),
                    name: story.name.clone(),
                    states: story.states.clone(),
                })
                .collect(),
        }
    }

    fn variable_manifest(&self) -> VariableManifest {
        let mut scopes = BTreeMap::<String, VariableScopeManifest>::new();
        for command in self.commands() {
            if let CompiledCommand::Mutate { scope, key, .. } = command {
                scopes
                    .entry(scope.clone())
                    .or_insert_with(|| VariableScopeManifest {
                        keys: BTreeSet::new(),
                    })
                    .keys
                    .insert(key.clone());
            }
        }
        VariableManifest {
            schema: "astra.vn.variable_manifest.v1".to_string(),
            scopes,
        }
    }

    fn command_manifest(&self) -> CommandManifest {
        let mut commands = Vec::new();
        for story in &self.stories {
            for state_id in &story.states {
                let Some(state) = self.states.get(state_id) else {
                    continue;
                };
                for scene in &state.scenes {
                    for command in &scene.commands {
                        let (id, kind) = command_manifest_identity(command);
                        commands.push(CommandManifestEntry {
                            id: id.to_string(),
                            kind: kind.to_string(),
                            story_id: story.id.clone(),
                            state_id: state.id.clone(),
                            scene_id: scene.id.clone(),
                            source: self.source_map.get(id).cloned(),
                        });
                    }
                }
            }
        }
        CommandManifest {
            schema: "astra.vn.command_manifest.v1".to_string(),
            commands,
        }
    }

    fn commands(&self) -> impl Iterator<Item = &CompiledCommand> {
        self.states
            .values()
            .flat_map(|state| state.scenes.iter())
            .flat_map(|scene| scene.commands.iter())
    }

    fn validate_targets(&self) -> Result<(), VnError> {
        let state_ids: BTreeSet<_> = self.states.keys().cloned().collect();
        for state in self.states.values() {
            for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
                match command {
                    CompiledCommand::Choice { options, .. } => {
                        for option in options {
                            self.validate_target_ref(
                                &state_ids,
                                &option.id,
                                &option.target,
                                TargetKind::Choice,
                            )?;
                        }
                    }
                    CompiledCommand::Jump { id, target } => {
                        self.validate_target_ref(&state_ids, id, target, TargetKind::Jump)?;
                    }
                    CompiledCommand::Call { id, target } => {
                        self.validate_target_ref(&state_ids, id, target, TargetKind::Call)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn validate_text_keys(&self) -> Result<(), VnError> {
        let mut seen = BTreeMap::<String, String>::new();
        for command in self.commands() {
            if let CompiledCommand::Dialogue { id, key, .. } = command {
                if let Some(first_id) = seen.get(key) {
                    let mut diagnostic = Diagnostic::blocking(
                        "ASTRA_VN_TEXT_KEY_DUPLICATE",
                        "dialogue text key is duplicated",
                    )
                    .with_field("key", key)
                    .with_field("first_command_id", first_id)
                    .with_field("command_id", id);
                    if let Some(source) = self.source_map.get(id) {
                        diagnostic = diagnostic.with_source(source.clone());
                    }
                    return Err(VnError::Diagnostic(diagnostic));
                }
                seen.insert(key.clone(), id.clone());
            }
        }
        Ok(())
    }

    fn validate_target_ref(
        &self,
        state_ids: &BTreeSet<String>,
        command_id: &str,
        target: &str,
        kind: TargetKind,
    ) -> Result<(), VnError> {
        let resolved = resolve_target(target, state_ids);
        if state_ids.contains(&resolved)
            || (kind.allows_terminal() && is_terminal_target(&resolved))
        {
            return Ok(());
        }

        let mut diagnostic = Diagnostic::blocking(
            "ASTRA_VN_TARGET_UNDEFINED",
            "route target does not resolve to a compiled state",
        )
        .with_field("command_id", command_id)
        .with_field("target", target)
        .with_field("resolved", &resolved)
        .with_field("kind", kind.as_str());
        if let Some(source) = self.source_map.get(command_id) {
            diagnostic = diagnostic.with_source(source.clone());
        }
        Err(VnError::Diagnostic(diagnostic))
    }

    fn validate_main_reachability(&self) -> Result<(), VnError> {
        let Some(story) = self
            .stories
            .iter()
            .find(|story| story.id == "story.main" || story.name == "main")
        else {
            return Ok(());
        };
        if story.states.len() <= 1 {
            return Ok(());
        }

        let state_ids: BTreeSet<_> = self.states.keys().cloned().collect();
        let mut reached = BTreeSet::new();
        let mut pending = vec![story.states[0].clone()];
        while let Some(state_id) = pending.pop() {
            if !reached.insert(state_id.clone()) {
                continue;
            }
            let Some(state) = self.states.get(&state_id) else {
                continue;
            };
            for target in self.resolved_edges_from(state, &state_ids) {
                if story.states.contains(&target) && !reached.contains(&target) {
                    pending.push(target);
                }
            }
        }

        if let Some(unreachable) = story
            .states
            .iter()
            .find(|state_id| !reached.contains(*state_id))
        {
            let mut diagnostic = Diagnostic::blocking(
                "ASTRA_VN_UNREACHABLE_STATE",
                "main story state is not reachable from the first state",
            )
            .with_field("story_id", &story.id)
            .with_field("state_id", unreachable);
            if let Some(source) = self.source_map.get(unreachable) {
                diagnostic = diagnostic.with_source(source.clone());
            }
            return Err(VnError::Diagnostic(diagnostic));
        }
        Ok(())
    }

    fn resolved_edges_from(&self, state: &State, state_ids: &BTreeSet<String>) -> Vec<String> {
        let mut targets = Vec::new();
        for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
            match command {
                CompiledCommand::Choice { options, .. } => {
                    targets.extend(
                        options
                            .iter()
                            .map(|option| resolve_target(&option.target, state_ids)),
                    );
                }
                CompiledCommand::Jump { target, .. } | CompiledCommand::Call { target, .. } => {
                    targets.push(resolve_target(target, state_ids));
                }
                _ => {}
            }
        }
        targets
    }

    fn route_graph(&mut self) -> RouteGraph {
        let state_ids: BTreeSet<_> = self.states.keys().cloned().collect();
        let mut nodes = BTreeMap::<String, RouteNode>::new();
        let mut edges = Vec::new();
        for state in self.states.values() {
            if !state.id.starts_with("state.") {
                continue;
            }
            nodes.insert(
                state.id.clone(),
                RouteNode {
                    id: state.id.clone(),
                    label: state.name.clone(),
                    terminal: false,
                },
            );
            for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
                match command {
                    CompiledCommand::Choice { options, .. } => {
                        for option in options {
                            let target = resolve_target(&option.target, &state_ids);
                            edges.push(RouteEdge {
                                from: state.id.clone(),
                                to: target.clone(),
                                trigger: option.id.clone(),
                            });
                            nodes.entry(target.clone()).or_insert(RouteNode {
                                id: target.clone(),
                                label: target,
                                terminal: false,
                            });
                        }
                    }
                    CompiledCommand::Jump { id, target } | CompiledCommand::Call { id, target } => {
                        let target = resolve_target(target, &state_ids);
                        edges.push(RouteEdge {
                            from: state.id.clone(),
                            to: target.clone(),
                            trigger: id.clone(),
                        });
                        nodes.entry(target.clone()).or_insert(RouteNode {
                            id: target.clone(),
                            label: target.clone(),
                            terminal: !state_ids.contains(&target),
                        });
                    }
                    _ => {}
                }
            }
        }
        RouteGraph {
            schema: "astra.vn.route_graph.v1".to_string(),
            nodes: nodes.into_values().collect(),
            edges,
        }
    }

    fn current_story(&self) -> Result<String, VnError> {
        self.current_story.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_STORY_CONTEXT",
                "command requires a current story context",
            )
        })
    }

    fn current_state_mut(&mut self) -> Result<&mut State, VnError> {
        let state_id = self.current_state.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_STATE_CONTEXT",
                "command requires a current state context",
            )
        })?;
        self.states.get_mut(&state_id).ok_or_else(|| {
            VnError::diagnostic("ASTRA_VN_STATE_CONTEXT", "current state is missing")
        })
    }

    fn current_scene_mut(&mut self) -> Result<&mut Scene, VnError> {
        let scene_id = self.current_scene.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_SCENE_CONTEXT",
                "command requires a current scene context",
            )
        })?;
        self.current_state_mut()?
            .scenes
            .iter_mut()
            .find(|scene| scene.id == scene_id)
            .ok_or_else(|| {
                VnError::diagnostic("ASTRA_VN_SCENE_CONTEXT", "current scene is missing")
            })
    }

    fn current_scene(&self) -> Option<&Scene> {
        let state_id = self.current_state.as_ref()?;
        let scene_id = self.current_scene.as_ref()?;
        self.states
            .get(state_id)?
            .scenes
            .iter()
            .find(|scene| &scene.id == scene_id)
    }
}

fn required_attr(line: &ParsedLine, key: &str) -> Result<String, VnError> {
    line.attr(key)
        .map(str::to_string)
        .ok_or_else(|| VnError::diagnostic("ASTRA_VN_ATTR_MISSING", format!("{key} is missing")))
}

fn duplicate_id_diagnostic(id: &str, line: &ParsedLine) -> VnError {
    VnError::Diagnostic(
        Diagnostic::blocking("ASTRA_VN_DUPLICATE_ID", "compiled id is duplicated")
            .with_source(line.source_ref())
            .with_field("id", id),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    Choice,
    Jump,
    Call,
}

impl TargetKind {
    fn allows_terminal(self) -> bool {
        matches!(self, Self::Choice | Self::Jump)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Choice => "choice",
            Self::Jump => "jump",
            Self::Call => "call",
        }
    }
}

fn is_terminal_target(target: &str) -> bool {
    target.starts_with("ending.")
}

fn is_allowed_variable_scope(scope: &str) -> bool {
    matches!(scope, "project" | "global" | "temp" | "system")
}

fn command_manifest_identity(command: &CompiledCommand) -> (&str, &'static str) {
    match command {
        CompiledCommand::Dialogue { id, .. } => (id, "dialogue"),
        CompiledCommand::Choice { id, .. } => (id, "choice"),
        CompiledCommand::Jump { id, .. } => (id, "jump"),
        CompiledCommand::Call { id, .. } => (id, "call"),
        CompiledCommand::Return { id } => (id, "return"),
        CompiledCommand::Mutate { id, .. } => (id, "mutate"),
        CompiledCommand::SystemPage { id, .. } => (id, "system_page"),
        CompiledCommand::Presentation { id, .. } => (id, "presentation"),
        CompiledCommand::Wait { id, .. } => (id, "wait"),
    }
}

pub fn resolve_target(target: &str, state_ids: &BTreeSet<String>) -> String {
    if let Some((_, state)) = target.split_once(':') {
        return state.to_string();
    }
    if state_ids.contains(target) {
        return target.to_string();
    }
    let state_target = format!("state.{target}");
    if state_ids.contains(&state_target) {
        return state_target;
    }
    target.to_string()
}
