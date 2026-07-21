use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::Hash128;

use crate::{
    resolve_target, AudioCue, BacklogEntry, BacklogLayoutMetadata, BranchOp, ChoiceOption,
    CompiledCommand, CompiledStory, MutationOp, PendingChoice, PresentationCommand, SkipMode,
    StageCommand, SystemUnlockKind, TimelineCommand, VnAudioBus, VnAudioSync, VnCallFrame,
    VnCommandCursor, VnCoverage, VnError, VnMovieEndBehavior, VnPlayerCommand, VnReplayUiState,
    VnRouteFlag, VnRouteFlagKind, VnRunConfig, VnRuntimeState, VnSaveBlob, VnStepOutput,
    VnSystemFrame, VnTimelineJoinPolicy, VnWaitKind, VnWaitState, VoiceReplayEntry,
    VN_RUNTIME_STATE_SCHEMA,
};

#[derive(Debug, Clone)]
pub struct VnRuntime {
    compiled: Arc<CompiledStory>,
    index: Arc<VnRuntimeIndex>,
    state: VnRuntimeState,
}

#[derive(Debug)]
pub struct VnRuntimeIndex {
    story_hash: Hash128,
    commands_by_state: BTreeMap<String, Vec<(usize, usize)>>,
    story_by_state: BTreeMap<String, String>,
}

