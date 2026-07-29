use std::collections::{BTreeMap, VecDeque};

use astra_core::{Diagnostic, Hash128};
use astra_worker_budget::WorkerBudgetBroker;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{MovieLoopMode, PresentationInterruptPolicy, VnError, VnMovieEndBehavior};

pub const PRESENTATION_COORDINATOR_SCHEMA: &str = "astra.vn.presentation_coordinator.v3";
const MAX_REGION_QUEUE: usize = 4_096;
const MAX_FRAME_DELTA_NS: u64 = 1_000_000_000;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PresentationRegion {
    Character,
    Background,
    Text,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationCommandEnvelope {
    pub fixed_step: u64,
    pub sequence: u64,
    pub command_id: String,
    pub interrupt: PresentationInterruptPolicy,
    pub fence: Option<String>,
    pub payload: PresentationRegionCommand,
}

impl PresentationCommandEnvelope {
    pub fn region(&self) -> PresentationRegion {
        match self.payload {
            PresentationRegionCommand::Character(_) => PresentationRegion::Character,
            PresentationRegionCommand::Background(_) => PresentationRegion::Background,
            PresentationRegionCommand::Text(_) => PresentationRegion::Text,
            PresentationRegionCommand::Video(_) => PresentationRegion::Video,
        }
    }

    fn layer_write(&self) -> Option<&str> {
        match &self.payload {
            PresentationRegionCommand::Character(command) => Some(command.layer.as_str()),
            PresentationRegionCommand::Background(command) => Some(command.layer.as_str()),
            PresentationRegionCommand::Video(command) => Some(command.layer.as_str()),
            PresentationRegionCommand::Text(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "region", content = "command", rename_all = "snake_case")]
pub enum PresentationRegionCommand {
    Character(CharacterRegionCommand),
    Background(BackgroundRegionCommand),
    Text(TextRegionCommand),
    Video(VideoRegionCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRegionCommand {
    pub character_id: String,
    pub asset: String,
    pub pose: Option<String>,
    pub layer: String,
    pub visible: bool,
    pub duration_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundRegionCommand {
    pub layer: String,
    pub asset: Option<String>,
    pub duration_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextRegionCommand {
    pub text_key: String,
    pub speaker: Option<String>,
    pub window: Option<String>,
    pub grapheme_count: u32,
    pub graphemes_per_second: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoRegionCommand {
    pub session_id: String,
    pub layer: String,
    pub asset: String,
    pub logical_start_ns: u64,
    pub loop_mode: MovieLoopMode,
    pub end_behavior: VnMovieEndBehavior,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterPresentationState {
    pub command_id: String,
    pub asset: String,
    pub pose: Option<String>,
    pub layer: String,
    pub visible: bool,
    pub elapsed_ns: u64,
    pub duration_ns: u64,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRegionState {
    pub characters: BTreeMap<String, CharacterPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundPresentationState {
    pub command_id: String,
    pub current: Option<String>,
    pub incoming: Option<String>,
    pub elapsed_ns: u64,
    pub duration_ns: u64,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundRegionState {
    pub layers: BTreeMap<String, BackgroundPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextPresentationState {
    pub command_id: String,
    pub text_key: String,
    pub speaker: Option<String>,
    pub window: Option<String>,
    pub grapheme_count: u32,
    pub visible_graphemes: u32,
    pub elapsed_ns: u64,
    pub graphemes_per_second: u16,
    pub layout_pending: bool,
    pub auto_timer_ns: Option<u64>,
    pub fence: Option<String>,
}

impl TextPresentationState {
    pub fn reveal_complete(&self) -> bool {
        self.visible_graphemes >= self.grapheme_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextRegionState {
    pub active: Option<TextPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoPhase {
    Prebuffering,
    Playing,
    Paused,
    Seeking,
    Ended,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoPresentationState {
    pub command_id: String,
    pub layer: String,
    pub asset: String,
    pub phase: VideoPhase,
    pub logical_time_ns: u64,
    pub loop_mode: MovieLoopMode,
    pub end_behavior: VnMovieEndBehavior,
    pub fallback: Option<String>,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoRegionState {
    pub sessions: BTreeMap<String, VideoPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FenceStatus {
    Pending,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationCoordinatorState {
    pub schema: String,
    pub character: CharacterRegionState,
    pub background: BackgroundRegionState,
    pub text: TextRegionState,
    pub video: VideoRegionState,
    pub fences: BTreeMap<String, FenceStatus>,
    pub activated_commands: VecDeque<String>,
    pub last_fixed_step: u64,
    pub last_sequence: Option<u64>,
}

impl Default for PresentationCoordinatorState {
    fn default() -> Self {
        Self {
            schema: PRESENTATION_COORDINATOR_SCHEMA.to_string(),
            character: CharacterRegionState {
                characters: BTreeMap::new(),
                queued: VecDeque::new(),
            },
            background: BackgroundRegionState {
                layers: BTreeMap::new(),
                queued: VecDeque::new(),
            },
            text: TextRegionState {
                active: None,
                queued: VecDeque::new(),
            },
            video: VideoRegionState {
                sessions: BTreeMap::new(),
                queued: VecDeque::new(),
            },
            fences: BTreeMap::new(),
            activated_commands: VecDeque::new(),
            last_fixed_step: 0,
            last_sequence: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationRegionDelta {
    pub region: PresentationRegion,
    pub first_sequence: u64,
    pub applied_commands: Vec<String>,
    pub completed_fences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextAdvanceDisposition {
    RevealCompleted,
    StoryAdvanceRequested,
    NoActiveText,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationCoordinator {
    state: PresentationCoordinatorState,
}

impl PresentationCoordinator {
    pub fn state(&self) -> &PresentationCoordinatorState {
        &self.state
    }

    pub fn stable_hash(&self) -> Result<Hash128, VnError> {
        Ok(Hash128::from_blake3(&postcard::to_allocvec(&self.state)?))
    }

    pub fn prepare_batch(
        &self,
        commands: &[PresentationCommandEnvelope],
        worker_count: usize,
    ) -> Result<(Self, Vec<PresentationRegionDelta>), VnError> {
        WorkerBudgetBroker::global()
            .run_scoped(|| self.prepare_batch_scoped(commands, worker_count))
            .map_err(|error| coordinator_error_owned(error.code(), error.to_string()))?
    }

    fn prepare_batch_scoped(
        &self,
        commands: &[PresentationCommandEnvelope],
        worker_count: usize,
    ) -> Result<(Self, Vec<PresentationRegionDelta>), VnError> {
        if !(1..=8).contains(&worker_count) {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_WORKER_COUNT",
                "presentation worker count must be within 1..=8",
            ));
        }
        validate_batch(&self.state, commands)?;
        let mut grouped: BTreeMap<PresentationRegion, Vec<PresentationCommandEnvelope>> =
            BTreeMap::new();
        for command in commands {
            grouped
                .entry(command.region())
                .or_default()
                .push(command.clone());
        }

        let character = self.state.character.clone();
        let background = self.state.background.clone();
        let text = self.state.text.clone();
        let video = self.state.video.clone();
        let execute = |region: PresentationRegion,
                       commands: Vec<PresentationCommandEnvelope>|
         -> Result<PreparedRegion, VnError> {
            match region {
                PresentationRegion::Character => prepare_character(character.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Character(state, delta)),
                PresentationRegion::Background => prepare_background(background.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Background(state, delta)),
                PresentationRegion::Text => prepare_text(text.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Text(state, delta)),
                PresentationRegion::Video => prepare_video(video.clone(), commands)
                    .map(|(state, delta)| PreparedRegion::Video(state, delta)),
            }
        };

        let prepared = if worker_count == 1 || grouped.len() <= 1 {
            grouped
                .into_iter()
                .map(|(region, commands)| execute(region, commands))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let jobs = grouped.into_iter().collect::<Vec<_>>();
            let mut leases = Vec::new();
            for _ in 1..worker_count.min(jobs.len()) {
                match WorkerBudgetBroker::global()
                    .try_acquire()
                    .map_err(|error| coordinator_error_owned(error.code(), error.to_string()))?
                {
                    Some(lease) => leases.push(lease),
                    None => break,
                }
            }
            let workers = (leases.len() + 1).min(jobs.len());
            let chunk_size = jobs.len().div_ceil(workers);
            std::thread::scope(|scope| {
                let mut chunks = jobs
                    .chunks(chunk_size)
                    .map(<[_]>::to_vec)
                    .collect::<Vec<_>>();
                let caller_chunk = chunks.remove(0);
                let handles = chunks
                    .into_iter()
                    .zip(leases)
                    .map(|(chunk, lease)| {
                        scope.spawn(move || {
                            let _lease = lease;
                            chunk
                                .iter()
                                .cloned()
                                .map(|(region, commands)| execute(region, commands))
                                .collect::<Result<Vec<_>, _>>()
                        })
                    })
                    .collect::<Vec<_>>();
                let mut prepared = caller_chunk
                    .into_iter()
                    .map(|(region, commands)| execute(region, commands))
                    .collect::<Result<Vec<_>, _>>()?;
                for handle in handles {
                    let mut chunk = handle.join().map_err(|_| {
                        coordinator_error(
                            "ASTRA_VN_PRESENTATION_WORKER_PANIC",
                            "presentation region worker panicked",
                        )
                    })??;
                    prepared.append(&mut chunk);
                }
                Ok::<Vec<PreparedRegion>, VnError>(prepared)
            })?
        };

        let mut next = self.clone();
        let mut deltas = Vec::new();
        for region in prepared {
            let delta = match region {
                PreparedRegion::Character(state, delta) => {
                    next.state.character = state;
                    delta
                }
                PreparedRegion::Background(state, delta) => {
                    next.state.background = state;
                    delta
                }
                PreparedRegion::Text(state, delta) => {
                    next.state.text = state;
                    delta
                }
                PreparedRegion::Video(state, delta) => {
                    next.state.video = state;
                    delta
                }
            };
            for fence in &delta.completed_fences {
                next.state
                    .fences
                    .insert(fence.clone(), FenceStatus::Completed);
            }
            deltas.push(delta);
        }
        for command in commands {
            if let Some(fence) = &command.fence {
                next.state
                    .fences
                    .entry(fence.clone())
                    .or_insert(FenceStatus::Pending);
            }
        }
        deltas.sort_by_key(|delta| (delta.first_sequence, delta.region));
        if let Some(command) = commands.last() {
            next.state.last_fixed_step = command.fixed_step;
            next.state.last_sequence = Some(command.sequence);
        }
        Ok((next, deltas))
    }

    pub fn apply_batch(
        &mut self,
        commands: &[PresentationCommandEnvelope],
        worker_count: usize,
    ) -> Result<Vec<PresentationRegionDelta>, VnError> {
        let (next, deltas) = self.prepare_batch(commands, worker_count)?;
        *self = next;
        Ok(deltas)
    }

    pub fn tick(&mut self, delta_ns: u64) -> Result<Vec<String>, VnError> {
        if delta_ns == 0 || delta_ns > MAX_FRAME_DELTA_NS {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_TICK_DELTA",
                "presentation frame delta is outside the fixed-step budget",
            ));
        }
        let mut next = self.clone();
        let mut completed = Vec::new();
        for state in next.state.character.characters.values_mut() {
            state.elapsed_ns = state
                .elapsed_ns
                .saturating_add(delta_ns)
                .min(state.duration_ns);
            if state.elapsed_ns == state.duration_ns {
                if let Some(fence) = state.fence.take() {
                    completed.push(fence);
                }
            }
        }
        for state in next.state.background.layers.values_mut() {
            state.elapsed_ns = state
                .elapsed_ns
                .saturating_add(delta_ns)
                .min(state.duration_ns);
            if state.elapsed_ns == state.duration_ns {
                state.current = state.incoming.take();
                if let Some(fence) = state.fence.take() {
                    completed.push(fence);
                }
            }
        }
        if let Some(text) = next.state.text.active.as_mut() {
            text.elapsed_ns = text.elapsed_ns.saturating_add(delta_ns);
            let visible = (u128::from(text.elapsed_ns) * u128::from(text.graphemes_per_second)
                / 1_000_000_000_u128)
                .min(u128::from(text.grapheme_count)) as u32;
            text.visible_graphemes = visible;
            if text.reveal_complete() {
                if let Some(fence) = text.fence.take() {
                    completed.push(fence);
                }
            }
        }
        for video in next.state.video.sessions.values_mut() {
            if video.phase == VideoPhase::Playing {
                video.logical_time_ns = video.logical_time_ns.saturating_add(delta_ns);
            }
        }
        let (character, activated_character) = drain_character_queue(next.state.character)?;
        next.state.character = character;
        let (background, activated_background) = drain_background_queue(next.state.background)?;
        next.state.background = background;
        let (video, activated_video) = drain_video_queue(next.state.video)?;
        next.state.video = video;
        for command_id in activated_character
            .into_iter()
            .chain(activated_background)
            .chain(activated_video)
        {
            push_activated(&mut next.state.activated_commands, command_id)?;
        }
        completed.sort();
        completed.dedup();
        for fence in &completed {
            next.state
                .fences
                .insert(fence.clone(), FenceStatus::Completed);
        }
        *self = next;
        Ok(completed)
    }

    pub fn request_text_advance(&mut self) -> TextAdvanceDisposition {
        let Some(text) = self.state.text.active.as_mut() else {
            return TextAdvanceDisposition::NoActiveText;
        };
        if !text.reveal_complete() {
            text.visible_graphemes = text.grapheme_count;
            if let Some(fence) = text.fence.take() {
                self.state.fences.insert(fence, FenceStatus::Completed);
            }
            TextAdvanceDisposition::RevealCompleted
        } else {
            TextAdvanceDisposition::StoryAdvanceRequested
        }
    }

    pub fn complete_text_layout(&mut self, command_id: &str) -> Result<(), VnError> {
        let text = self.state.text.active.as_mut().ok_or_else(|| {
            coordinator_error(
                "ASTRA_VN_TEXT_LAYOUT_STATE",
                "text layout completion references an empty text region",
            )
        })?;
        if text.command_id != command_id {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_LAYOUT_IDENTITY",
                "text layout completion does not match the active command",
            ));
        }
        text.layout_pending = false;
        Ok(())
    }

    pub fn acknowledge_story_advance(&mut self) -> Result<(), VnError> {
        let Some(active) = self.state.text.active.as_ref() else {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_ADVANCE_STATE",
                "story advance cannot acknowledge an empty text region",
            ));
        };
        if !active.reveal_complete() {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_ADVANCE_STATE",
                "story advance cannot acknowledge an incomplete text reveal",
            ));
        }
        self.state.text.active = None;
        let (text, activated) = drain_text_queue(std::mem::replace(
            &mut self.state.text,
            TextRegionState {
                active: None,
                queued: VecDeque::new(),
            },
        ))?;
        self.state.text = text;
        for command_id in activated {
            push_activated(&mut self.state.activated_commands, command_id)?;
        }
        Ok(())
    }

    pub fn take_activated_commands(&mut self) -> Vec<String> {
        self.state.activated_commands.drain(..).collect()
    }

    pub fn start_video(&mut self, session_id: &str) -> Result<(), VnError> {
        let video = self
            .state
            .video
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| {
                coordinator_error(
                    "ASTRA_VN_VIDEO_SESSION_MISSING",
                    "video start references an unknown presentation session",
                )
            })?;
        match video.phase {
            VideoPhase::Prebuffering => video.phase = VideoPhase::Playing,
            VideoPhase::Playing => {}
            _ => {
                return Err(coordinator_error(
                    "ASTRA_VN_VIDEO_PHASE",
                    "decoded video frames are accepted only while prebuffering or playing",
                ));
            }
        }
        Ok(())
    }

    pub fn complete_video(&mut self, session_id: &str) -> Result<Vec<String>, VnError> {
        let video = self
            .state
            .video
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| {
                coordinator_error(
                    "ASTRA_VN_VIDEO_SESSION_MISSING",
                    "video completion references an unknown presentation session",
                )
            })?;
        video.phase = VideoPhase::Ended;
        let mut completed = Vec::new();
        if let Some(fence) = video.fence.take() {
            self.state
                .fences
                .insert(fence.clone(), FenceStatus::Completed);
            completed.push(fence);
        }
        let (video, activated) = drain_video_queue(std::mem::replace(
            &mut self.state.video,
            VideoRegionState {
                sessions: BTreeMap::new(),
                queued: VecDeque::new(),
            },
        ))?;
        self.state.video = video;
        for command_id in activated {
            push_activated(&mut self.state.activated_commands, command_id)?;
        }
        Ok(completed)
    }

    pub fn fail_video(&mut self, session_id: &str) -> Result<Option<String>, VnError> {
        let fallback = {
            let video = self
                .state
                .video
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| {
                    coordinator_error(
                        "ASTRA_VN_VIDEO_SESSION_MISSING",
                        "video failure references an unknown presentation session",
                    )
                })?;
            video.phase = VideoPhase::Failed;
            if let Some(fence) = video.fence.take() {
                self.state.fences.insert(fence, FenceStatus::Failed);
            }
            video.fallback.clone()
        };
        let state = std::mem::replace(
            &mut self.state.video,
            VideoRegionState {
                sessions: BTreeMap::new(),
                queued: VecDeque::new(),
            },
        );
        let (video, activated) = drain_video_queue(state)?;
        self.state.video = video;
        for command_id in activated {
            push_activated(&mut self.state.activated_commands, command_id)?;
        }
        Ok(fallback)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, VnError> {
        postcard::to_allocvec(self).map_err(Into::into)
    }

    pub fn restore(bytes: &[u8]) -> Result<Self, VnError> {
        let restored: Self = postcard::from_bytes(bytes)?;
        if restored.state.schema != PRESENTATION_COORDINATOR_SCHEMA {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_SNAPSHOT_SCHEMA",
                "presentation coordinator snapshot schema is invalid",
            ));
        }
        validate_queues(&restored.state)?;
        Ok(restored)
    }
}

enum PreparedRegion {
    Character(CharacterRegionState, PresentationRegionDelta),
    Background(BackgroundRegionState, PresentationRegionDelta),
    Text(TextRegionState, PresentationRegionDelta),
    Video(VideoRegionState, PresentationRegionDelta),
}

fn validate_batch(
    state: &PresentationCoordinatorState,
    commands: &[PresentationCommandEnvelope],
) -> Result<(), VnError> {
    let mut prior = state.last_sequence;
    let mut layer_regions: BTreeMap<&str, PresentationRegion> = BTreeMap::new();
    for command in commands {
        if command.command_id.is_empty()
            || command.fence.as_ref().is_some_and(String::is_empty)
            || prior.is_some_and(|value| command.sequence <= value)
            || command.fixed_step < state.last_fixed_step
        {
            return Err(coordinator_error(
                "ASTRA_VN_PRESENTATION_COMMAND_ORDER",
                "presentation command identity, sequence or fixed step is invalid",
            ));
        }
        prior = Some(command.sequence);
        if let Some(layer) = command.layer_write() {
            if let Some(previous) = layer_regions.insert(layer, command.region()) {
                if previous != command.region() {
                    return Err(coordinator_error(
                        "ASTRA_VN_PRESENTATION_REGION_WRITE_CONFLICT",
                        "multiple presentation regions write the same layer in one transaction",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_queues(state: &PresentationCoordinatorState) -> Result<(), VnError> {
    if [
        state.character.queued.len(),
        state.background.queued.len(),
        state.text.queued.len(),
        state.video.queued.len(),
        state.activated_commands.len(),
    ]
    .into_iter()
    .any(|len| len > MAX_REGION_QUEUE)
    {
        return Err(coordinator_error(
            "ASTRA_VN_PRESENTATION_QUEUE_LIMIT",
            "presentation region queue exceeds its serialized bound",
        ));
    }
    Ok(())
}

fn prepare_character(
    mut state: CharacterRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(CharacterRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Character(command) = &envelope.payload else {
            unreachable!("region classifier guarantees character payloads");
        };
        let active = state
            .characters
            .get(&command.character_id)
            .is_some_and(|value| value.elapsed_ns < value.duration_ns);
        if active {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_CHARACTER_INTERRUPT_REJECTED",
                        "character command conflicts with an active transition",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        state.characters.insert(
            command.character_id.clone(),
            CharacterPresentationState {
                command_id: envelope.command_id.clone(),
                asset: command.asset.clone(),
                pose: command.pose.clone(),
                layer: command.layer.clone(),
                visible: command.visible,
                elapsed_ns: 0,
                duration_ns: command.duration_ns,
                fence: envelope.fence.clone(),
            },
        );
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Character,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

fn prepare_background(
    mut state: BackgroundRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(BackgroundRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Background(command) = &envelope.payload else {
            unreachable!("region classifier guarantees background payloads");
        };
        let existing = state.layers.get(&command.layer).cloned();
        let active = existing
            .as_ref()
            .is_some_and(|value| value.elapsed_ns < value.duration_ns);
        if active {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_BACKGROUND_INTERRUPT_REJECTED",
                        "background command conflicts with an active transition",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        let current = existing.and_then(|value| match envelope.interrupt {
            PresentationInterruptPolicy::SnapThenStart => value.incoming.or(value.current),
            _ => value.current,
        });
        state.layers.insert(
            command.layer.clone(),
            BackgroundPresentationState {
                command_id: envelope.command_id.clone(),
                current,
                incoming: command.asset.clone(),
                elapsed_ns: 0,
                duration_ns: command.duration_ns,
                fence: envelope.fence.clone(),
            },
        );
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Background,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

fn prepare_text(
    mut state: TextRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(TextRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Text(command) = &envelope.payload else {
            unreachable!("region classifier guarantees text payloads");
        };
        if command.graphemes_per_second == 0 {
            return Err(coordinator_error(
                "ASTRA_VN_TEXT_REVEAL_RATE",
                "text reveal rate must be non-zero",
            ));
        }
        if state.active.is_some() {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_TEXT_INTERRUPT_REJECTED",
                        "text command conflicts with an active line",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        state.active = Some(TextPresentationState {
            command_id: envelope.command_id.clone(),
            text_key: command.text_key.clone(),
            speaker: command.speaker.clone(),
            window: command.window.clone(),
            grapheme_count: command.grapheme_count,
            visible_graphemes: 0,
            elapsed_ns: 0,
            graphemes_per_second: command.graphemes_per_second,
            layout_pending: true,
            auto_timer_ns: None,
            fence: envelope.fence.clone(),
        });
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Text,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

fn prepare_video(
    mut state: VideoRegionState,
    commands: Vec<PresentationCommandEnvelope>,
) -> Result<(VideoRegionState, PresentationRegionDelta), VnError> {
    let mut applied = Vec::new();
    let first_sequence = commands.first().map_or(0, |command| command.sequence);
    for envelope in commands {
        let PresentationRegionCommand::Video(command) = &envelope.payload else {
            unreachable!("region classifier guarantees video payloads");
        };
        let active = state
            .sessions
            .get(&command.session_id)
            .is_some_and(|video| !matches!(video.phase, VideoPhase::Ended | VideoPhase::Failed));
        if active {
            match envelope.interrupt {
                PresentationInterruptPolicy::Queue => {
                    push_queued(&mut state.queued, envelope)?;
                    continue;
                }
                PresentationInterruptPolicy::Reject => {
                    return Err(coordinator_error(
                        "ASTRA_VN_VIDEO_INTERRUPT_REJECTED",
                        "video command conflicts with an active playback session",
                    ));
                }
                PresentationInterruptPolicy::SnapThenStart
                | PresentationInterruptPolicy::ReplaceFromCurrent => {}
            }
        }
        state.sessions.insert(
            command.session_id.clone(),
            VideoPresentationState {
                command_id: envelope.command_id.clone(),
                layer: command.layer.clone(),
                asset: command.asset.clone(),
                phase: VideoPhase::Prebuffering,
                logical_time_ns: command.logical_start_ns,
                loop_mode: command.loop_mode,
                end_behavior: command.end_behavior,
                fallback: command.fallback.clone(),
                fence: envelope.fence.clone(),
            },
        );
        applied.push(envelope.command_id);
    }
    Ok((
        state,
        PresentationRegionDelta {
            region: PresentationRegion::Video,
            first_sequence,
            applied_commands: applied,
            completed_fences: vec![],
        },
    ))
}

fn push_queued(
    queue: &mut VecDeque<PresentationCommandEnvelope>,
    command: PresentationCommandEnvelope,
) -> Result<(), VnError> {
    if queue.len() >= MAX_REGION_QUEUE {
        return Err(coordinator_error(
            "ASTRA_VN_PRESENTATION_QUEUE_LIMIT",
            "presentation region queue is full",
        ));
    }
    queue.push_back(command);
    Ok(())
}

fn drain_character_queue(
    mut state: CharacterRegionState,
) -> Result<(CharacterRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_character(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

fn drain_background_queue(
    mut state: BackgroundRegionState,
) -> Result<(BackgroundRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_background(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

fn drain_text_queue(mut state: TextRegionState) -> Result<(TextRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_text(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

fn drain_video_queue(
    mut state: VideoRegionState,
) -> Result<(VideoRegionState, Vec<String>), VnError> {
    let mut activated = Vec::new();
    let queued = std::mem::take(&mut state.queued);
    for command in queued {
        let (next, delta) = prepare_video(state, vec![command])?;
        state = next;
        activated.extend(delta.applied_commands);
    }
    Ok((state, activated))
}

fn push_activated(queue: &mut VecDeque<String>, command_id: String) -> Result<(), VnError> {
    if queue.len() >= MAX_REGION_QUEUE {
        return Err(coordinator_error(
            "ASTRA_VN_PRESENTATION_ACTIVATED_LIMIT",
            "presentation activated-command queue is full",
        ));
    }
    queue.push_back(command_id);
    Ok(())
}

fn coordinator_error(code: &'static str, message: &'static str) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message))
}

fn coordinator_error_owned(code: &'static str, message: String) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message))
}
