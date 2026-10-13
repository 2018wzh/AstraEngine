use super::*;

impl CompileBuilder {
    pub(super) fn resolved_edges_from(
        &self,
        state: &State,
        state_ids: &BTreeSet<String>,
    ) -> Vec<String> {
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
                CompiledCommand::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    targets.push(resolve_target(then_target, state_ids));
                    targets.push(resolve_target(else_target, state_ids));
                }
                _ => {}
            }
        }
        targets
    }

    pub(super) fn route_graph(&mut self) -> RouteGraph {
        // System stories are an out-of-band UI navigation graph. Their leaf states
        // must never become gameplay terminals or pollute route coverage evidence.
        let system_story_ids = self
            .stories
            .iter()
            .filter(|story| story.name == "system")
            .map(|story| story.id.as_str())
            .collect::<BTreeSet<_>>();
        let state_ids: BTreeSet<_> = self
            .states
            .values()
            .filter(|state| !system_story_ids.contains(state.story_id.as_str()))
            .map(|state| state.id.clone())
            .collect();
        let mut nodes = BTreeMap::<String, RouteNode>::new();
        let mut edges = Vec::new();
        for state in self.states.values() {
            if !state.id.starts_with("state.") || system_story_ids.contains(state.story_id.as_str())
            {
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
                    CompiledCommand::Branch {
                        id,
                        then_target,
                        else_target,
                        ..
                    } => {
                        for (suffix, target) in [("then", then_target), ("else", else_target)] {
                            let target = resolve_target(target, &state_ids);
                            edges.push(RouteEdge {
                                from: state.id.clone(),
                                to: target.clone(),
                                trigger: format!("{id}.{suffix}"),
                            });
                            nodes.entry(target.clone()).or_insert(RouteNode {
                                id: target.clone(),
                                label: target.clone(),
                                terminal: !state_ids.contains(&target),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        let states_with_outgoing_edges = edges
            .iter()
            .map(|edge| edge.from.as_str())
            .collect::<BTreeSet<_>>();
        for node in nodes.values_mut() {
            if state_ids.contains(&node.id)
                && !states_with_outgoing_edges.contains(node.id.as_str())
            {
                node.terminal = true;
            }
        }
        RouteGraph {
            schema: "astra.vn.route_graph.v1".to_string(),
            nodes: nodes.into_values().collect(),
            edges,
        }
    }

    pub(super) fn current_story(&self) -> Result<String, VnError> {
        self.current_story.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_STORY_CONTEXT",
                "command requires a current story context",
            )
        })
    }

    pub(super) fn current_state_mut(&mut self) -> Result<&mut State, VnError> {
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

    pub(super) fn current_scene_mut(&mut self) -> Result<&mut Scene, VnError> {
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

    pub(super) fn current_scene(&self) -> Option<&Scene> {
        let state_id = self.current_state.as_ref()?;
        let scene_id = self.current_scene.as_ref()?;
        self.states
            .get(state_id)?
            .scenes
            .iter()
            .find(|scene| &scene.id == scene_id)
    }
}

pub(super) fn semantic_story(compiled: &CompiledStory) -> CompiledStory {
    let mut semantic = compiled.clone();
    semantic.story_hash = Hash128::from_bytes([0; 16]);
    semantic.source_map = CommandSourceMap::default();
    semantic.debug_symbols.clear();
    for entry in &mut semantic.command_manifest.commands {
        entry.id.clear();
        entry.source = None;
    }
    for entry in semantic.system_story_manifest.entries.values_mut() {
        entry.source_id.clear();
    }
    for state in semantic.states.values_mut() {
        for scene in &mut state.scenes {
            for command in &mut scene.commands {
                match command {
                    CompiledCommand::Dialogue { id, .. }
                    | CompiledCommand::Choice { id, .. }
                    | CompiledCommand::Jump { id, .. }
                    | CompiledCommand::Branch { id, .. }
                    | CompiledCommand::Call { id, .. }
                    | CompiledCommand::Return { id }
                    | CompiledCommand::Mutate { id, .. }
                    | CompiledCommand::SystemPage { id, .. }
                    | CompiledCommand::Presentation { id, .. }
                    | CompiledCommand::Wait { id, .. }
                    | CompiledCommand::InputWait { id } => id.clear(),
                }
            }
        }
    }
    semantic
}

pub(super) fn required_attr(line: &ParsedLine, key: &str) -> Result<String, VnError> {
    line.attr(key)
        .map(str::to_string)
        .ok_or_else(|| VnError::diagnostic("ASTRA_VN_ATTR_MISSING", format!("{key} is missing")))
}

pub(super) fn validate_system_action_symbol(field: &str, value: &str) -> Result<(), VnError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(VnError::diagnostic(
            "ASTRA_VN_SYSTEM_ACTION_SYMBOL",
            format!("{field} must be a bounded safe symbol"),
        ));
    }
    Ok(())
}

pub(super) fn parse_variable_condition(
    line: &ParsedLine,
    value: &str,
) -> Result<VariableCondition, VnError> {
    let parts = value.split(',').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_CHOICE_CONDITION",
                "choice condition must use path,operator,integer",
            )
            .with_source(line.source_ref()),
        ));
    }
    let (scope, key) = parts[0].split_once('.').ok_or_else(|| {
        VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_CHOICE_CONDITION",
                "choice condition path needs scope.key",
            )
            .with_source(line.source_ref()),
        )
    })?;
    if !is_allowed_variable_scope(scope) || key.is_empty() {
        return Err(VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_CHOICE_CONDITION",
                "choice condition variable path is invalid",
            )
            .with_source(line.source_ref()),
        ));
    }
    let op = match parts[1] {
        "eq" => BranchOp::Eq,
        "not_eq" => BranchOp::NotEq,
        "less" => BranchOp::Less,
        "less_eq" => BranchOp::LessEq,
        "greater" => BranchOp::Greater,
        "greater_eq" => BranchOp::GreaterEq,
        _ => {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_CHOICE_CONDITION",
                    "choice condition operator is unsupported",
                )
                .with_source(line.source_ref()),
            ));
        }
    };
    let parsed = parts[2].parse::<i64>().map_err(|error| {
        VnError::Diagnostic(
            Diagnostic::blocking(
                "ASTRA_VN_CHOICE_CONDITION",
                "choice condition value must be an integer",
            )
            .with_source(line.source_ref())
            .with_field("error", error.to_string()),
        )
    })?;
    Ok(VariableCondition {
        scope: scope.to_string(),
        key: key.to_string(),
        op,
        value: parsed,
    })
}