impl VnRuntimeIndex {
    pub fn build(compiled: &CompiledStory) -> Result<Self, VnError> {
        let mut story_by_state = BTreeMap::new();
        for story in &compiled.stories {
            for state_id in &story.states {
                if !compiled.states.contains_key(state_id) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_RUNTIME_INDEX_STATE_MISSING",
                        format!("story {} references missing state {state_id}", story.id),
                    ));
                }
                if story_by_state
                    .insert(state_id.clone(), story.id.clone())
                    .is_some()
                {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_RUNTIME_INDEX_STATE_OWNER_CONFLICT",
                        format!("state {state_id} belongs to multiple stories"),
                    ));
                }
            }
        }
        let commands_by_state = compiled
            .states
            .iter()
            .map(|(state_id, state)| {
                let commands = state
                    .scenes
                    .iter()
                    .enumerate()
                    .flat_map(|(scene_index, scene)| {
                        (0..scene.commands.len())
                            .map(move |command_index| (scene_index, command_index))
                    })
                    .collect();
                (state_id.clone(), commands)
            })
            .collect();
        Ok(Self {
            story_hash: compiled.story_hash,
            commands_by_state,
            story_by_state,
        })
    }

    fn validate_story(&self, compiled: &CompiledStory) -> Result<(), VnError> {
        if self.story_hash != compiled.story_hash {
            return Err(VnError::diagnostic(
                "ASTRA_VN_RUNTIME_INDEX_STORY_MISMATCH",
                "runtime index does not belong to the compiled story",
            ));
        }
        Ok(())
    }
}

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
        tracing::trace!(
            event = "vn.runtime.command.start",
            before_state_hash = %before,
            "AstraVN player command started"
        );
        let before_variables = self.state.variables.clone();
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
                                let entry = self
                                    .state
                                    .variables
                                    .entry(scope)
                                    .or_default()
                                    .entry(key)
                                    .or_default();
                                *entry = match op {
                                    MutationOp::Set => Some(value),
                                    MutationOp::Add => entry.checked_add(value),
                                    MutationOp::Sub => entry.checked_sub(value),
                                }
                                .ok_or_else(|| {
                                    VnError::diagnostic(
                                        "ASTRA_VN_SYSTEM_ACTION_MUTATION_OVERFLOW",
                                        "system action mutation overflowed i64",
                                    )
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
        let mutations = variable_mutations(&before_variables, &self.state.variables);
        Ok(VnStepOutput {
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
                    presentation.push(PresentationCommand::Dialogue {
                        key,
                        speaker,
                        voice,
                        window,
                    });
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

    fn set_pending_wait(&mut self, mut wait: VnWaitState) -> Result<(), VnError> {
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

    fn choose(&mut self, option_id: &str, reached: &mut BTreeSet<String>) -> Result<(), VnError> {
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

    fn command_at_cursor(&self, cursor: &VnCommandCursor) -> Option<&CompiledCommand> {
        let state = self.compiled.states.get(&cursor.state_id)?;
        let (scene_index, command_index) = *self
            .index
            .commands_by_state
            .get(&cursor.state_id)?
            .get(cursor.ordinal)?;
        state.scenes.get(scene_index)?.commands.get(command_index)
    }

    fn choice_option_enabled(&self, option: &ChoiceOption) -> Result<bool, VnError> {
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

    fn jump_to_state(
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

    fn command_location(&self, command_id: &str) -> Result<(String, String, usize), VnError> {
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

    fn cursor_for(
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

    fn advance_cursor(&mut self) -> Result<(), VnError> {
        let cursor = self.state.cursor.clone().ok_or_else(|| {
            VnError::diagnostic("ASTRA_VN_CURSOR_MISSING", "VN command cursor is not set")
        })?;
        self.state.cursor =
            Some(self.cursor_for(&cursor.story_id, &cursor.state_id, cursor.ordinal + 1)?);
        Ok(())
    }

    fn story_for_state(&self, state_id: &str) -> Result<String, VnError> {
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

    fn open_system_story(
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

    fn switch_system_story(
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

    fn fast_forward_until_blocked(
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
                    let presentation_start = presentation.len();
                    self.run_until_blocked(presentation, reached)?;
                    if matches!(
                        self.state.pending_wait.as_ref().map(|wait| wait.kind),
                        Some(VnWaitKind::Dialogue | VnWaitKind::Input)
                    ) && self.state.system_stack.is_empty()
                    {
                        let retained = presentation
                            .drain(presentation_start..)
                            .filter(|command| {
                                !matches!(command, PresentationCommand::Dialogue { .. })
                            })
                            .collect::<Vec<_>>();
                        presentation.extend(retained);
                    }
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

fn compiled_command_id(command: &CompiledCommand) -> String {
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

fn vn_audio_command(command: &PresentationCommand) -> Option<crate::VnAudioCommand> {
    let PresentationCommand::Stage(StageCommand::Audio(cue)) = command else {
        return None;
    };
    Some(crate::VnAudioCommand {
        command_id: cue.id.clone(),
        cue: cue.clone(),
    })
}

fn vn_timeline_task(command: &PresentationCommand) -> Option<crate::VnTimelineTask> {
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

fn variable_mutations(
    before: &BTreeMap<String, BTreeMap<String, i64>>,
    after: &BTreeMap<String, BTreeMap<String, i64>>,
) -> Vec<crate::VnMutationRecord> {
    let scopes = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut mutations = Vec::new();
    for scope in scopes {
        let keys = before
            .get(&scope)
            .into_iter()
            .flat_map(|values| values.keys())
            .chain(
                after
                    .get(&scope)
                    .into_iter()
                    .flat_map(|values| values.keys()),
            )
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in keys {
            let previous = before
                .get(&scope)
                .and_then(|values| values.get(&key))
                .copied();
            let current = after
                .get(&scope)
                .and_then(|values| values.get(&key))
                .copied();
            if previous != current {
                mutations.push(crate::VnMutationRecord {
                    scope: scope.clone(),
                    key,
                    before: previous,
                    after: current,
                });
            }
        }
    }
    mutations
}

fn route_flag_kind_id(kind: VnRouteFlagKind) -> &'static str {
    match kind {
        VnRouteFlagKind::Launch => "launch",
        VnRouteFlagKind::Choice => "choice",
        VnRouteFlagKind::Jump => "jump",
        VnRouteFlagKind::Branch => "branch",
        VnRouteFlagKind::Call => "call",
        VnRouteFlagKind::Return => "return",
    }
}

fn wait_state_from_presentation(
    command_id: &str,
    command: &PresentationCommand,
) -> Result<Option<VnWaitState>, VnError> {
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

fn required_typed_fence(command_id: &str, fence: Option<&str>) -> Result<String, VnError> {
    fence.map(str::to_string).ok_or_else(|| {
        VnError::diagnostic(
            "ASTRA_VN_TYPED_FENCE_MISSING",
            format!("typed presentation command {command_id} is missing its required fence"),
        )
    })
}

fn validate_system_value(label: &str, value: &str, max_bytes: usize) -> Result<(), VnError> {
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
