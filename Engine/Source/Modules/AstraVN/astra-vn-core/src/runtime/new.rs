use super::*;

impl VnRuntime {
    pub fn new(compiled: impl Into<CompiledStory>, config: VnRunConfig) -> Result<Self, VnError> {
        Self::new_shared(Arc::new(compiled.into()), config)
    }

    pub fn new_shared(compiled: Arc<CompiledStory>, config: VnRunConfig) -> Result<Self, VnError> {
        let index = Arc::new(VnRuntimeIndex::build(&compiled)?);
        Self::new_shared_indexed(compiled, index, config)
    }

    pub fn new_shared_indexed(
        compiled: Arc<CompiledStory>,
        index: Arc<VnRuntimeIndex>,
        config: VnRunConfig,
    ) -> Result<Self, VnError> {
        index.validate_story(&compiled)?;
        tracing::info!(
            event = "vn.runtime.create",
            profile = %config.profile,
            locale = %config.locale,
            state_count = compiled.states.len(),
            "AstraVN runtime created"
        );
        Ok(Self {
            compiled,
            index,
            state: VnRuntimeState {
                schema: VN_RUNTIME_STATE_SCHEMA.to_string(),
                revision: 0,
                instance_id: "vn.default".to_string(),
                profile: config.profile,
                locale: config.locale,
                cursor: None,
                call_stack: Vec::new(),
                system_stack: Vec::new(),
                system: Default::default(),
                pending_choice: None,
                variables: Default::default(),
                backlog: Vec::new(),
                read_state: Default::default(),
                voice_replay: Default::default(),
                route_coverage: Default::default(),
                route_flags: Default::default(),
                wait_sequence: 0,
                pending_wait: None,
            },
            mutation_journal: BTreeMap::new(),
        })
    }

    pub fn from_state(
        compiled: impl Into<CompiledStory>,
        state: VnRuntimeState,
    ) -> Result<Self, VnError> {
        Self::from_shared_state(Arc::new(compiled.into()), state)
    }

    pub fn from_shared_state(
        compiled: Arc<CompiledStory>,
        state: VnRuntimeState,
    ) -> Result<Self, VnError> {
        let index = Arc::new(VnRuntimeIndex::build(&compiled)?);
        Self::from_shared_state_indexed(compiled, index, state)
    }

    pub fn from_shared_state_indexed(
        compiled: Arc<CompiledStory>,
        index: Arc<VnRuntimeIndex>,
        state: VnRuntimeState,
    ) -> Result<Self, VnError> {
        if state.schema != VN_RUNTIME_STATE_SCHEMA {
            return Err(VnError::diagnostic(
                "ASTRA_VN_RUNTIME_STATE_SCHEMA",
                "VN runtime state schema is invalid",
            ));
        }
        index.validate_story(&compiled)?;
        Ok(Self {
            compiled,
            index,
            state,
            mutation_journal: BTreeMap::new(),
        })
    }

    pub fn state(&self) -> &VnRuntimeState {
        &self.state
    }

    pub fn default_launch_command(&self) -> Option<VnPlayerCommand> {
        let story = self
            .compiled
            .stories
            .iter()
            .find(|story| story.id == "story.main")
            .or_else(|| self.compiled.stories.first())?;
        let state_id = story
            .states
            .iter()
            .find(|state| state.as_str() == "state.prologue")
            .or_else(|| story.states.first())?
            .clone();
        Some(VnPlayerCommand::Launch {
            story_id: story.id.clone(),
            state_id,
        })
    }

    pub fn state_revision(&self) -> u64 {
        self.state.revision
    }

    pub fn replay_ui_state(&self) -> VnReplayUiState {
        let read_count = self.state.backlog.iter().filter(|entry| entry.read).count();
        VnReplayUiState {
            schema: "astra.vn.replay_ui_state.v1".to_string(),
            backlog: self.state.backlog.clone(),
            voice_replay: self.state.voice_replay.values().cloned().collect(),
            read_count,
            unread_count: self.state.backlog.len().saturating_sub(read_count),
        }
    }