pub(super) fn duplicate_id_diagnostic(id: &str, line: &ParsedLine) -> VnError {
    VnError::Diagnostic(
        Diagnostic::blocking("ASTRA_VN_DUPLICATE_ID", "compiled id is duplicated")
            .with_source(line.source_ref())
            .with_field("id", id),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TargetKind {
    Choice,
    Jump,
    Branch,
    Call,
}

impl TargetKind {
    pub(super) fn allows_terminal(self) -> bool {
        matches!(self, Self::Choice | Self::Jump | Self::Branch)
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Choice => "choice",
            Self::Jump => "jump",
            Self::Branch => "branch",
            Self::Call => "call",
        }
    }
}

pub(super) fn is_terminal_target(target: &str) -> bool {
    target.starts_with("ending.")
}

pub(super) fn is_allowed_variable_scope(scope: &str) -> bool {
    matches!(scope, "project" | "global" | "temp" | "system")
}

pub(super) fn command_manifest_identity(command: &CompiledCommand) -> (&str, &'static str) {
    match command {
        CompiledCommand::Dialogue { id, .. } => (id, "dialogue"),
        CompiledCommand::Choice { id, .. } => (id, "choice"),
        CompiledCommand::Jump { id, .. } => (id, "jump"),
        CompiledCommand::Branch { id, .. } => (id, "branch"),
        CompiledCommand::Call { id, .. } => (id, "call"),
        CompiledCommand::Return { id } => (id, "return"),
        CompiledCommand::Mutate { id, .. } => (id, "mutate"),
        CompiledCommand::SystemPage { id, .. } => (id, "system_page"),
        CompiledCommand::Presentation { id, .. } => (id, "presentation"),
        CompiledCommand::Wait { id, .. } => (id, "wait"),
        CompiledCommand::InputWait { id } => (id, "input_wait"),
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
