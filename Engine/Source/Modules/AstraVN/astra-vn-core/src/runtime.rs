use std::collections::BTreeSet;

use astra_core::Hash128;

use crate::{
    resolve_target, AudioCue, BacklogEntry, BacklogLayoutMetadata, ChoiceOption, CompiledCommand,
    CompiledStory, MutationOp, PendingChoice, PresentationCommand, SkipMode, StageCommand,
    SystemUnlockKind, TimelineCommand, VnAudioBus, VnAudioSync, VnCallFrame, VnCommandCursor,
    VnCoverage, VnError, VnMovieEndBehavior, VnPlayerCommand, VnReplayUiState, VnRouteFlag,
    VnRouteFlagKind, VnRunConfig, VnRuntimeState, VnSaveBlob, VnStepOutput, VnSystemFrame,
    VnTimelineJoinPolicy, VnWaitKind, VnWaitState, VoiceReplayEntry,
};

#[derive(Debug, Clone)]
pub struct VnRuntime {
    compiled: CompiledStory,
    state: VnRuntimeState,
}

impl VnRuntime {
    pub fn new(compiled: impl Into<CompiledStory>, config: VnRunConfig) -> Result<Self, VnError> {
        let compiled = compiled.into();
        tracing::info!(
            event = "vn.runtime.create",
            profile = %config.profile,
            locale = %config.locale,
            state_count = compiled.states.len(),
            "AstraVN runtime created"
        );
        Ok(Self {
            compiled,
            state: VnRuntimeState {
                schema: "astra.vn.runtime_state.v1".to_string(),
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
                pending_wait: None,
            },
        })
    }

    pub fn from_state(
        compiled: impl Into<CompiledStory>,
        state: VnRuntimeState,
    ) -> Result<Self, VnError> {
        let compiled = compiled.into();
        if state.schema != "astra.vn.runtime_state.v1" {
            return Err(VnError::diagnostic(
                "ASTRA_VN_RUNTIME_STATE_SCHEMA",
                "VN runtime state schema is invalid",
            ));
        }
        Ok(Self { compiled, state })
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
        let before_state = self.state.clone();
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
                match self.state.pending_wait.as_ref().map(|wait| wait.kind) {
                    Some(VnWaitKind::Dialogue) => {
                        self.state.pending_wait = None;
                        self.run_until_blocked(&mut presentation, &mut reached)?;
                    }
                    None => self.run_until_blocked(&mut presentation, &mut reached)?,
                    Some(_) => {}
                }
            }
            VnPlayerCommand::Choose { option_id } => {
                self.choose(&option_id, &mut reached)?;
                self.run_until_blocked(&mut presentation, &mut reached)?;
            }
            VnPlayerCommand::OpenSystem { page } => {
                self.open_system_story(page, &mut presentation, &mut reached)?;
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
        let mutations = variable_mutations(&before_state, &self.state);
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
                    self.state.pending_wait = Some(VnWaitState::new(
                        VnWaitKind::Dialogue,
                        format!("dialogue:{id}"),
                        id,
                    ));
                    return Ok(());
                }
                CompiledCommand::Choice { id, key, options } => {
                    self.advance_cursor()?;
                    self.state.pending_choice = Some(PendingChoice {
                        choice_id: id.clone(),
                        key: key.clone(),
                        options: options.clone(),
                    });
                    presentation.push(PresentationCommand::Choice { key, options });
                    self.state.pending_wait = Some(VnWaitState::new(
                        VnWaitKind::Choice,
                        format!("choice:{id}"),
                        id,
                    ));
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
                        self.state.pending_wait = Some(VnWaitState::new(
                            VnWaitKind::SystemPage,
                            format!("system:{id}"),
                            id,
                        ));
                    }
                    return Ok(());
                }
                CompiledCommand::Presentation { id, command } => {
                    self.advance_cursor()?;
                    let wait = wait_state_from_presentation(&id, &command)?;
                    presentation.push(command);
                    if let Some(wait) = wait {
                        self.state.pending_wait = Some(wait);
                        return Ok(());
                    }
                }
                CompiledCommand::Wait { id, fence } => {
                    self.advance_cursor()?;
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
        self.compiled
            .states
            .get(&cursor.state_id)?
            .scenes
            .iter()
            .flat_map(|scene| &scene.commands)
            .nth(cursor.ordinal)
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
        let command = state
            .scenes
            .iter()
            .flat_map(|scene| {
                scene
                    .commands
                    .iter()
                    .map(move |command| (scene.id.as_str(), command))
            })
            .nth(ordinal);
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
        self.compiled
            .stories
            .iter()
            .find(|story| story.states.iter().any(|state| state == state_id))
            .map(|story| story.id.clone())
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
    compiled: &CompiledStory,
    state: &VnRuntimeState,
    command: VnPlayerCommand,
) -> Result<(VnRuntimeState, VnStepOutput), VnError> {
    let mut runtime = VnRuntime::from_state(compiled.clone(), state.clone())?;
    let output = runtime.apply(command)?;
    Ok((runtime.state, output))
}

fn compiled_command_id(command: &CompiledCommand) -> String {
    match command {
        CompiledCommand::Dialogue { id, .. }
        | CompiledCommand::Choice { id, .. }
        | CompiledCommand::Jump { id, .. }
        | CompiledCommand::Call { id, .. }
        | CompiledCommand::Return { id }
        | CompiledCommand::Mutate { id, .. }
        | CompiledCommand::SystemPage { id, .. }
        | CompiledCommand::Presentation { id, .. }
        | CompiledCommand::Wait { id, .. } => id.clone(),
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
    before: &VnRuntimeState,
    after: &VnRuntimeState,
) -> Vec<crate::VnMutationRecord> {
    let scopes = before
        .variables
        .keys()
        .chain(after.variables.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut mutations = Vec::new();
    for scope in scopes {
        let keys = before
            .variables
            .get(&scope)
            .into_iter()
            .flat_map(|values| values.keys())
            .chain(
                after
                    .variables
                    .get(&scope)
                    .into_iter()
                    .flat_map(|values| values.keys()),
            )
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in keys {
            let previous = before
                .variables
                .get(&scope)
                .and_then(|values| values.get(&key))
                .copied();
            let current = after
                .variables
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
