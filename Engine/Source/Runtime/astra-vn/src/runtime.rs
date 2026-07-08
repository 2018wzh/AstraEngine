use std::collections::BTreeSet;

use astra_core::Hash128;

use crate::{
    resolve_target, BacklogEntry, BacklogLayoutMetadata, ChoiceOption, CompiledCommand,
    CompiledStory, MutationOp, PendingChoice, PresentationCommand, SkipMode, SystemUnlockKind,
    VnCallFrame, VnCoverage, VnError, VnPlayerCommand, VnReplayUiState, VnRouteFlag,
    VnRouteFlagKind, VnRunConfig, VnRuntimeState, VnSaveBlob, VnStepOutput, VnWaitKind,
    VnWaitState, VoiceReplayEntry,
};

#[derive(Debug, Clone)]
pub struct VnRuntime {
    compiled: CompiledStory,
    state: VnRuntimeState,
}

impl VnRuntime {
    pub fn new(compiled: CompiledStory, config: VnRunConfig) -> Result<Self, VnError> {
        Ok(Self {
            compiled,
            state: VnRuntimeState {
                schema: "astra.vn.runtime_state.v1".to_string(),
                profile: config.profile,
                locale: config.locale,
                current_story: None,
                current_state: None,
                command_cursor: 0,
                call_stack: Vec::new(),
                system: Default::default(),
                pending_choice: None,
                variables: Default::default(),
                backlog: Vec::new(),
                read_state: Default::default(),
                voice_replay: Default::default(),
                route_coverage: Default::default(),
                route_flags: Default::default(),
                pending_wait: None,
            },
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

    pub fn state_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(&self.state)
                .expect("AstraVN runtime state must serialize for hashing"),
        )
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
        let before = self.state_hash();
        let mut presentation = Vec::new();
        let mut reached = BTreeSet::new();
        match command {
            VnPlayerCommand::Launch { story_id, state_id } => {
                self.state.current_story = Some(story_id);
                self.state.current_state = Some(state_id.clone());
                self.state.command_cursor = 0;
                self.state.call_stack.clear();
                self.state.pending_choice = None;
                self.state.pending_wait = None;
                self.state.route_coverage.insert(state_id.clone());
                self.record_route_flag(VnRouteFlagKind::Launch, "launch", &state_id);
                reached.insert(state_id);
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::Advance => {
                if self.state.pending_wait.is_none() {
                    self.run_until_blocked(&mut presentation, &mut reached)?;
                }
            }
            VnPlayerCommand::Choose { option_id } => {
                self.choose(&option_id, &mut reached)?;
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::OpenSystem { page } => {
                presentation.push(PresentationCommand::SystemPage { page });
            }
            VnPlayerCommand::ReplayVoice { voice } => {
                if !self.state.voice_replay.contains_key(&voice) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_VOICE_REPLAY_MISSING",
                        format!("voice replay entry {voice} is not available"),
                    ));
                }
                presentation.push(PresentationCommand::Stage {
                    command: "voice_replay".to_string(),
                    attributes: [("voice".to_string(), voice)].into_iter().collect(),
                });
            }
            VnPlayerCommand::SetAuto { enabled } => {
                self.state.system.auto_enabled = enabled;
            }
            VnPlayerCommand::SetSkip { mode } => {
                self.state.system.skip_mode = mode;
            }
            VnPlayerCommand::SetConfig { key, value } => {
                self.state.system.config.insert(key, value);
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
        Ok(VnStepOutput {
            schema: "astra.vn.step_output.v1".to_string(),
            presentation,
            coverage: VnCoverage { reached },
            state_hash_before_advance: before,
            state_hash_after_advance: self.state_hash(),
        })
    }

    pub fn save_slot(&self, slot: impl Into<String>) -> Result<VnSaveBlob, VnError> {
        Ok(VnSaveBlob {
            schema: "astra.vn.save_slot.v1".to_string(),
            slot: slot.into(),
            state_hash: self.state_hash(),
            state: self.state.clone(),
        })
    }

    pub fn load_slot(&mut self, save: VnSaveBlob) -> Result<(), VnError> {
        if save.schema != "astra.vn.save_slot.v1" {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SAVE_SCHEMA",
                "AstraVN save slot schema is invalid",
            ));
        }
        self.state = save.state;
        Ok(())
    }