    pub fn apply(&mut self, command: VnPlayerCommand) -> Result<VnStepOutput, VnError> {
        let pending = self.apply_deferred(command)?;
        Ok(pending.finalize(self.state.revision))
    }

    pub(super) fn apply_pending(
        &mut self,
        command: VnPlayerCommand,
        before: u64,
        writes: &[(String, String, i64)],
    ) -> Result<PendingVnStepOutput, VnError> {
        tracing::trace!(
            event = "vn.runtime.command.start",
            before_state_revision = before,
            "AstraVN player command started"
        );
        self.mutation_journal.clear();
        for (scope, key, value) in writes {
            validate_system_value("variable scope", scope, 256)?;
            validate_system_value("variable key", key, 256)?;
            self.mutate_variable(scope.clone(), key.clone(), |_| Some(*value))?;
        }
        let mut presentation = Vec::new();
        let mut reached = BTreeSet::new();
        match command {
            VnPlayerCommand::Launch { story_id, state_id } => {
                self.state.cursor = Some(self.cursor_for(&story_id, &state_id, 0)?);
                self.state.call_stack.clear();
                self.state.system_stack.clear();
                self.state.pending_choice = None;
                self.state.pending_wait = None;
                self.state.route_coverage.insert(state_id.clone());
                self.record_route_flag(VnRouteFlagKind::Launch, "launch", &state_id);
                reached.insert(state_id);
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::Advance => {
                if self.state.system.reading_mode == crate::ReadingMode::Hidden {
                    self.state.system.reading_mode = crate::ReadingMode::Manual;
                } else {
                    match self.state.pending_wait.as_ref().map(|wait| wait.kind) {
                        Some(VnWaitKind::Dialogue | VnWaitKind::Input) => {
                            self.state.pending_wait = None;
                            self.run_until_blocked(&mut presentation, &mut reached)?;
                        }
                        None => self.run_until_blocked(&mut presentation, &mut reached)?,
                        Some(_) => {}
                    }
                }
            }
            VnPlayerCommand::Choose { option_id } => {
                self.choose(&option_id, &mut reached)?;
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::OpenSystem { page } => {
                self.open_system_story(page, &mut presentation, &mut reached)?;
            }
            VnPlayerCommand::SwitchSystemPage { page } => {
                self.switch_system_story(page, &mut presentation, &mut reached)?;
            }
            VnPlayerCommand::ReturnSystem => {
                let frame = self.state.system_stack.pop().ok_or_else(|| {
                    VnError::diagnostic(
                        "ASTRA_VN_SYSTEM_STACK",
                        "system.return was supplied without an open system story",
                    )
                })?;
                self.state.cursor = Some(frame.return_to);
                self.state.pending_wait = frame.return_wait;
                self.state.pending_choice = frame.return_choice;
                if self.state.pending_wait.is_none() && self.state.pending_choice.is_none() {
                    self.run_until_blocked(&mut presentation, &mut reached)?;
                }
            }
            VnPlayerCommand::ReplayVoice { voice } => {
                if !self.state.voice_replay.contains_key(&voice) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_VOICE_REPLAY_MISSING",
                        format!("voice replay entry {voice} is not available"),
                    ));
                }
                presentation.push(PresentationCommand::Stage(StageCommand::Audio(AudioCue {
                    id: format!("voice.replay.{voice}"),
                    bus: VnAudioBus::Voice,
                    asset: voice,
                    looped: false,
                    fade_ms: 0,
                    sync: VnAudioSync::None,
                })));
            }
            VnPlayerCommand::SetAuto { enabled } => {
                self.state.system.auto_enabled = enabled;
            }
            VnPlayerCommand::SetSkip { mode } => {
                self.state.system.skip_mode = mode;
            }
            VnPlayerCommand::SetReadingMode { mode } => {
                self.state.system.reading_mode = mode;
                if mode == crate::ReadingMode::FastForward {
                    self.fast_forward_until_blocked(&mut presentation, &mut reached)?;
                }
            }
            VnPlayerCommand::SetAudioEnabled { enabled } => {
                self.state.system.audio_enabled = enabled;
                for bus in [VnAudioBus::Bgm, VnAudioBus::Se] {
                    presentation.push(PresentationCommand::Stage(
                        StageCommand::SetAudioBusEnabled { bus, enabled },
                    ));
                }
            }
            VnPlayerCommand::InvokeSystemAction { action_id } => {
                let action_checkpoint = self.state.clone();
                let action_result = (|| -> Result<(), VnError> {
                    validate_system_value("system action id", &action_id, 128)?;
                    let program = self
                        .compiled
                        .system_story_manifest
                        .actions
                        .get(&action_id)
                        .cloned()
                        .ok_or_else(|| {
                            VnError::diagnostic(
                                "ASTRA_VN_SYSTEM_ACTION_UNDECLARED",
                                format!(
                                    "system action {action_id} is not declared by the compiled story"
                                ),
                            )
                        })?;
                    for effect in program.effects {
                        match effect {
                            crate::SystemActionEffect::Mutate {
                                scope,
                                key,
                                op,
                                value,
                            } => {
                                self.mutate_variable(scope, key, |current| match op {
                                    MutationOp::Set => Some(value),
                                    MutationOp::Add => current.checked_add(value),
                                    MutationOp::Sub => current.checked_sub(value),
                                })?;
                            }
                            crate::SystemActionEffect::Jump { target } => {
                                let target = self.resolve_runtime_target(&target);
                                if !self.compiled.states.contains_key(&target) {
                                    return Err(VnError::diagnostic(
                                        "ASTRA_VN_SYSTEM_ACTION_TARGET",
                                        "system action jump target is not compiled",
                                    ));
                                }
                                self.state.system_stack.clear();
                                self.state.pending_wait = None;
                                self.state.pending_choice = None;
                                let story_id = self.story_for_state(&target)?;
                                self.state.cursor = Some(self.cursor_for(&story_id, &target, 0)?);
                                self.record_route_flag(VnRouteFlagKind::Jump, &action_id, &target);
                                self.reach(&target, &mut reached);
                                self.run_until_blocked(&mut presentation, &mut reached)?;
                            }
                            crate::SystemActionEffect::SwitchSystemPage { page } => {
                                self.switch_system_story(page, &mut presentation, &mut reached)?;
                            }
                            crate::SystemActionEffect::ReturnSystem => {
                                let frame = self.state.system_stack.pop().ok_or_else(|| {
                                    VnError::diagnostic(
                                        "ASTRA_VN_SYSTEM_STACK",
                                        "system action return requires an open system story",
                                    )
                                })?;
                                self.state.cursor = Some(frame.return_to);
                                self.state.pending_wait = frame.return_wait;
                                self.state.pending_choice = frame.return_choice;
                                if self.state.pending_wait.is_none()
                                    && self.state.pending_choice.is_none()
                                {
                                    self.run_until_blocked(&mut presentation, &mut reached)?;
                                }
                            }
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = action_result {
                    self.state = action_checkpoint;
                    return Err(error);
                }
            }
            VnPlayerCommand::SetConfig { key, value } => {
                validate_system_value("config key", &key, 256)?;
                validate_system_value("config value", &value, 16_384)?;
                if key == "display.language" {
                    if value.is_empty()
                        || value.len() > 64
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    {
                        return Err(VnError::diagnostic(
                            "ASTRA_VN_LOCALE_IDENTITY",
                            "display.language must be a safe locale identifier",
                        ));
                    }
                    self.state.locale = value.clone();
                }
                self.state.system.config.insert(key, value);
            }
            VnPlayerCommand::StartReplay { replay_id } => {
                if !self.state.system.replay_unlocks.contains(&replay_id) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_REPLAY_LOCKED",
                        format!("replay {replay_id} is not unlocked"),
                    ));
                }
                self.jump_to_state(&replay_id, "replay", &mut reached)?;
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::PreviewGallery { item_id } => {
                if !self.state.system.gallery_unlocks.contains(&item_id) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_GALLERY_LOCKED",
                        format!("gallery item {item_id} is not unlocked"),
                    ));
                }
                presentation.push(PresentationCommand::Marker {
                    id: format!("gallery.preview.{item_id}"),
                });
            }
            VnPlayerCommand::JumpRoute { node_id } => {
                if !self
                    .compiled
                    .route_graph
                    .nodes
                    .iter()
                    .any(|node| node.id == node_id)
                    || !self.state.route_coverage.contains(&node_id)
                {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_ROUTE_JUMP_DENIED",
                        format!("route node {node_id} is not a reached route destination"),
                    ));
                }
                self.jump_to_state(&node_id, "route_chart", &mut reached)?;
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::JumpBacklog { command_id } => {
                if !self
                    .state
                    .backlog
                    .iter()
                    .any(|entry| entry.command_id == command_id && entry.read)
                {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_BACKLOG_JUMP_DENIED",
                        format!("backlog command {command_id} is absent or unread"),
                    ));
                }
                let (story_id, state_id, ordinal) = self.command_location(&command_id)?;
                self.state.cursor = Some(self.cursor_for(&story_id, &state_id, ordinal)?);
                self.state.call_stack.clear();
                self.state.system_stack.clear();
                self.state.pending_choice = None;
                self.state.pending_wait = None;
                self.record_route_flag(VnRouteFlagKind::Jump, "backlog", &state_id);
                self.reach(&state_id, &mut reached);
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::SubmitText { input_id, value } => {
                validate_system_value("text input id", &input_id, 256)?;
                validate_system_value("text input value", &value, 16_384)?;
                self.state
                    .system
                    .config
                    .insert(format!("text_input.{input_id}"), value);
            }
            VnPlayerCommand::Unlock { kind, id } => match kind {
                SystemUnlockKind::Gallery => {
                    self.state.system.gallery_unlocks.insert(id);
                }
                SystemUnlockKind::Replay => {
                    self.state.system.replay_unlocks.insert(id);
                }
            },
            VnPlayerCommand::CompleteWait { fence } => {
                let pending = self.state.pending_wait.clone().ok_or_else(|| {
                    VnError::diagnostic(
                        "ASTRA_VN_WAIT_MISSING",
                        "await completion was supplied without a pending wait state",
                    )
                })?;
                if pending.fence != fence {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_WAIT_FENCE",
                        format!(
                            "await completion fence {fence} does not match pending fence {}",
                            pending.fence
                        ),
                    ));
                }
                self.state.pending_wait = None;
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
        }
        if self.state.system.reading_mode == crate::ReadingMode::FastForward
            && self.state.system.skip_allowed
            && self.state.system_stack.is_empty()
            && matches!(
                self.state.pending_wait.as_ref().map(|wait| wait.kind),
                Some(VnWaitKind::Dialogue | VnWaitKind::Input)
            )
        {
            self.fast_forward_until_blocked(&mut presentation, &mut reached)?;
        }
        let events = reached
            .iter()
            .cloned()
            .map(|id| crate::VnEvent {
                kind: "vn.route.reached".to_string(),
                id,
            })
            .collect();
        let audio = presentation.iter().filter_map(vn_audio_command).collect();
        let timeline_tasks = presentation.iter().filter_map(vn_timeline_task).collect();
        let mutations = self.variable_mutations();
        Ok(PendingVnStepOutput(VnStepOutput {
            schema: "astra.vn.step_output.v1".to_string(),
            next_cursor: self.state.cursor.clone(),
            wait: self.state.pending_wait.clone(),
            awaits: Vec::new(),
            events,
            presentation,
            audio,
            timeline_tasks,
            mutations,
            coverage: VnCoverage { reached },
            state_revision_before_advance: before,
            state_revision_after_advance: before,
        }))
    }

    pub fn save_slot(&self, slot: impl Into<String>) -> Result<VnSaveBlob, VnError> {
        Ok(VnSaveBlob {
            schema: "astra.vn.save_slot.v2".to_string(),
            slot: slot.into(),
            state: self.state.clone(),
        })
    }
}
