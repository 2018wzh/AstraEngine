use super::*;

impl VnRuntime {
    pub(super) fn cursor_for(
        &self,
        story_id: &str,
        state_id: &str,
        ordinal: usize,
    ) -> Result<VnCommandCursor, VnError> {
        let state = self.compiled.states.get(state_id).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_CURSOR_STATE",
                format!("cursor state {state_id} is not compiled"),
            )
        })?;
        let command = self
            .index
            .commands_by_state
            .get(state_id)
            .and_then(|commands| commands.get(ordinal))
            .and_then(|(scene_index, command_index)| {
                state.scenes.get(*scene_index).and_then(|scene| {
                    scene
                        .commands
                        .get(*command_index)
                        .map(|command| (scene.id.as_str(), command))
                })
            });
        let (scene_id, command_id) = command
            .map(|(scene_id, command)| (scene_id.to_string(), compiled_command_id(command)))
            .unwrap_or_else(|| {
                (
                    state
                        .scenes
                        .last()
                        .map(|scene| scene.id.clone())
                        .unwrap_or_else(|| "astra.vn.scene.none".to_string()),
                    "astra.vn.cursor.end".to_string(),
                )
            });
        Ok(VnCommandCursor {
            story_id: story_id.to_string(),
            state_id: state_id.to_string(),
            scene_id,
            command_id,
            ordinal,
        })
    }

    pub(super) fn advance_cursor(&mut self) -> Result<(), VnError> {
        let cursor = self.state.cursor.clone().ok_or_else(|| {
            VnError::diagnostic("ASTRA_VN_CURSOR_MISSING", "VN command cursor is not set")
        })?;
        self.state.cursor =
            Some(self.cursor_for(&cursor.story_id, &cursor.state_id, cursor.ordinal + 1)?);
        Ok(())
    }

    pub(super) fn story_for_state(&self, state_id: &str) -> Result<String, VnError> {
        self.index
            .story_by_state
            .get(state_id)
            .cloned()
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_CURSOR_STORY",
                    format!("state {state_id} has no owning story"),
                )
            })
    }

    pub(super) fn open_system_story(
        &mut self,
        page: crate::SystemPageKind,
        presentation: &mut Vec<PresentationCommand>,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        let entry = self
            .compiled
            .system_story_manifest
            .entries
            .get(&page)
            .cloned()
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_SYSTEM_ENTRY_MISSING",
                    format!("system page {page:?} has no compiled story entry"),
                )
            })?;
        let return_to = self.state.cursor.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_SYSTEM_RETURN_CURSOR",
                "system story requires a return cursor",
            )
        })?;
        self.state.system_stack.push(VnSystemFrame {
            return_to,
            return_wait: self.state.pending_wait.take(),
            return_choice: self.state.pending_choice.take(),
            page,
        });
        self.state.cursor = Some(self.cursor_for(&entry.story_id, &entry.state_id, 0)?);
        self.reach(&entry.state_id, reached);
        self.run_until_blocked(presentation, reached)
    }

    pub(super) fn switch_system_story(
        &mut self,
        page: crate::SystemPageKind,
        presentation: &mut Vec<PresentationCommand>,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        let entry = self
            .compiled
            .system_story_manifest
            .entries
            .get(&page)
            .cloned()
            .ok_or_else(|| {
                VnError::diagnostic(
                    "ASTRA_VN_SYSTEM_ENTRY_MISSING",
                    format!("system page {page:?} has no compiled story entry"),
                )
            })?;
        let frame = self.state.system_stack.last_mut().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_SYSTEM_STACK",
                "system page switch requires an open system story",
            )
        })?;
        frame.page = page;
        self.state.pending_wait = None;
        self.state.pending_choice = None;
        self.state.cursor = Some(self.cursor_for(&entry.story_id, &entry.state_id, 0)?);
        self.reach(&entry.state_id, reached);
        self.run_until_blocked(presentation, reached)
    }

    pub(super) fn fast_forward_until_blocked(
        &mut self,
        presentation: &mut Vec<PresentationCommand>,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        const MAX_FAST_FORWARD_WAITS: usize = 100_000;
        for _ in 0..MAX_FAST_FORWARD_WAITS {
            if !self.state.system.skip_allowed {
                return Ok(());
            }
            match self.state.pending_wait.as_ref().map(|wait| wait.kind) {
                Some(VnWaitKind::Dialogue | VnWaitKind::Input) => {
                    self.state.pending_wait = None;
                    // Fast-forward still commits dialogue to backlog/read-state, but only
                    // non-dialogue presentation reaches the renderer. Suppressing the
                    // transient dialogue command at its source avoids a drain/filter/
                    // reallocate cycle for every skipped wait while preserving authored
                    // stage, audio, timeline, and route effects in their original order.
                    self.run_until_blocked_with_dialogue_presentation(
                        presentation,
                        reached,
                        false,
                    )?;
                }
                None => {
                    self.run_until_blocked(presentation, reached)?;
                    if self.state.pending_wait.is_none() && self.state.pending_choice.is_none() {
                        return Ok(());
                    }
                }
                Some(_) => return Ok(()),
            }
            if self.state.pending_choice.is_some() || !self.state.system_stack.is_empty() {
                return Ok(());
            }
        }
        Err(VnError::diagnostic(
            "ASTRA_VN_READING_MODE_BUDGET",
            "fast-forward exceeded the bounded dialogue/input wait budget",
        ))
    }

    pub(super) fn resolve_runtime_target(&self, target: &str) -> String {
        resolve_target(target, &self.index.state_ids)
    }

    pub(super) fn reach(&mut self, target: &str, reached: &mut BTreeSet<String>) {
        self.state.route_coverage.insert(target.to_string());
        reached.insert(target.to_string());
    }

    pub(super) fn record_route_flag(
        &mut self,
        kind: VnRouteFlagKind,
        source: impl Into<String>,
        target: &str,
    ) {
        let source = source.into();
        let key = format!("{}:{source}:{target}", route_flag_kind_id(kind));
        self.state
            .route_flags
            .entry(key)
            .and_modify(|flag| flag.count = flag.count.saturating_add(1))
            .or_insert_with(|| VnRouteFlag::new(kind, source, target.to_string()));
    }

    pub(super) fn should_skip_dialogue(&self, command_id: &str) -> bool {
        if !self.state.system.skip_allowed {
            return false;
        }
        match self.state.system.skip_mode {
            SkipMode::None => false,
            SkipMode::Read => self.state.read_state.contains(command_id),
            SkipMode::All => true,
        }
    }

    pub(super) fn mutate_variable(
        &mut self,
        scope: String,
        key: String,
        update: impl FnOnce(i64) -> Option<i64>,
    ) -> Result<(), VnError> {
        let before = self
            .state
            .variables
            .get(&scope)
            .and_then(|values| values.get(&key))
            .copied();
        self.mutation_journal
            .entry((scope.clone(), key.clone()))
            .or_insert(before);
        let after = update(before.unwrap_or_default()).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_MUTATION_OVERFLOW",
                "variable mutation overflowed i64",
            )
        })?;
        self.state
            .variables
            .entry(scope)
            .or_default()
            .insert(key, after);
        Ok(())
    }

    pub(super) fn variable_mutations(&self) -> Vec<crate::VnMutationRecord> {
        self.mutation_journal
            .iter()
            .filter_map(|((scope, key), before)| {
                let after = self
                    .state
                    .variables
                    .get(scope)
                    .and_then(|values| values.get(key))
                    .copied();
                (*before != after).then(|| crate::VnMutationRecord {
                    scope: scope.clone(),
                    key: key.clone(),
                    before: *before,
                    after,
                })
            })
            .collect()
    }
}