    fn run_until_blocked(
        &mut self,
        presentation: &mut Vec<PresentationCommand>,
        reached: &mut BTreeSet<String>,
    ) -> Result<(), VnError> {
        loop {
            if self.state.pending_wait.is_some() {
                return Ok(());
            }
            let Some(state_id) = self.state.current_state.clone() else {
                return Ok(());
            };
            let Some(command) = self.command_at_cursor(&state_id).cloned() else {
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
                        self.state.command_cursor += 1;
                        continue;
                    }
                    let story_id = self.state.current_story.clone().unwrap_or_default();
                    let route_position = self.state.command_cursor;
                    self.state.command_cursor += 1;
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
                    self.state.read_state.insert(id);
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
                    presentation.push(PresentationCommand::Dialogue {
                        key,
                        speaker,
                        voice,
                        window,
                    });
                    return Ok(());
                }
                CompiledCommand::Choice { id, key, options } => {
                    self.state.command_cursor += 1;
                    self.state.pending_choice = Some(PendingChoice {
                        choice_id: id,
                        key: key.clone(),
                        options: options.clone(),
                    });
                    presentation.push(PresentationCommand::Choice { key, options });
                    return Ok(());
                }
                CompiledCommand::Jump { id, target } => {
                    self.state.command_cursor += 1;
                    let target = self.resolve_runtime_target(&target);
                    self.record_route_flag(VnRouteFlagKind::Jump, &id, &target);
                    self.reach(&target, reached);
                    if self.compiled.states.contains_key(&target) {
                        self.state.current_state = Some(target);
                        self.state.command_cursor = 0;
                    } else {
                        return Ok(());
                    }
                }
                CompiledCommand::Call { id, target } => {
                    let Some(story_id) = self.state.current_story.clone() else {
                        return Err(VnError::diagnostic(
                            "ASTRA_VN_CALL_CONTEXT",
                            "call command requires a current story",
                        ));
                    };
                    self.state.command_cursor += 1;
                    let Some(state_id) = self.state.current_state.clone() else {
                        return Err(VnError::diagnostic(
                            "ASTRA_VN_CALL_CONTEXT",
                            "call command requires a current state",
                        ));
                    };
                    let target = self.resolve_runtime_target(&target);
                    if !self.compiled.states.contains_key(&target) {
                        return Err(VnError::diagnostic(
                            "ASTRA_VN_CALL_TARGET",
                            format!("call target {target} is not a compiled state"),
                        ));
                    }
                    self.record_route_flag(VnRouteFlagKind::Call, &id, &target);
                    self.state.call_stack.push(VnCallFrame {
                        story_id,
                        state_id,
                        command_cursor: self.state.command_cursor,
                        reason: id,
                    });
                    self.reach(&target, reached);
                    self.state.current_state = Some(target);
                    self.state.command_cursor = 0;
                }
                CompiledCommand::Return { id } => {
                    self.state.command_cursor += 1;
                    let frame = self.state.call_stack.pop().ok_or_else(|| {
                        VnError::diagnostic(
                            "ASTRA_VN_RETURN_STACK",
                            format!("return command {id} has no call frame"),
                        )
                    })?;
                    self.state.current_story = Some(frame.story_id);
                    self.record_route_flag(VnRouteFlagKind::Return, &id, &frame.state_id);
                    self.state.current_state = Some(frame.state_id);
                    self.state.command_cursor = frame.command_cursor;
                }
                CompiledCommand::Mutate {
                    scope,
                    key,
                    op,
                    value,
                    ..
                } => {
                    self.state.command_cursor += 1;
                    let entry = self
                        .state
                        .variables
                        .entry(scope)
                        .or_default()
                        .entry(key)
                        .or_default();
                    match op {
                        MutationOp::Set => *entry = value,
                        MutationOp::Add => *entry += value,
                        MutationOp::Sub => *entry -= value,
                    }
                }
                CompiledCommand::SystemPage { page, .. } => {
                    self.state.command_cursor += 1;
                    presentation.push(PresentationCommand::SystemPage { page });
                    return Ok(());
                }
                CompiledCommand::Presentation { id, command } => {
                    self.state.command_cursor += 1;
                    let wait = wait_state_from_presentation(&id, &command);
                    presentation.push(command);
                    if let Some(wait) = wait {
                        self.state.pending_wait = Some(wait);
                        return Ok(());
                    }
                }
                CompiledCommand::Wait { id, fence } => {
                    self.state.command_cursor += 1;
                    self.state.pending_wait = Some(VnWaitState::new(VnWaitKind::Fence, fence, id));
                    return Ok(());
                }
            }
        }
    }

    fn choose(&mut self, option_id: &str, reached: &mut BTreeSet<String>) -> Result<(), VnError> {
        let pending = self.state.pending_choice.clone().ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_CHOICE_MISSING",
                "choice input was supplied without a pending choice",
            )
        })?;
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
        let target = self.resolve_runtime_target(&option.target);
        self.state.pending_choice = None;
        self.record_route_flag(
            VnRouteFlagKind::Choice,
            format!("{}:{}", pending.choice_id, option.id),
            &target,
        );
        self.reach(&target, reached);
        if self.compiled.states.contains_key(&target) {
            self.state.current_state = Some(target);
            self.state.command_cursor = 0;
        }
        Ok(())
    }

    fn command_at_cursor(&self, state_id: &str) -> Option<&CompiledCommand> {
        self.compiled
            .states
            .get(state_id)?
            .scenes
            .iter()
            .flat_map(|scene| &scene.commands)
            .nth(self.state.command_cursor)
    }

    fn resolve_runtime_target(&self, target: &str) -> String {
        let state_ids = self.compiled.states.keys().cloned().collect();
        resolve_target(target, &state_ids)
    }

    fn reach(&mut self, target: &str, reached: &mut BTreeSet<String>) {
        self.state.route_coverage.insert(target.to_string());
        reached.insert(target.to_string());
    }

    fn record_route_flag(
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

    fn should_skip_dialogue(&self, command_id: &str) -> bool {
        match self.state.system.skip_mode {
            SkipMode::None => false,
            SkipMode::Read => self.state.read_state.contains(command_id),
            SkipMode::All => true,
        }
    }
}

