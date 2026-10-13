use super::*;

impl VnRuntime {
    pub fn load_slot(&mut self, save: VnSaveBlob) -> Result<(), VnError> {
        if save.schema != "astra.vn.save_slot.v2" || save.state.schema != VN_RUNTIME_STATE_SCHEMA {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SAVE_SCHEMA",
                "AstraVN save slot schema is invalid",
            ));
        }
        self.state = save.state;
        Ok(())
    }

    pub(super) fn run_until_blocked(
        &mut self,
        presentation: &mut Vec<PresentationCommand>,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        self.run_until_blocked_with_dialogue_presentation(presentation, reached, true)
    }

    pub(super) fn run_until_blocked_with_dialogue_presentation(
        &mut self,
        presentation: &mut Vec<PresentationCommand>,
        reached: &mut BTreeSet<String>,
        emit_dialogue: bool,
    ) -> Result<(), VnError> {
        loop {
            if self.state.pending_wait.is_some() {
                return Ok(());
            }
            let Some(cursor) = self.state.cursor.clone() else {
                return Ok(());
            };
            let state_id = cursor.state_id.clone();
            let Some(command) = self.command_at_cursor(&cursor).cloned() else {
                self.state.cursor = None;
                return Ok(());
            };
            match command {
                CompiledCommand::Dialogue {
                    id,
                    key,
                    speaker,
                    voice,
                    window,
                } => {
                    if self.should_skip_dialogue(&id) {
                        self.advance_cursor()?;
                        continue;
                    }
                    let story_id = cursor.story_id.clone();
                    let route_position = cursor.ordinal;
                    self.advance_cursor()?;
                    self.state.backlog.push(BacklogEntry {
                        command_id: id.clone(),
                        key: key.clone(),
                        speaker: speaker.clone(),
                        voice: voice.clone(),
                        story_id,
                        state_id: state_id.clone(),
                        route_position,
                        read: true,
                        layout: BacklogLayoutMetadata {
                            window: window.clone(),
                        },
                    });
                    self.state.read_state.insert(id.clone());
                    if let Some(voice_id) = &voice {
                        self.state.voice_replay.insert(
                            voice_id.clone(),
                            VoiceReplayEntry {
                                voice: voice_id.clone(),
                                line_key: key.clone(),
                                speaker: speaker.clone(),
                            },
                        );
                    }
                    if emit_dialogue {
                        presentation.push(PresentationCommand::Dialogue {
                            key,
                            speaker,
                            voice,
                            window,
                        });
                    }
                    self.set_pending_wait(VnWaitState::new(
                        VnWaitKind::Dialogue,
                        format!("dialogue:{id}"),
                        id,
                    ))?;
                    return Ok(());
                }
                CompiledCommand::Choice { id, key, options } => {
                    self.advance_cursor()?;
                    let enabled_option_ids = options
                        .iter()
                        .map(|option| {
                            self.choice_option_enabled(option)
                                .map(|enabled| (option.id.clone(), enabled))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .filter_map(|(id, enabled)| enabled.then_some(id))
                        .collect();
                    self.state.pending_choice = Some(PendingChoice {
                        choice_id: id.clone(),
                        key: key.clone(),
                        options: options.clone(),
                        enabled_option_ids,
                    });
                    presentation.push(PresentationCommand::Choice { key, options });
                    self.set_pending_wait(VnWaitState::new(
                        VnWaitKind::Choice,
                        format!("choice:{id}"),
                        id,
                    ))?;
                    return Ok(());
                }
                CompiledCommand::Jump { id, target } => {
                    self.advance_cursor()?;
                    let target = self.resolve_runtime_target(&target);
                    self.record_route_flag(VnRouteFlagKind::Jump, &id, &target);
                    self.reach(&target, reached);
                    if self.compiled.states.contains_key(&target) {
                        let story_id = self.story_for_state(&target)?;
                        self.state.cursor = Some(self.cursor_for(&story_id, &target, 0)?);
                    } else {
                        self.state.cursor = None;
                        return Ok(());
                    }
                }
                CompiledCommand::Branch {
                    id,
                    scope,
                    key,
                    op,
                    value,
                    then_target,
                    else_target,
                } => {
                    self.advance_cursor()?;
                    let actual = self
                        .state
                        .variables
                        .get(&scope)
                        .and_then(|variables| variables.get(&key))
                        .copied()
                        .ok_or_else(|| {
                            VnError::diagnostic(
                                "ASTRA_VN_BRANCH_VARIABLE_MISSING",
                                format!(
                                    "branch command {id} requires initialized variable {scope}.{key}"
                                ),
                            )
                        })?;
                    let condition = match op {
                        BranchOp::Eq => actual == value,
                        BranchOp::NotEq => actual != value,
                        BranchOp::Less => actual < value,
                        BranchOp::LessEq => actual <= value,
                        BranchOp::Greater => actual > value,
                        BranchOp::GreaterEq => actual >= value,
                    };
                    let target = if condition { then_target } else { else_target };
                    let target = self.resolve_runtime_target(&target);
                    self.record_route_flag(VnRouteFlagKind::Branch, &id, &target);
                    self.reach(&target, reached);
                    if self.compiled.states.contains_key(&target) {
                        let story_id = self.story_for_state(&target)?;
                        self.state.cursor = Some(self.cursor_for(&story_id, &target, 0)?);
                    } else {
                        self.state.cursor = None;
                        return Ok(());
                    }
                }
                CompiledCommand::Call { id, target } => {
                    self.advance_cursor()?;
                    let return_to = self.state.cursor.clone().ok_or_else(|| {
                        VnError::diagnostic(
                            "ASTRA_VN_CALL_CONTEXT",
                            "call command requires a return cursor",
                        )
                    })?;
                    let target = self.resolve_runtime_target(&target);
                    if !self.compiled.states.contains_key(&target) {
                        return Err(VnError::diagnostic(
                            "ASTRA_VN_CALL_TARGET",
                            format!("call target {target} is not a compiled state"),
                        ));
                    }
                    self.record_route_flag(VnRouteFlagKind::Call, &id, &target);
                    self.state.call_stack.push(VnCallFrame {
                        return_to,
                        source_command_id: id.clone(),
                        reason: id,
                    });
                    self.reach(&target, reached);
                    let story_id = self.story_for_state(&target)?;
                    self.state.cursor = Some(self.cursor_for(&story_id, &target, 0)?);
                }
                CompiledCommand::Return { id } => {
                    self.advance_cursor()?;
                    let frame = self.state.call_stack.pop().ok_or_else(|| {
                        VnError::diagnostic(
                            "ASTRA_VN_RETURN_STACK",
                            format!("return command {id} has no call frame"),
                        )
                    })?;
                    self.record_route_flag(VnRouteFlagKind::Return, &id, &frame.return_to.state_id);
                    self.state.cursor = Some(frame.return_to);
                }
                CompiledCommand::Mutate {
                    scope,
                    key,
                    op,
                    value,
                    ..
                } => {
                    self.advance_cursor()?;
                    self.mutate_variable(scope, key, |current| match op {
                        MutationOp::Set => Some(value),
                        MutationOp::Add => current.checked_add(value),
                        MutationOp::Sub => current.checked_sub(value),
                    })?;
                }
                CompiledCommand::SystemPage { id, page, .. } => {
                    let entry = self
                        .compiled
                        .system_story_manifest
                        .entries
                        .get(&page)
                        .cloned();
                    if entry
                        .as_ref()
                        .is_some_and(|entry| entry.state_id != state_id)
                    {
                        self.advance_cursor()?;
                        self.open_system_story(page, presentation, reached)?;
                    } else {
                        self.advance_cursor()?;
                        presentation.push(PresentationCommand::SystemPage { page });
                        self.set_pending_wait(VnWaitState::new(
                            VnWaitKind::SystemPage,
                            format!("system:{id}"),
                            id,
                        ))?;
                    }
                    return Ok(());
                }
                CompiledCommand::Presentation { id, command } => {
                    self.advance_cursor()?;
                    let wait = wait_state_from_presentation(&id, &command)?;
                    if let PresentationCommand::Stage(StageCommand::SetSkipAllowed { allowed }) =
                        &command
                    {
                        self.state.system.skip_allowed = *allowed;
                    }
                    presentation.push(command);
                    if let Some(wait) = wait {
                        self.set_pending_wait(wait)?;
                        return Ok(());
                    }
                }
                CompiledCommand::Wait { id, fence } => {
                    self.advance_cursor()?;
                    self.set_pending_wait(VnWaitState::new(VnWaitKind::Fence, fence, id))?;
                    return Ok(());
                }
                CompiledCommand::InputWait { id } => {
                    self.advance_cursor()?;
                    self.set_pending_wait(VnWaitState::new(
                        VnWaitKind::Input,
                        format!("input:{id}"),
                        id,
                    ))?;
                    return Ok(());
                }
            }
        }
    }

    pub(super) fn set_pending_wait(&mut self, mut wait: VnWaitState) -> Result<(), VnError> {
        self.state.wait_sequence = self.state.wait_sequence.checked_add(1).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_WAIT_SEQUENCE_OVERFLOW",
                "VN wait occurrence sequence exhausted its deterministic range",
            )
        })?;
        wait.await_id = Some(format!("wait.{:016x}", self.state.wait_sequence));
        self.state.pending_wait = Some(wait);
        Ok(())
    }

    pub(super) fn choose(
        &mut self,
        option_id: &str,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        let pending = self.state.pending_choice.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_CHOICE_MISSING",
                "choice input was supplied without a pending choice",
            )
        })?;
        if self.state.pending_wait.as_ref().map(|wait| wait.kind) != Some(VnWaitKind::Choice) {
            return Err(VnError::diagnostic(
                "ASTRA_VN_CHOICE_WAIT_MISSING",
                "choice input requires a matching choice wait",
            ));
        }
        let option: ChoiceOption = pending
            .options
            .into_iter()
            .find(|option| option.id == option_id || option.key == option_id)
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_CHOICE_OPTION",
                    format!("choice option {option_id} is not available"),
                )
            })?;
        if !pending.enabled_option_ids.contains(&option.id) {
            return Err(VnError::diagnostic(
                "ASTRA_VN_CHOICE_OPTION_DISABLED",
                format!("choice option {} is disabled", option.id),
            ));
        }
        let target = self.resolve_runtime_target(&option.target);
        self.state.pending_choice = None;
        self.state.pending_wait = None;
        self.record_route_flag(
            VnRouteFlagKind::Choice,
            format!("{}:{}", pending.choice_id, option.id),
            &target,
        );
        self.reach(&target, reached);
        if self.compiled.states.contains_key(&target) {
            let story_id = self.story_for_state(&target)?;
            self.state.cursor = Some(self.cursor_for(&story_id, &target, 0)?);
        }
        Ok(())
    }

    pub(super) fn command_at_cursor(&self, cursor: &VnCommandCursor) -> Option<&CompiledCommand> {
        let state = self.compiled.states.get(&cursor.state_id)?;
        let (scene_index, command_index) = *self
            .index
            .commands_by_state
            .get(&cursor.state_id)?
            .get(cursor.ordinal)?;
        state.scenes.get(scene_index)?.commands.get(command_index)
    }

    pub(super) fn choice_option_enabled(&self, option: &ChoiceOption) -> Result<bool, VnError> {
        let Some(condition) = &option.enabled_when else {
            return Ok(true);
        };
        let actual = self
            .state
            .variables
            .get(&condition.scope)
            .and_then(|variables| variables.get(&condition.key))
            .copied()
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_CHOICE_VARIABLE_MISSING",
                    format!(
                        "choice option {} requires initialized variable {}.{}",
                        option.id, condition.scope, condition.key
                    ),
                )
            })?;
        Ok(match condition.op {
            BranchOp::Eq => actual == condition.value,
            BranchOp::NotEq => actual != condition.value,
            BranchOp::Less => actual < condition.value,
            BranchOp::LessEq => actual <= condition.value,
            BranchOp::Greater => actual > condition.value,
            BranchOp::GreaterEq => actual >= condition.value,
        })
    }

    pub(super) fn jump_to_state(
        &mut self,
        state_id: &str,
        source: &str,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        let story_id = self.story_for_state(state_id)?;
        self.state.cursor = Some(self.cursor_for(&story_id, state_id, 0)?);
        self.state.call_stack.clear();
        self.state.system_stack.clear();
        self.state.pending_choice = None;
        self.state.pending_wait = None;
        self.record_route_flag(VnRouteFlagKind::Jump, source, state_id);
        self.reach(state_id, reached);
        Ok(())
    }

    pub(super) fn command_location(
        &self,
        command_id: &str,
    ) -> Result<(String, String, usize), VnError> {
        let entry = self
            .compiled
            .command_manifest
            .commands
            .iter()
            .find(|entry| entry.id == command_id)
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_BACKLOG_COMMAND_MISSING",
                    format!("backlog command {command_id} is not compiled"),
                )
            })?;
        let state = self.compiled.states.get(&entry.state_id).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_BACKLOG_STATE_MISSING",
                format!("backlog command {command_id} references a missing state"),
            )
        })?;
        let ordinal = state
            .scenes
            .iter()
            .flat_map(|scene| &scene.commands)
            .position(|command| compiled_command_id(command) == command_id)
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_BACKLOG_COMMAND_LOCATION",
                    format!("backlog command {command_id} is absent from its compiled state"),
                )
            })?;
        Ok((entry.story_id.clone(), entry.state_id.clone(), ordinal))
    }
}