pub struct PendingVnStepOutput(pub(super) VnStepOutput);

impl PendingVnStepOutput {
    /// Remove the static policy action request after the session accepts ownership.
    pub fn take_policy_action(&mut self) -> Option<crate::ExtensionPresentationCommand> {
        let index = self.0.presentation.iter().position(|p| {
            matches!(p,
            PresentationCommand::Extension(e) if e.provider_id == crate::POLICY_ACTION_PROVIDER)
        })?;
        match self.0.presentation.remove(index) {
            PresentationCommand::Extension(command) => Some(command),
            _ => unreachable!(),
        }
    }

    pub fn events(&self) -> &[crate::VnEvent] {
        &self.0.events
    }

    pub fn set_wait(&mut self, wait: VnWaitState) {
        self.0.wait = Some(wait);
    }

    pub fn push_await(&mut self, await_id: String) {
        self.0.awaits.push(await_id);
    }

    pub fn finalize(mut self, state_revision_after_advance: u64) -> VnStepOutput {
        self.0.state_revision_after_advance = state_revision_after_advance;
        self.0
    }
}

pub fn reduce_vn_step(
    compiled: Arc<CompiledStory>,
    state: &VnRuntimeState,
    command: VnPlayerCommand,
) -> Result<(VnRuntimeState, VnStepOutput), VnError> {
    let index = Arc::new(VnRuntimeIndex::build(&compiled)?);
    reduce_vn_step_indexed(compiled, index, state.clone(), command)
}

