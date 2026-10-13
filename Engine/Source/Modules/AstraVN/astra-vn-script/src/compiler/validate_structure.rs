use super::*;

impl CompileBuilder {
    pub(super) fn validate_structure(&self, line: &ParsedLine) -> Result<(), VnError> {
        let valid = match line.keyword.as_str() {
            "story" | "state" | "system_action" => line.indent == 0,
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

    pub(super) fn record_source(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn start_story(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn start_state(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn start_scene(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_dialogue(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_choice(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_option(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let enabled_when = line
            .attr("when")
            .map(|value| parse_variable_condition(line, value))
            .transpose()?;
        let option = ChoiceOption {
            id: line.stable_id(),
            key: required_attr(line, "key")?,
            target: required_attr(line, "target")?,
            enabled_when,
        };
        let commands = &mut self.current_scene_mut()?.commands;
        if let Some(CompiledCommand::Choice { options, .. }) = commands.last_mut() {
            options.push(option);
            Ok(())
        } else if matches!(commands.last(), Some(CompiledCommand::SystemPage { .. })) {
            commands.push(CompiledCommand::Presentation {
                id: option.id.clone(),
                command: PresentationCommand::SystemOption { option },
            });
            Ok(())
        } else {
            Err(VnError::Diagnostic(
                Diagnostic::blocking("ASTRA_VN_OPTION_CONTEXT", "option must belong to a choice")
                    .with_source(line.source_ref()),
            ))
        }
    }

    pub(super) fn push_jump(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_branch(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        let path = required_attr(line, "path")?;
        let (scope, key) = path.split_once('.').ok_or_else(|| {
            VnError::diagnostic("ASTRA_VN_BRANCH_PATH", "branch path needs scope.key")
        })?;
        if !is_allowed_variable_scope(scope) {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking("ASTRA_VN_VARIABLE_SCOPE", "variable scope is not allowed")
                    .with_source(line.source_ref())
                    .with_field("scope", scope)
                    .with_field("allowed", "project,global,temp,system"),
            ));
        }
        if key.is_empty() {
            return Err(VnError::Diagnostic(
                Diagnostic::blocking("ASTRA_VN_BRANCH_PATH", "branch variable key is empty")
                    .with_source(line.source_ref()),
            ));
        }
        let op = match required_attr(line, "op")?.as_str() {
            "eq" => BranchOp::Eq,
            "not_eq" => BranchOp::NotEq,
            "less" => BranchOp::Less,
            "less_eq" => BranchOp::LessEq,
            "greater" => BranchOp::Greater,
            "greater_eq" => BranchOp::GreaterEq,
            op => {
                return Err(VnError::Diagnostic(
                    Diagnostic::blocking("ASTRA_VN_BRANCH_OP", "branch operator is not supported")
                        .with_source(line.source_ref())
                        .with_field("operator", op)
                        .with_field("allowed", "eq,not_eq,less,less_eq,greater,greater_eq"),
                ));
            }
        };
        let raw_value = required_attr(line, "value")?;
        let value = raw_value.parse::<i64>().map_err(|error| {
            VnError::Diagnostic(
                Diagnostic::blocking(
                    "ASTRA_VN_BRANCH_VALUE",
                    "branch comparison value must be an integer",
                )
                .with_source(line.source_ref())
                .with_field("value", raw_value)
                .with_field("error", error.to_string()),
            )
        })?;
        let then_target = required_attr(line, "then")?;
        let else_target = required_attr(line, "else")?;
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Branch {
                id: line.stable_id(),
                scope: scope.to_string(),
                key: key.to_string(),
                op,
                value,
                then_target,
                else_target,
            });
        Ok(())
    }

    pub(super) fn push_call(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_return(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::Return {
                id: line.stable_id(),
            });
        Ok(())
    }

    pub(super) fn push_mutate(&mut self, line: &ParsedLine) -> Result<(), VnError> {
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

    pub(super) fn push_system_page(&mut self, line: &ParsedLine) -> Result<(), VnError> {
        self.current_scene_mut()?
            .commands
            .push(CompiledCommand::SystemPage {
                id: line.stable_id(),
                page: SystemPageKind::parse(line.attr("kind").unwrap_or("unknown")),
                policy: line.attr("policy").map(str::to_string),
            });
        Ok(())
    }
}
