use super::*;

impl CompileBuilder {
    pub(super) fn push_system_action(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let id = line.source_id.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_SYSTEM_ACTION_ID",
                "system_action requires a stable #@id",
            )
        })?;
        validate_system_action_symbol("action id", &id)?;
        let mut effects = Vec::new();
        if line.attr("mutation").is_some() && line.attr("mutations").is_some() {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SYSTEM_ACTION_MUTATION_CONFLICT",
                "system_action cannot declare both mutation and mutations",
            ));
        }
        let mutations = line
            .attr("mutation")
            .into_iter()
            .chain(
                line.attr("mutations")
                    .into_iter()
                    .flat_map(|value| value.split(';')),
            )
            .map(str::trim)
            .collect::<Vec<_>>();
        if mutations.iter().any(|mutation| mutation.is_empty()) {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SYSTEM_ACTION_MUTATION",
                "mutations must not contain an empty effect",
            ));
        }
        for mutation in mutations {
            let fields = mutation.split(',').map(str::trim).collect::<Vec<_>>();
            if fields.len() != 4 {
                return Err(VnError::diagnostic(
                    "ASTRA_VN_SYSTEM_ACTION_MUTATION",
                    "each mutation must be scope,key,op,value",
                ));
            }
            validate_system_action_symbol("mutation scope", fields[0])?;
            validate_system_action_symbol("mutation key", fields[1])?;
            let op = match fields[2] {
                "set" => MutationOp::Set,
                "add" => MutationOp::Add,
                "sub" => MutationOp::Sub,
                _ => {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_SYSTEM_ACTION_MUTATION_OP",
                        "mutation op must be set, add or sub",
                    ))
                }
            };
            let value = fields[3].parse::<i64>().map_err(|_| {
                VnError::diagnostic(
                    "ASTRA_VN_SYSTEM_ACTION_MUTATION_VALUE",
                    "mutation value must be an i64",
                )
            })?;
            effects.push(SystemActionEffect::Mutate {
                scope: fields[0].to_string(),
                key: fields[1].to_string(),
                op,
                value,
            });
        }
        if let Some(target) = line.attr("jump") {
            validate_system_action_symbol("jump target", target)?;
            effects.push(SystemActionEffect::Jump {
                target: target.to_string(),
            });
        }
        if let Some(value) = line.attr("switch_page") {
            let page = SystemPageKind::parse(value);
            if page == SystemPageKind::Unknown {
                return Err(VnError::diagnostic(
                    "ASTRA_VN_SYSTEM_ACTION_PAGE",
                    "switch_page must name a built-in page",
                ));
            }
            effects.push(SystemActionEffect::SwitchSystemPage { page });
        }
        if let Some(value) = line.attr("return_system") {
            if value != "true" {
                return Err(VnError::diagnostic(
                    "ASTRA_VN_SYSTEM_ACTION_RETURN",
                    "return_system, when present, must be true",
                ));
            }
            effects.push(SystemActionEffect::ReturnSystem);
        }
        if effects.is_empty() || effects.len() > 8 {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SYSTEM_ACTION_EFFECTS",
                "system_action must contain between one and eight compiled effects",
            ));
        }
        let terminal_effects = effects
            .iter()
            .enumerate()
            .filter(|(_, effect)| {
                matches!(
                    effect,
                    SystemActionEffect::Jump { .. }
                        | SystemActionEffect::SwitchSystemPage { .. }
                        | SystemActionEffect::ReturnSystem
                )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if terminal_effects.len() > 1
            || terminal_effects
                .first()
                .is_some_and(|index| *index + 1 != effects.len())
        {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SYSTEM_ACTION_TERMINAL_EFFECT",
                "jump, switch_page or return_system may appear once and must be the final effect",
            ));
        }
        if self
            .system_actions
            .insert(id.clone(), SystemActionProgram { id, effects })
            .is_some()
        {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SYSTEM_ACTION_DUPLICATE",
                "system action id is duplicated",
            ));
        }
        Ok(())
    }

    pub(super) fn push_wait(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_input_wait(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        if !line.attrs.is_empty() || !line.args.is_empty() {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_INPUT_WAIT_ARGUMENT",
                    "input_wait does not accept arguments or attributes",
                )
                .with_source(line.source_ref()),
            ));
        }
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::InputWait {
                id: line.stable_id(),
            });
        Ok(())
    }

    pub(super) fn push_stage_presentation(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Presentation {
                id: line.stable_id(),
                command: PresentationCommand::Stage(compile_stage_command(line)?),
            });
        Ok(())
    }

    pub(super) fn push_extension_presentation(
        &mut self,
        line: &ParsedLine,
        descriptor: &ExtensionCommandDescriptor,
    ) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Presentation {
                id: line.stable_id(),
                command: PresentationCommand::Extension(compile_extension_command(
                    line, descriptor,
                )?),
            });
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<CompiledStory, VnError> {
        trace_pass(SemanticPass::Routes);
        self.validate_targets()?;
        self.validate_system_action_targets()?;
        self.validate_main_reachability()?;
        trace_pass(SemanticPass::Variables);
        self.validate_text_keys()?;
        let story_manifest = self.story_manifest();
        let variable_manifest = self.variable_manifest();
        let command_manifest = self.command_manifest();
        let route_graph = self.route_graph();
        let compiled = CompiledStory {
            schema: "astra.vn.story".to_string(),
            story_hash: Hash128::from_bytes([0; 16]),
            story_manifest,
            variable_manifest,
            command_manifest,
            system_story_manifest: SystemStoryManifest {
                actions: self.system_actions,
                ..SystemStoryManifest::empty()
            },
            stories: self.stories,
            states: self.states,
            route_graph,
            source_map: self.source_map,
            debug_symbols: self.debug_symbols,
        };
        Ok(compiled)
    }

    pub(super) fn story_manifest(&self) -> StoryManifest {
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

    pub(super) fn variable_manifest(&self) -> VariableManifest {
        let mut scopes = BTreeMap::<String, VariableScopeManifest>::new();
        for command in self.commands() {
            if let CompiledCommand::Mutate { scope, key, .. }
            | CompiledCommand::Branch { scope, key, .. } = command
            {
                scopes
                    .entry(scope.clone())
                    .or_insert_with(|| VariableScopeManifest {
                        keys: BTreeSet::new(),
                    })
                    .keys
                    .insert(key.clone());
            }
            if let CompiledCommand::Choice { options, .. } = command {
                for condition in options
                    .iter()
                    .filter_map(|option| option.enabled_when.as_ref())
                {
                    scopes
                        .entry(condition.scope.clone())
                        .or_insert_with(|| VariableScopeManifest {
                            keys: BTreeSet::new(),
                        })
                        .keys
                        .insert(condition.key.clone());
                }
            }
        }
        VariableManifest {
            schema: "astra.vn.variable_manifest.v1".to_string(),
            scopes,
        }
    }

    pub(super) fn command_manifest(&self) -> CommandManifest {
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

    pub(super) fn commands(&self) -> impl Iterator<Item = &CompiledCommand> {
        self.states
            .values()
            .flat_map(|state| state.scenes.iter())
            .flat_map(|scene| scene.commands.iter())
    }

    pub(super) fn validate_targets(&self) -> Result<(), VnError> {
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
                    CompiledCommand::Branch {
                        id,
                        then_target,
                        else_target,
                        ..
                    } => {
                        self.validate_target_ref(&state_ids, id, then_target, TargetKind::Branch)?;
                        self.validate_target_ref(&state_ids, id, else_target, TargetKind::Branch)?;
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

    pub(super) fn validate_text_keys(&self) -> Result<(), VnError> {
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

    pub(super) fn validate_target_ref(
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

    pub(super) fn validate_main_reachability(&self) -> Result<(), VnError> {
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

    pub(super) fn validate_system_action_targets(&self) -> Result<(), VnError> {
        let state_ids = self.states.keys().cloned().collect::<BTreeSet<_>>();
        for action in self.system_actions.values() {
            for effect in &action.effects {
                let SystemActionEffect::Jump { target } = effect else {
                    continue;
                };
                let resolved = resolve_target(target, &state_ids);
                if !state_ids.contains(&resolved) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_SYSTEM_ACTION_TARGET",
                        format!("system action {} targets unknown state {target}", action.id),
                    ));
                }
            }
        }
        Ok(())
    }
}