fn route_flag_kind_id(kind: VnRouteFlagKind) -> &'static str {
    match kind {
        VnRouteFlagKind::Launch => "launch",
        VnRouteFlagKind::Choice => "choice",
        VnRouteFlagKind::Jump => "jump",
        VnRouteFlagKind::Call => "call",
        VnRouteFlagKind::Return => "return",
    }
}

fn wait_state_from_presentation(
    command_id: &str,
    command: &PresentationCommand,
) -> Option<VnWaitState> {
    let PresentationCommand::Stage {
        command,
        attributes,
    } = command
    else {
        return None;
    };
    match command.as_str() {
        "movie" if attr_is(attributes, "end", "wait") || attr_is(attributes, "wait_for", "end") => {
            Some(VnWaitState::new(
                VnWaitKind::MovieEnd,
                attributes
                    .get("fence")
                    .cloned()
                    .unwrap_or_else(|| format!("{command_id}.end")),
                command_id.to_string(),
            ))
        }
        "voice"
            if attr_is(attributes, "sync", "text")
                || attr_is(attributes, "sync", "fence")
                || attr_is(attributes, "wait", "true") =>
        {
            Some(VnWaitState::new(
                VnWaitKind::VoiceEnd,
                attributes
                    .get("fence")
                    .cloned()
                    .unwrap_or_else(|| format!("{command_id}.end")),
                command_id.to_string(),
            ))
        }
        "timeline"
            if attr_is(attributes, "join", "wait") || attr_is(attributes, "join", "block") =>
        {
            Some(VnWaitState::new(
                VnWaitKind::TimelineComplete,
                attributes
                    .get("fence")
                    .cloned()
                    .unwrap_or_else(|| format!("{command_id}.complete")),
                command_id.to_string(),
            ))
        }
        _ => None,
    }
}

fn attr_is(
    attributes: &std::collections::BTreeMap<String, String>,
    key: &str,
    expected: &str,
) -> bool {
    attributes
        .get(key)
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}