pub fn reduce_vn_step_indexed(
    compiled: Arc<CompiledStory>,
    index: Arc<VnRuntimeIndex>,
    state: VnRuntimeState,
    command: VnPlayerCommand,
) -> Result<(VnRuntimeState, VnStepOutput), VnError> {
    let mut runtime = VnRuntime::from_shared_state_indexed(compiled, index, state)?;
    let output = runtime.apply(command)?;
    Ok((runtime.state, output))
}

pub(super) fn compiled_command_id(command: &CompiledCommand) -> String {
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
        | CompiledCommand::InputWait { id } => id.clone(),
    }
}

pub(super) fn vn_audio_command(command: &PresentationCommand) -> Option<crate::VnAudioCommand> {
    let PresentationCommand::Stage(StageCommand::Audio(cue)) = command else {
        return None;
    };
    Some(crate::VnAudioCommand {
        command_id: cue.id.clone(),
        cue: cue.clone(),
    })
}

pub(super) fn vn_timeline_task(command: &PresentationCommand) -> Option<crate::VnTimelineTask> {
    let PresentationCommand::Stage(StageCommand::Timeline(command)) = command else {
        return None;
    };
    let command_id = match command {
        TimelineCommand::Start(spec) => spec.id.clone(),
        TimelineCommand::Cancel { id, .. } => id.clone(),
    };
    Some(crate::VnTimelineTask {
        command_id,
        command: command.clone(),
    })
}

pub(super) fn route_flag_kind_id(kind: VnRouteFlagKind) -> &'static str {
    match kind {
        VnRouteFlagKind::Launch => "launch",
        VnRouteFlagKind::Choice => "choice",
        VnRouteFlagKind::Jump => "jump",
        VnRouteFlagKind::Branch => "branch",
        VnRouteFlagKind::Call => "call",
        VnRouteFlagKind::Return => "return",
    }
}

pub(super) fn wait_state_from_presentation(
    command_id: &str,
    command: &PresentationCommand,
) -> Result<Option<VnWaitState>, VnError> {
    if matches!(command, PresentationCommand::Extension(extension) if extension.provider_id == crate::POLICY_ACTION_PROVIDER)
    {
        return Ok(Some(VnWaitState::new(
            VnWaitKind::Fence,
            format!("policy.{command_id}"),
            command_id.to_owned(),
        )));
    }
    let PresentationCommand::Stage(stage) = command else {
        return Ok(None);
    };
    let wait = match stage {
        StageCommand::Movie {
            end: VnMovieEndBehavior::Wait,
            fence,
            ..
        } => Some(VnWaitState::new(
            VnWaitKind::MovieEnd,
            required_typed_fence(command_id, fence.as_deref())?,
            command_id.to_string(),
        )),
        StageCommand::Audio(cue)
            if cue.bus == VnAudioBus::Voice && cue.sync != VnAudioSync::None =>
        {
            let fence = match &cue.sync {
                VnAudioSync::Fence(fence) => fence.clone(),
                VnAudioSync::Text => format!("{}.end", cue.id),
                VnAudioSync::None => unreachable!(),
            };
            Some(VnWaitState::new(
                VnWaitKind::VoiceEnd,
                fence,
                command_id.to_string(),
            ))
        }
        StageCommand::Timeline(TimelineCommand::Start(spec))
            if spec.join == VnTimelineJoinPolicy::Block =>
        {
            Some(VnWaitState::new(
                VnWaitKind::TimelineComplete,
                required_typed_fence(command_id, spec.fence.as_deref())?,
                command_id.to_string(),
            ))
        }
        _ => None,
    };
    Ok(wait)
}

pub(super) fn required_typed_fence(
    command_id: &str,
    fence: Option<&str>,
) -> Result<String, VnError> {
    fence.map(str::to_string).ok_or_else(|| {
        VnError::diagnostic(
            "ASTRA_VN_TYPED_FENCE_MISSING",
            format!("typed presentation command {command_id} is missing its required fence"),
        )
    })
}

pub(super) fn validate_system_value(
    label: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), VnError> {
    if value.trim().is_empty()
        || value.len() > max_bytes
        || value.chars().any(|character| character.is_control())
    {
        return Err(VnError::diagnostic(
            "ASTRA_VN_SYSTEM_VALUE_INVALID",
            format!("{label} must be non-empty, bounded, and contain no control characters"),
        ));
    }
    Ok(())
}
