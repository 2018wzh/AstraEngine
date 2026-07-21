use std::collections::BTreeMap;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    script::tokenize_operands, ScCommand, ScControlFlow, ScLineKind, ScOperand, ScScript,
    SourceSpan,
};

pub const MINORI_RUNTIME_STATE_SCHEMA: &str = "astra.emu.minori.runtime_state.v6";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriRuntimeState {
    pub schema: String,
    pub script_uri: String,
    pub script_hash: Hash256,
    pub pc_line: u32,
    pub variables: BTreeMap<String, i64>,
    pub global_variables: BTreeMap<String, i64>,
    pub wait: Option<MinoriWaitState>,
    pub message: Option<MinoriMessageState>,
    pub choice: Option<MinoriChoiceState>,
    pub layers: BTreeMap<u32, MinoriLayerState>,
    pub transition: MinoriTransitionState,
    pub effect: Option<MinoriEffectState>,
    pub panel: Option<MinoriPanelState>,
    pub audio: BTreeMap<u32, MinoriAudioState>,
    pub movie: Option<MinoriMovieState>,
    pub system_ui: MinoriSystemUiState,
    pub gallery_unlocks: Vec<Hash256>,
    pub fixed_tick: u64,
    pub session_seed: u64,
    pub random_state: u64,
    pub instruction_count: u64,
    pub effect_sequence: u64,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriWaitState {
    Time {
        token_id: String,
        timer_ticks: u32,
        milliseconds: u32,
    },
    Input {
        token_id: String,
    },
    Media {
        token_id: String,
        media_id: String,
    },
    Presentation {
        token_id: String,
        fence_id: String,
    },
    Provider {
        token_id: String,
        request_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriMessageState {
    pub source: SourceSpan,
    pub message_id: i64,
    pub text_hash: Hash256,
    pub speaker_hash: Option<Hash256>,
    pub voice_hash: Option<Hash256>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriChoiceState {
    pub source: SourceSpan,
    pub option_hashes: Vec<Hash256>,
    pub selected_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriLayerState {
    pub resource_uri: String,
    pub content_hash: Option<Hash256>,
    pub x_milli: i32,
    pub y_milli: i32,
    pub scale_x_milli: i32,
    pub scale_y_milli: i32,
    pub opacity_milli: u16,
    pub blend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MinoriTransitionState {
    pub mode: i32,
    pub resource: Option<String>,
    pub duration_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriEffectState {
    pub kind: MinoriEffectKind,
    pub resources: Vec<Option<String>>,
    pub current_index: u32,
    pub next_index: u32,
    pub alpha_255: u32,
    pub alpha_step: u32,
    pub interval_ms: u32,
    pub elapsed_ns: u64,
    pub visible_current_index: u32,
    pub visible_next_index: u32,
    pub visible_alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriPanelState {
    pub mode: u32,
    pub resource_uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriEffectKind {
    CrossFade2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriEffectFrame {
    pub sequence: u64,
    pub current_resource_uri: Option<String>,
    pub next_resource_uri: Option<String>,
    pub alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriStageCommand {
    pub foreground: Option<MinoriStageLayer>,
    pub background: Option<MinoriStageLayer>,
    pub stands: Vec<MinoriStandLayer>,
    pub transition: MinoriTransitionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriStageLayer {
    pub resource_uri: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriStandLayer {
    pub resource_uri: String,
    pub position: i32,
    pub offset: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriAudioState {
    pub bus: String,
    pub resource_uri: String,
    pub looped: bool,
    pub volume_milli: u16,
    pub pan_milli: i16,
    pub playing: bool,
    pub continuation_pts: u64,
}

/// Resource token accepted by the original BGM/SE path. The bracket suffix is
/// family metadata, not part of the archive entry name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriAudioResourceSpec {
    pub resource: String,
    pub volume_percent: u16,
    pub pan_percent: i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriMovieState {
    pub media_id: String,
    pub resource_uri: String,
    pub width: u32,
    pub height: u32,
    pub skippable: bool,
    pub continuation_pts: u64,
    pub fence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MinoriSystemPage {
    #[default]
    None,
    Title,
    Load,
    Save,
    Config,
    Backlog,
    GalleryCg,
    GalleryBgm,
    GalleryReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MinoriSystemUiState {
    pub page: MinoriSystemPage,
    pub focus_index: u32,
    pub auto_mode: bool,
    pub skip_mode: bool,
    pub backlog_cursor: Option<u32>,
    pub pending_save_slot: Option<u32>,
    pub pending_load_slot: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MinoriVmEvent {
    Wait(MinoriWaitState),
    Message {
        text: String,
        speaker: Option<String>,
        wait: MinoriWaitState,
    },
    Audio {
        commands: Vec<MinoriAudioCommand>,
    },
    Stage(MinoriStageCommand),
    Effect(MinoriEffectFrame),
    Panel {
        sequence: u64,
    },
    Chain {
        target: String,
    },
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MinoriAudioCommand {
    LoadResource {
        sequence: u64,
        stream_id: u32,
        resource_uri: String,
    },
    Play {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    },
    Stop {
        sequence: u64,
        stream_id: u32,
        fade_ms: u32,
    },
    SetParams {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MinoriRuntimeError {
    #[error("ASTRA_EMU_MINORI_RUNTIME_STATE: runtime state is invalid")]
    State,
    #[error("ASTRA_EMU_MINORI_RUNTIME_PC: program counter is outside the script")]
    ProgramCounter,
    #[error("ASTRA_EMU_MINORI_RUNTIME_LABEL: branch label is missing or duplicated")]
    Label,
    #[error("ASTRA_EMU_MINORI_RUNTIME_OPERAND: command operands do not match the verified schema")]
    Operand,
    #[error(
        "ASTRA_EMU_MINORI_RUNTIME_OPCODE: command `{opcode}` at ordinal {ordinal} is not verified"
    )]
    UnsupportedOpcode { opcode: String, ordinal: u32 },
    #[error("ASTRA_EMU_MINORI_RUNTIME_BUDGET: instruction budget is exhausted")]
    Budget,
    #[error("ASTRA_EMU_MINORI_RUNTIME_WAIT: runtime is awaiting an unresolved token")]
    Waiting,
    #[error("ASTRA_EMU_MINORI_RUNTIME_OVERFLOW: deterministic counter overflowed")]
    Overflow,
    #[error("ASTRA_EMU_MINORI_RUNTIME_SNAPSHOT: runtime snapshot is malformed")]
    Snapshot,
    #[error("ASTRA_EMU_MINORI_RUNTIME_CHAIN: chain target is outside the script mount")]
    ChainTarget,
    #[error("ASTRA_EMU_MINORI_RUNTIME_AUDIO_RESOURCE: audio resource specification is invalid")]
    AudioResource,
    #[error("ASTRA_EMU_MINORI_RUNTIME_EFFECT: effect operands or timeline are invalid")]
    Effect,
    #[error("ASTRA_EMU_MINORI_RUNTIME_PANEL: panel operands or mode are invalid")]
    Panel,
}

/// Reproduces the bounded part of the original audio resource parser:
/// `resource[volume,pan]`, with volume clamped to 0..100 and pan to -100..100.
/// A missing closing bracket leaves the default metadata intact, as observed in
/// the original handler. URI resolution remains a separate fail-closed step.
pub fn parse_audio_resource_spec(
    token: &str,
) -> Result<MinoriAudioResourceSpec, MinoriRuntimeError> {
    if token.is_empty() || token.len() > 4 * 1024 || token.contains('\0') {
        return Err(MinoriRuntimeError::AudioResource);
    }
    let Some(open) = token.find('[') else {
        return Ok(MinoriAudioResourceSpec {
            resource: token.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let resource = &token[..open];
    if resource.is_empty() {
        return Err(MinoriRuntimeError::AudioResource);
    }
    let Some(relative_close) = token[open + 1..].find(']') else {
        return Ok(MinoriAudioResourceSpec {
            resource: resource.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let close = open + 1 + relative_close;
    let metadata = &token[open + 1..close];
    let (volume, pan) = metadata
        .split_once(',')
        .map_or((metadata, ""), |(volume, pan)| (volume, pan));
    let volume = parse_c_decimal_prefix(volume).unwrap_or(100).clamp(0, 100);
    let pan = parse_c_decimal_prefix(pan).unwrap_or(0).clamp(-100, 100);
    Ok(MinoriAudioResourceSpec {
        resource: resource.to_owned(),
        volume_percent: volume as u16,
        pan_percent: pan as i16,
    })
}

fn parse_c_decimal_prefix(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digit_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == digit_start {
        return None;
    }
    value[..end].parse().ok()
}

pub struct MinoriVm {
    script: ScScript,
    labels: BTreeMap<String, u32>,
    state: MinoriRuntimeState,
}

impl MinoriVm {
    pub fn new(
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
        session_seed: u64,
    ) -> Result<Self, MinoriRuntimeError> {
        let labels = build_labels(&script)?;
        let state = MinoriRuntimeState {
            schema: MINORI_RUNTIME_STATE_SCHEMA.into(),
            script_uri,
            script_hash,
            pc_line: 0,
            variables: BTreeMap::new(),
            global_variables: BTreeMap::new(),
            wait: None,
            message: None,
            choice: None,
            layers: BTreeMap::new(),
            transition: MinoriTransitionState::default(),
            effect: None,
            panel: None,
            audio: BTreeMap::new(),
            movie: None,
            system_ui: MinoriSystemUiState::default(),
            gallery_unlocks: Vec::new(),
            fixed_tick: 0,
            session_seed,
            random_state: session_seed,
            instruction_count: 0,
            effect_sequence: 0,
            terminal: false,
        };
        Ok(Self {
            script,
            labels,
            state,
        })
    }

    pub fn state(&self) -> &MinoriRuntimeState {
        &self.state
    }

    pub fn state_hash(&self) -> Result<Hash256, MinoriRuntimeError> {
        let bytes = postcard::to_allocvec(&self.state).map_err(|_| MinoriRuntimeError::Snapshot)?;
        Ok(Hash256::from_sha256(&bytes))
    }

    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, MinoriRuntimeError> {
        postcard::to_allocvec(&self.state).map_err(|_| MinoriRuntimeError::Snapshot)
    }

    pub fn decode_snapshot(bytes: &[u8]) -> Result<MinoriRuntimeState, MinoriRuntimeError> {
        let state: MinoriRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MinoriRuntimeError::Snapshot)?;
        if state.schema != MINORI_RUNTIME_STATE_SCHEMA {
            return Err(MinoriRuntimeError::State);
        }
        Ok(state)
    }

    pub fn replace_script(
        &mut self,
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
    ) -> Result<(), MinoriRuntimeError> {
        let labels = build_labels(&script)?;
        self.script = script;
        self.labels = labels;
        self.state.script_uri = script_uri;
        self.state.script_hash = script_hash;
        self.state.pc_line = 0;
        self.state.variables.clear();
        self.state.wait = None;
        self.state.message = None;
        self.state.choice = None;
        self.state.terminal = false;
        Ok(())
    }

    pub fn restore_state(&mut self, bytes: &[u8]) -> Result<(), MinoriRuntimeError> {
        let restored: MinoriRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MinoriRuntimeError::Snapshot)?;
        if restored.schema != MINORI_RUNTIME_STATE_SCHEMA
            || restored.script_uri != self.state.script_uri
            || restored.script_hash != self.state.script_hash
            || restored.session_seed != self.state.session_seed
            || restored.pc_line as usize > self.script.lines.len()
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state = restored;
        Ok(())
    }

    pub fn resolve_wait(&mut self, token_id: &str) -> Result<(), MinoriRuntimeError> {
        let current = self
            .state
            .wait
            .as_ref()
            .ok_or(MinoriRuntimeError::Waiting)?;
        let expected = match current {
            MinoriWaitState::Time { token_id, .. }
            | MinoriWaitState::Input { token_id }
            | MinoriWaitState::Media { token_id, .. }
            | MinoriWaitState::Presentation { token_id, .. }
            | MinoriWaitState::Provider { token_id, .. } => token_id,
        };
        if expected != token_id {
            return Err(MinoriRuntimeError::Waiting);
        }
        self.state.wait = None;
        Ok(())
    }

    pub fn advance_waiting_tick(&mut self, fixed_tick: u64) -> Result<(), MinoriRuntimeError> {
        if self.state.wait.is_none() || fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MinoriRuntimeError::State);
        }
        self.state.fixed_tick = fixed_tick;
        Ok(())
    }

    pub fn advance_effect_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriEffectFrame>, MinoriRuntimeError> {
        let Some(effect) = self.state.effect.as_mut() else {
            return Ok(None);
        };
        effect.elapsed_ns = effect
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let interval_ns = u64::from(effect.interval_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        if effect.elapsed_ns < interval_ns {
            return Ok(None);
        }
        // The original handler performs at most one update per render call and
        // resets its time origin to the current clock value.
        effect.elapsed_ns = 0;
        if effect.alpha_255 >= 255 {
            effect.current_index = effect.next_index;
            effect.next_index = next_effect_index(effect.next_index, effect.resources.len())?;
            effect.alpha_255 = 0;
        }
        let mut frame = effect_frame(effect)?;
        effect.visible_current_index = effect.current_index;
        effect.visible_next_index = effect.next_index;
        effect.visible_alpha_255 = frame.alpha_255;
        effect.alpha_255 = effect
            .alpha_255
            .checked_add(effect.alpha_step)
            .ok_or(MinoriRuntimeError::Overflow)?;
        next_effect_sequence(&mut self.state)?;
        frame.sequence = self.state.effect_sequence;
        Ok(Some(frame))
    }

    pub fn step(
        &mut self,
        fixed_tick: u64,
        max_instructions: u32,
    ) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
        if fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MinoriRuntimeError::State);
        }
        if self.state.wait.is_some() {
            return Err(MinoriRuntimeError::Waiting);
        }
        if self.state.terminal {
            return Ok(Some(MinoriVmEvent::Terminal));
        }
        self.state.fixed_tick = fixed_tick;
        for _ in 0..max_instructions {
            let line_index = self.state.pc_line as usize;
            let line = self
                .script
                .lines
                .get(line_index)
                .ok_or(MinoriRuntimeError::ProgramCounter)?;
            self.state.pc_line = self
                .state
                .pc_line
                .checked_add(1)
                .ok_or(MinoriRuntimeError::Overflow)?;
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            self.state.instruction_count = self
                .state
                .instruction_count
                .checked_add(1)
                .ok_or(MinoriRuntimeError::Overflow)?;
            if let Some(event) = execute_control(command, &self.labels, &mut self.state)? {
                return Ok(Some(event));
            }
        }
        Err(MinoriRuntimeError::Budget)
    }
}

fn execute_control(
    command: &ScCommand,
    labels: &BTreeMap<String, u32>,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    match command.opcode.as_str() {
        "pragma" | "label" => Ok(None),
        "set" | "setglobal" => {
            let (key, value) = evaluate_assignment(&command.operands, state)?;
            if command.opcode == "set" {
                state.variables.insert(key.to_owned(), value);
            } else {
                state.global_variables.insert(key.to_owned(), value);
            }
            Ok(None)
        }
        "goto" => {
            let target = branch_target(&command.control_flow)?;
            state.pc_line = *labels.get(target).ok_or(MinoriRuntimeError::Label)?;
            Ok(None)
        }
        "if" => {
            let [ScOperand::Symbol { value: key }, ScOperand::Operator { value: operator }, ScOperand::Integer { value }, ScOperand::Symbol { value: target }] =
                command.operands.as_slice()
            else {
                return Err(MinoriRuntimeError::Operand);
            };
            let current = state
                .variables
                .get(key)
                .or_else(|| state.global_variables.get(key))
                .copied()
                .unwrap_or(0);
            if compare(current, operator, *value)? {
                state.pc_line = *labels.get(target).ok_or(MinoriRuntimeError::Label)?;
            }
            Ok(None)
        }
        "wait" => {
            let [ScOperand::Integer { value }] = command.operands.as_slice() else {
                return Err(MinoriRuntimeError::Operand);
            };
            let timer_ticks = u32::try_from(*value).map_err(|_| MinoriRuntimeError::Operand)?;
            let milliseconds = timer_ticks
                .checked_mul(10)
                .ok_or(MinoriRuntimeError::Overflow)?;
            let token_id = format!("minori.wait.{}", state.instruction_count);
            let wait = MinoriWaitState::Time {
                token_id,
                timer_ticks,
                milliseconds,
            };
            state.wait = Some(wait.clone());
            Ok(Some(MinoriVmEvent::Wait(wait)))
        }
        "message" => execute_message(command, state),
        "playbgm" => execute_play_bgm(command, state),
        "playse" => execute_play_se(command, state, 1, "se"),
        "playse2" => execute_play_se(command, state, 2, "se2"),
        "playse3" => execute_play_se(command, state, 3, "se3"),
        "playvoice" => execute_play_voice(command, state),
        "transition" => execute_transition(command, state),
        "stage" => execute_stage(command, state),
        "effect" => execute_effect(command, state),
        "panel" => execute_panel(command, state),
        "chain" => {
            let ScControlFlow::Chain { target } = &command.control_flow else {
                return Err(MinoriRuntimeError::Operand);
            };
            validate_chain_target(target)?;
            Ok(Some(MinoriVmEvent::Chain {
                target: target.clone(),
            }))
        }
        "end" => {
            state.terminal = true;
            Ok(Some(MinoriVmEvent::Terminal))
        }
        _ => Err(MinoriRuntimeError::UnsupportedOpcode {
            opcode: command.opcode.clone(),
            ordinal: command.ordinal,
        }),
    }
}

fn execute_effect(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Effect)?;
    if tokens.len() < 2 || tokens.len() > 5 || tokens[0] != "CrossFade2" {
        return Err(MinoriRuntimeError::Effect);
    }
    let resources = tokens[1]
        .split(':')
        .map(|resource| {
            if resource == "*" {
                Ok(None)
            } else {
                validate_scene_filename(resource)?;
                Ok(Some(format!("minori:/bg/{resource}")))
            }
        })
        .collect::<Result<Vec<_>, MinoriRuntimeError>>()?;
    if resources.len() < 2 || resources.len() > 64 || resources.iter().all(Option::is_none) {
        return Err(MinoriRuntimeError::Effect);
    }
    let alpha_step = parse_effect_integer(tokens.get(2), -1)?;
    let interval_ms = parse_effect_integer(tokens.get(3), -1)?;
    let unused = parse_effect_integer(tokens.get(4), -1)?;
    if alpha_step <= 0 || interval_ms <= 0 || unused != -1 {
        return Err(MinoriRuntimeError::Effect);
    }
    let mut effect = MinoriEffectState {
        kind: MinoriEffectKind::CrossFade2,
        resources,
        current_index: 0,
        next_index: 1,
        alpha_255: 0,
        alpha_step: u32::try_from(alpha_step).map_err(|_| MinoriRuntimeError::Effect)?,
        interval_ms: u32::try_from(interval_ms).map_err(|_| MinoriRuntimeError::Effect)?,
        elapsed_ns: 0,
        visible_current_index: 0,
        visible_next_index: 1,
        visible_alpha_255: 0,
    };
    // Creation immediately performs the first zero-alpha update in the
    // original effect object, then advances the accumulator.
    effect.alpha_255 = effect.alpha_step;
    state.effect = Some(effect);
    next_effect_sequence(state)?;
    let mut frame = effect_frame(state.effect.as_ref().ok_or(MinoriRuntimeError::Effect)?)?;
    frame.alpha_255 = 0;
    frame.sequence = state.effect_sequence;
    Ok(Some(MinoriVmEvent::Effect(frame)))
}

fn execute_panel(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Panel)?;
    let [mode] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::Panel);
    };
    let mode = mode.parse::<u32>().map_err(|_| MinoriRuntimeError::Panel)?;
    // The original CMessagePanel switch maps mode 1 to the default
    // `msgPanel.png` resource. Other modes, the secondary transition operand,
    // and filename overrides remain blocked until their behavior is verified.
    if mode != 1 {
        return Err(MinoriRuntimeError::Panel);
    }
    state.panel = Some(MinoriPanelState {
        mode,
        resource_uri: "minori:/sys/msgPanel.png".into(),
    });
    next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::Panel {
        sequence: state.effect_sequence,
    }))
}

fn parse_effect_integer(token: Option<&String>, default: i32) -> Result<i32, MinoriRuntimeError> {
    token.map_or(Ok(default), |value| {
        value.parse().map_err(|_| MinoriRuntimeError::Effect)
    })
}

fn next_effect_index(current: u32, len: usize) -> Result<u32, MinoriRuntimeError> {
    let len = u32::try_from(len).map_err(|_| MinoriRuntimeError::Overflow)?;
    current
        .checked_add(1)
        .map(|next| next % len)
        .ok_or(MinoriRuntimeError::Overflow)
}

fn effect_frame(effect: &MinoriEffectState) -> Result<MinoriEffectFrame, MinoriRuntimeError> {
    let current = usize::try_from(effect.current_index).map_err(|_| MinoriRuntimeError::Effect)?;
    let next = usize::try_from(effect.next_index).map_err(|_| MinoriRuntimeError::Effect)?;
    Ok(MinoriEffectFrame {
        sequence: 0,
        current_resource_uri: effect
            .resources
            .get(current)
            .ok_or(MinoriRuntimeError::Effect)?
            .clone(),
        next_resource_uri: effect
            .resources
            .get(next)
            .ok_or(MinoriRuntimeError::Effect)?
            .clone(),
        alpha_255: effect.alpha_255.min(255) as u16,
    })
}

fn execute_transition(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let [mode, resource, duration] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::Operand);
    };
    let mode = mode
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let duration_ticks = duration
        .parse::<u32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let resource = if resource == "*" {
        None
    } else {
        validate_scene_filename(resource)?;
        Some(resource.clone())
    };
    state.transition = MinoriTransitionState {
        mode,
        resource,
        duration_ticks,
    };
    Ok(None)
}

fn execute_stage(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    const BACKGROUND_LAYER: u32 = 0;
    const FOREGROUND_LAYER: u32 = 1;
    const STAND_LAYER_BASE: u32 = 16;

    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.len() < 4 || tokens.len() > 26 {
        return Err(MinoriRuntimeError::Operand);
    }
    let foreground_name = &tokens[0];
    let mut cursor = 1usize;
    let mut foreground_position = (0, 0);
    if tokens.len() >= 6 && !tokens[1].contains('.') && !tokens[2].contains('.') {
        if let (Ok(x), Ok(y)) = (tokens[1].parse::<i32>(), tokens[2].parse::<i32>()) {
            foreground_position = (x, y);
            cursor = 3;
        }
    }
    if cursor + 3 > tokens.len() || !(tokens.len() - (cursor + 3)).is_multiple_of(2) {
        return Err(MinoriRuntimeError::Operand);
    }
    let background_name = &tokens[cursor];
    let background_x = tokens[cursor + 1]
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let background_y = tokens[cursor + 2]
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    cursor += 3;

    let foreground = stage_layer(
        "bg",
        foreground_name,
        foreground_position.0,
        foreground_position.1,
    )?;
    let background = stage_layer("bg", background_name, background_x, background_y)?;
    let mut stands = Vec::with_capacity((tokens.len() - cursor) / 2);
    while cursor < tokens.len() {
        let (filename, offset) = tokens[cursor]
            .split_once(',')
            .map_or((tokens[cursor].as_str(), "0"), |(filename, offset)| {
                (filename, offset)
            });
        validate_scene_filename(filename)?;
        let offset = offset
            .parse::<i32>()
            .map_err(|_| MinoriRuntimeError::Operand)?;
        let position = tokens[cursor + 1]
            .parse::<i32>()
            .map_err(|_| MinoriRuntimeError::Operand)?;
        stands.push(MinoriStandLayer {
            resource_uri: format!("minori:/st/{filename}"),
            position,
            offset,
        });
        cursor += 2;
    }
    if stands.len() > 10 {
        return Err(MinoriRuntimeError::Operand);
    }

    state.layers.clear();
    if let Some(layer) = &background {
        state
            .layers
            .insert(BACKGROUND_LAYER, stage_layer_state(layer, "alpha"));
    }
    if let Some(layer) = &foreground {
        state
            .layers
            .insert(FOREGROUND_LAYER, stage_layer_state(layer, "alpha"));
    }
    for (index, stand) in stands.iter().enumerate() {
        let layer_id = STAND_LAYER_BASE
            .checked_add(u32::try_from(index).map_err(|_| MinoriRuntimeError::Overflow)?)
            .ok_or(MinoriRuntimeError::Overflow)?;
        state.layers.insert(
            layer_id,
            MinoriLayerState {
                resource_uri: stand.resource_uri.clone(),
                content_hash: None,
                // The original passes position and offset as separate stage parameters;
                // they are not pixel coordinates and remain on the typed stage event.
                x_milli: 0,
                y_milli: 0,
                scale_x_milli: 1000,
                scale_y_milli: 1000,
                opacity_milli: 1000,
                blend: "alpha".into(),
            },
        );
    }
    next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::Stage(MinoriStageCommand {
        foreground,
        background,
        stands,
        transition: state.transition.clone(),
    })))
}

fn stage_layer(
    role: &str,
    filename: &str,
    x: i32,
    y: i32,
) -> Result<Option<MinoriStageLayer>, MinoriRuntimeError> {
    if filename == "*" {
        return Ok(None);
    }
    validate_scene_filename(filename)?;
    Ok(Some(MinoriStageLayer {
        resource_uri: format!("minori:/{role}/{filename}"),
        x,
        y,
    }))
}

fn stage_layer_state(layer: &MinoriStageLayer, blend: &str) -> MinoriLayerState {
    MinoriLayerState {
        resource_uri: layer.resource_uri.clone(),
        content_hash: None,
        x_milli: layer.x.saturating_mul(1000),
        y_milli: layer.y.saturating_mul(1000),
        scale_x_milli: 1000,
        scale_y_milli: 1000,
        opacity_milli: 1000,
        blend: blend.into(),
    }
}

fn validate_scene_filename(value: &str) -> Result<(), MinoriRuntimeError> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value.contains(['/', '\\', ':', '\0'])
        || value == "."
        || value == ".."
    {
        return Err(MinoriRuntimeError::Operand);
    }
    Ok(())
}

fn execute_play_se(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
    loop_stream_id: u32,
    bus: &str,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MinoriRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource == "*" {
        let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
        return stop_audio_stream(state, loop_stream_id, fade_out_ms);
    }
    validate_audio_relative_path(&spec.resource)?;
    let repeat = tokens
        .get(1)
        .and_then(|token| token.as_bytes().first())
        .is_some_and(|byte| *byte == b't');
    let fade_in_ms = parse_optional_command_integer(tokens.get(2), 2, 2)?;
    let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
    let volume_milli = spec.volume_percent * 10;
    let pan_milli = spec.pan_percent * 10;
    let resource_uri = format!("minori:/se/{}", spec.resource);
    let stream_id = if repeat {
        loop_stream_id
    } else {
        let ordinal =
            u32::try_from(state.instruction_count).map_err(|_| MinoriRuntimeError::Overflow)?;
        0x1000_0000u32
            .checked_add(
                ordinal
                    .checked_mul(4)
                    .and_then(|value| value.checked_add(loop_stream_id))
                    .ok_or(MinoriRuntimeError::Overflow)?,
            )
            .ok_or(MinoriRuntimeError::Overflow)?
    };
    let mut commands = Vec::new();
    if repeat {
        match state.audio.get(&loop_stream_id).cloned() {
            Some(current) if current.playing && current.resource_uri == resource_uri => {
                if current.volume_milli != volume_milli || current.pan_milli != pan_milli {
                    commands.push(MinoriAudioCommand::SetParams {
                        sequence: next_effect_sequence(state)?,
                        stream_id,
                        volume: f32::from(volume_milli) / 1000.0,
                        pan: f32::from(pan_milli) / 1000.0,
                        repeat,
                    });
                }
            }
            Some(current) if current.playing => {
                commands.push(MinoriAudioCommand::Stop {
                    sequence: next_effect_sequence(state)?,
                    stream_id,
                    fade_ms: fade_out_ms,
                });
                append_audio_load_and_play(
                    state,
                    &mut commands,
                    stream_id,
                    &resource_uri,
                    volume_milli,
                    pan_milli,
                    repeat,
                    fade_in_ms,
                )?;
            }
            _ => append_audio_load_and_play(
                state,
                &mut commands,
                stream_id,
                &resource_uri,
                volume_milli,
                pan_milli,
                repeat,
                fade_in_ms,
            )?,
        }
    } else {
        append_audio_load_and_play(
            state,
            &mut commands,
            stream_id,
            &resource_uri,
            volume_milli,
            pan_milli,
            repeat,
            fade_in_ms,
        )?;
    }
    state.audio.insert(
        stream_id,
        MinoriAudioState {
            bus: bus.into(),
            resource_uri,
            looped: repeat,
            volume_milli,
            pan_milli,
            playing: true,
            continuation_pts: 0,
        },
    );
    Ok(Some(MinoriVmEvent::Audio { commands }))
}

#[allow(clippy::too_many_arguments)]
fn append_audio_load_and_play(
    state: &mut MinoriRuntimeState,
    commands: &mut Vec<MinoriAudioCommand>,
    stream_id: u32,
    resource_uri: &str,
    volume_milli: u16,
    pan_milli: i16,
    repeat: bool,
    fade_in_ms: u32,
) -> Result<(), MinoriRuntimeError> {
    commands.push(MinoriAudioCommand::LoadResource {
        sequence: next_effect_sequence(state)?,
        stream_id,
        resource_uri: resource_uri.into(),
    });
    commands.push(MinoriAudioCommand::Play {
        sequence: next_effect_sequence(state)?,
        stream_id,
        volume: f32::from(volume_milli) / 1000.0,
        pan: f32::from(pan_milli) / 1000.0,
        repeat,
        fade_in_ms,
    });
    Ok(())
}

fn execute_play_bgm(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    const BGM_STREAM_ID: u32 = 0;
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MinoriRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource == "*" {
        let fade_out_ms = parse_optional_command_integer(tokens.get(2), 1, 2)?;
        return stop_audio_stream(state, BGM_STREAM_ID, fade_out_ms);
    }
    validate_audio_relative_path(&spec.resource)?;
    let fade_in_ms = parse_optional_command_integer(tokens.get(1), 1, 2)?;
    let fade_out_ms = parse_optional_command_integer(tokens.get(2), 1, 2)?;
    let command_volume = parse_optional_command_integer(tokens.get(3), 100, 100)?;
    if !(0..=400).contains(&command_volume) {
        return Err(MinoriRuntimeError::Operand);
    }
    let volume_milli =
        u16::try_from(i64::from(command_volume) * i64::from(spec.volume_percent) * 1000 / 10_000)
            .map_err(|_| MinoriRuntimeError::Overflow)?;
    let pan_milli = spec.pan_percent * 10;
    let resource_uri = format!("minori:/bgm/{}", spec.resource);
    let mut commands = Vec::new();
    match state.audio.get(&BGM_STREAM_ID).cloned() {
        Some(current) if current.playing && current.resource_uri == resource_uri => {
            if current.volume_milli != volume_milli || current.pan_milli != pan_milli {
                commands.push(MinoriAudioCommand::SetParams {
                    sequence: next_effect_sequence(state)?,
                    stream_id: BGM_STREAM_ID,
                    volume: f32::from(volume_milli) / 1000.0,
                    pan: f32::from(pan_milli) / 1000.0,
                    repeat: true,
                });
            }
        }
        Some(current) if current.playing => {
            commands.push(MinoriAudioCommand::Stop {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                fade_ms: fade_out_ms,
            });
            commands.push(MinoriAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                resource_uri: resource_uri.clone(),
            });
            commands.push(MinoriAudioCommand::Play {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                volume: f32::from(volume_milli) / 1000.0,
                pan: f32::from(pan_milli) / 1000.0,
                repeat: true,
                fade_in_ms,
            });
        }
        _ => {
            commands.push(MinoriAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                resource_uri: resource_uri.clone(),
            });
            commands.push(MinoriAudioCommand::Play {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                volume: f32::from(volume_milli) / 1000.0,
                pan: f32::from(pan_milli) / 1000.0,
                repeat: true,
                fade_in_ms,
            });
        }
    }
    state.audio.insert(
        BGM_STREAM_ID,
        MinoriAudioState {
            bus: "bgm".into(),
            resource_uri,
            looped: true,
            volume_milli,
            pan_milli,
            playing: true,
            continuation_pts: 0,
        },
    );
    Ok(Some(MinoriVmEvent::Audio { commands }))
}

fn execute_play_voice(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    const VOICE_STREAM_ID: u32 = 4;
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MinoriRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource != "*" {
        // The authorized script census only contains the verified stop form.
        // Message-bound voice lookup is a separate command path and remains
        // fail-closed until its archive key mapping is proven.
        return Err(MinoriRuntimeError::UnsupportedOpcode {
            opcode: "playvoice.resource".into(),
            ordinal: command.ordinal,
        });
    }
    let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
    stop_audio_stream(state, VOICE_STREAM_ID, fade_out_ms)
}

fn stop_audio_stream(
    state: &mut MinoriRuntimeState,
    stream_id: u32,
    fade_ms: u32,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let mut commands = Vec::new();
    if state
        .audio
        .get(&stream_id)
        .is_some_and(|current| current.playing)
    {
        commands.push(MinoriAudioCommand::Stop {
            sequence: next_effect_sequence(state)?,
            stream_id,
            fade_ms,
        });
        if let Some(current) = state.audio.get_mut(&stream_id) {
            current.playing = false;
        }
    }
    Ok(Some(MinoriVmEvent::Audio { commands }))
}

fn parse_optional_command_integer(
    token: Option<&String>,
    missing: i32,
    malformed: i32,
) -> Result<u32, MinoriRuntimeError> {
    let value = token
        .map(|token| parse_c_decimal_prefix(token).unwrap_or(malformed))
        .unwrap_or(missing);
    u32::try_from(value).map_err(|_| MinoriRuntimeError::Operand)
}

fn validate_audio_relative_path(value: &str) -> Result<(), MinoriRuntimeError> {
    if value.len() > 256
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
    {
        return Err(MinoriRuntimeError::AudioResource);
    }
    Ok(())
}

fn next_effect_sequence(state: &mut MinoriRuntimeState) -> Result<u64, MinoriRuntimeError> {
    state.effect_sequence = state
        .effect_sequence
        .checked_add(1)
        .ok_or(MinoriRuntimeError::Overflow)?;
    Ok(state.effect_sequence)
}

fn execute_message(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let (message_id, voice, speaker, text) = if tokens.len() >= 4 {
        let message_id = tokens[0]
            .parse::<i64>()
            .map_err(|_| MinoriRuntimeError::Operand)?;
        (
            message_id,
            (!tokens[1].is_empty()).then(|| tokens[1].clone()),
            (!tokens[2].is_empty()).then(|| tokens[2].clone()),
            tokens[3..].join(" "),
        )
    } else {
        // The original CommandMessage parser leaves constructor defaults intact when fewer
        // than four operands are present, then still executes the empty message update.
        (-1, None, None, String::new())
    };
    let text_hash = Hash256::from_sha256(text.as_bytes());
    let speaker_hash = speaker
        .as_ref()
        .map(|value| Hash256::from_sha256(value.as_bytes()));
    state.message = Some(MinoriMessageState {
        source: command.span,
        message_id,
        text_hash,
        speaker_hash,
        // The voice archive/path mapping has not yet been verified, so only its identity enters
        // deterministic state and no resource URI is guessed.
        voice_hash: voice.map(|value| Hash256::from_sha256(value.as_bytes())),
    });
    next_effect_sequence(state)?;
    let wait = MinoriWaitState::Input {
        token_id: format!("minori.message.{}", state.instruction_count),
    };
    state.wait = Some(wait.clone());
    Ok(Some(MinoriVmEvent::Message {
        text,
        speaker,
        wait,
    }))
}

fn validate_chain_target(target: &str) -> Result<(), MinoriRuntimeError> {
    if target.is_empty()
        || target.len() > 256
        || !target.to_ascii_lowercase().ends_with(".sc")
        || target.contains('/')
        || target.contains('\\')
        || target == "."
        || target.contains("..")
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(MinoriRuntimeError::ChainTarget);
    }
    Ok(())
}

fn evaluate_assignment<'a>(
    operands: &'a [ScOperand],
    state: &MinoriRuntimeState,
) -> Result<(&'a str, i64), MinoriRuntimeError> {
    match operands {
        [ScOperand::Symbol { value: key }, ScOperand::Operator { value: assign }, rhs]
            if assign == "=" =>
        {
            Ok((key, resolve_integer(rhs, state)?))
        }
        [ScOperand::Symbol { value: key }, ScOperand::Operator { value: assign }, left, ScOperand::Operator { value: operator }, right]
            if assign == "=" =>
        {
            let left = resolve_integer(left, state)?;
            let right = resolve_integer(right, state)?;
            let value = match operator.as_str() {
                "|" => left | right,
                "&" => left & right,
                "+" => left
                    .checked_add(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "-" => left
                    .checked_sub(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "*" => left
                    .checked_mul(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "/" if right != 0 => left
                    .checked_div(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "%" if right != 0 => left
                    .checked_rem(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                _ => return Err(MinoriRuntimeError::Operand),
            };
            Ok((key, value))
        }
        _ => Err(MinoriRuntimeError::Operand),
    }
}

fn resolve_integer(
    operand: &ScOperand,
    state: &MinoriRuntimeState,
) -> Result<i64, MinoriRuntimeError> {
    match operand {
        ScOperand::Integer { value } => Ok(*value),
        ScOperand::Symbol { value } => Ok(state
            .variables
            .get(value)
            .or_else(|| state.global_variables.get(value))
            .copied()
            .unwrap_or(0)),
        _ => Err(MinoriRuntimeError::Operand),
    }
}

fn compare(left: i64, operator: &str, right: i64) -> Result<bool, MinoriRuntimeError> {
    Ok(match operator {
        "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        _ => return Err(MinoriRuntimeError::Operand),
    })
}

fn branch_target(control_flow: &ScControlFlow) -> Result<&str, MinoriRuntimeError> {
    match control_flow {
        ScControlFlow::Jump { target } | ScControlFlow::ConditionalJump { target } => Ok(target),
        _ => Err(MinoriRuntimeError::Operand),
    }
}

fn build_labels(script: &ScScript) -> Result<BTreeMap<String, u32>, MinoriRuntimeError> {
    let mut labels = BTreeMap::new();
    for (line_index, line) in script.lines.iter().enumerate() {
        let ScLineKind::Command { command } = &line.kind else {
            continue;
        };
        if let ScControlFlow::Label { id } = &command.control_flow {
            let target = u32::try_from(line_index + 1).map_err(|_| MinoriRuntimeError::Overflow)?;
            if labels.insert(id.clone(), target).is_some() {
                return Err(MinoriRuntimeError::Label);
            }
        }
    }
    Ok(labels)
}

#[cfg(test)]
mod tests {
    use crate::{parse_sc, ScOpcodeCatalog};

    use super::*;

    #[test]
    fn deterministic_control_flow_wait_and_restore_round_trip() {
        let source = b".setglobal route = 1\r\n.label loop\r\n.set count = count + 1\r\n.if count < 3 loop\r\n.wait 20\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            7,
        )
        .unwrap();
        let event = vm.step(1, 32).unwrap().unwrap();
        let MinoriVmEvent::Wait(MinoriWaitState::Time {
            token_id,
            timer_ticks,
            milliseconds,
        }) = event
        else {
            panic!("expected time wait")
        };
        assert_eq!(timer_ticks, 20);
        assert_eq!(milliseconds, 200);
        assert_eq!(vm.state().variables.get("count"), Some(&3));
        let snapshot = vm.snapshot_bytes().unwrap();
        let hash = vm.state_hash().unwrap();
        vm.resolve_wait(&token_id).unwrap();
        assert_eq!(vm.step(2, 4).unwrap(), Some(MinoriVmEvent::Terminal));
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state_hash().unwrap(), hash);
    }

    #[test]
    fn unsupported_presentation_command_blocks_without_advancing_silently() {
        let source = b".char 0\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap_err(),
            MinoriRuntimeError::UnsupportedOpcode {
                opcode: "char".into(),
                ordinal: 0,
            }
        );
    }

    #[test]
    fn panel_mode_one_uses_the_verified_message_panel_resource() {
        let source = b".panel 1\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Panel { sequence }) = vm.step(1, 4).unwrap() else {
            panic!("expected panel event")
        };
        assert_eq!(sequence, 1);
        assert_eq!(
            vm.state().panel,
            Some(MinoriPanelState {
                mode: 1,
                resource_uri: "minori:/sys/msgPanel.png".into(),
            })
        );
        let snapshot = vm.snapshot_bytes().unwrap();
        assert_eq!(
            MinoriVm::decode_snapshot(&snapshot).unwrap().panel,
            vm.state().panel
        );

        for source in [b".panel 0\r\n".as_slice(), b".panel 1 -1\r\n".as_slice()] {
            let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(source),
                script,
                1,
            )
            .unwrap();
            assert_eq!(vm.step(1, 4).unwrap_err(), MinoriRuntimeError::Panel);
        }
    }

    #[test]
    fn crossfade2_keeps_the_verified_resource_and_timeline_state() {
        let source = b".effect CrossFade2 first.png:second.png:*:* 320 100\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Effect(frame)) = vm.step(1, 4).unwrap() else {
            panic!("expected effect frame")
        };
        assert_eq!(
            frame.current_resource_uri.as_deref(),
            Some("minori:/bg/first.png")
        );
        assert_eq!(
            frame.next_resource_uri.as_deref(),
            Some("minori:/bg/second.png")
        );
        assert_eq!(frame.alpha_255, 0);
        let effect = vm.state().effect.as_ref().unwrap();
        assert_eq!(effect.alpha_step, 320);
        assert_eq!(effect.interval_ms, 100);
        assert_eq!(effect.resources.len(), 4);
        assert_eq!(effect.visible_current_index, 0);
        assert_eq!(effect.visible_next_index, 1);
        assert_eq!(effect.visible_alpha_255, 0);

        assert_eq!(vm.advance_effect_clock(99_000_000).unwrap(), None);
        let repeated = vm.advance_effect_clock(1_000_000).unwrap().unwrap();
        assert_eq!(
            repeated.current_resource_uri.as_deref(),
            Some("minori:/bg/second.png")
        );
        assert_eq!(repeated.next_resource_uri, None);
        assert_eq!(repeated.alpha_255, 0);
        let effect = vm.state().effect.as_ref().unwrap();
        assert_eq!(effect.visible_current_index, 1);
        assert_eq!(effect.visible_next_index, 2);
        assert_eq!(effect.visible_alpha_255, 0);
        let snapshot = vm.snapshot_bytes().unwrap();
        let restored = MinoriVm::decode_snapshot(&snapshot).unwrap();
        assert_eq!(restored.effect, vm.state().effect);
        assert_eq!(restored.effect_sequence, vm.state().effect_sequence);
    }

    #[test]
    fn crossfade2_rejects_unknown_modes_and_invalid_timeline_values() {
        for source in [
            b".effect CrossFade first.png:second.png 320 100\r\n".as_slice(),
            b".effect CrossFade2 first.png:second.png 0 100\r\n".as_slice(),
            b".effect CrossFade2 first.png:second.png 320 0\r\n".as_slice(),
            b".effect CrossFade2 first.png:second.png 320 100 1\r\n".as_slice(),
        ] {
            let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(source),
                script,
                1,
            )
            .unwrap();
            assert_eq!(vm.step(1, 4).unwrap_err(), MinoriRuntimeError::Effect);
        }
    }

    #[test]
    fn transition_configures_the_following_stage_without_guessing_star_as_a_resource() {
        let source = b".transition 0 * 10\r\n.stage * BLACK.png 0 0\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Stage(stage)) = vm.step(1, 4).unwrap() else {
            panic!("expected stage event")
        };
        assert_eq!(stage.foreground, None);
        assert_eq!(
            stage.background,
            Some(MinoriStageLayer {
                resource_uri: "minori:/bg/BLACK.png".into(),
                x: 0,
                y: 0,
            })
        );
        assert_eq!(stage.transition.mode, 0);
        assert_eq!(stage.transition.resource, None);
        assert_eq!(stage.transition.duration_ticks, 10);
    }

    #[test]
    fn message_uses_the_verified_four_operand_and_joined_tail_contract() {
        let source = b".message 42 voice speaker hello world\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message {
            text,
            speaker,
            wait: MinoriWaitState::Input { token_id },
        }) = vm.step(1, 4).unwrap()
        else {
            panic!("expected message input wait")
        };
        assert_eq!(text, "hello world");
        assert_eq!(speaker.as_deref(), Some("speaker"));
        assert_eq!(token_id, "minori.message.1");
        let state = vm.state().message.as_ref().unwrap();
        assert_eq!(state.message_id, 42);
        assert_eq!(state.text_hash, Hash256::from_sha256(b"hello world"));
        assert_eq!(state.voice_hash, Some(Hash256::from_sha256(b"voice")));
    }

    #[test]
    fn short_message_executes_the_observed_constructor_defaults() {
        let source = b".message 42 incomplete\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message { text, speaker, .. }) = vm.step(1, 1).unwrap() else {
            panic!("expected empty message update")
        };
        assert!(text.is_empty());
        assert!(speaker.is_none());
        assert_eq!(vm.state().message.as_ref().unwrap().message_id, -1);
    }

    #[test]
    fn audio_resource_suffix_matches_the_observed_volume_pan_contract() {
        assert_eq!(
            parse_audio_resource_spec("theme.ogg[75,-120]").unwrap(),
            MinoriAudioResourceSpec {
                resource: "theme.ogg".into(),
                volume_percent: 75,
                pan_percent: -100,
            }
        );
        assert_eq!(
            parse_audio_resource_spec("theme.ogg[120,25suffix]").unwrap(),
            MinoriAudioResourceSpec {
                resource: "theme.ogg".into(),
                volume_percent: 100,
                pan_percent: 25,
            }
        );
        assert_eq!(
            parse_audio_resource_spec("theme.ogg[broken").unwrap(),
            MinoriAudioResourceSpec {
                resource: "theme.ogg".into(),
                volume_percent: 100,
                pan_percent: 0,
            }
        );
        assert_eq!(
            parse_audio_resource_spec("[50,0]").unwrap_err(),
            MinoriRuntimeError::AudioResource
        );
    }

    #[test]
    fn play_bgm_emits_stable_uri_audio_commands_with_observed_defaults() {
        let source = b".playBGM theme.ogg[50,-25] * * 80\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Audio { commands }) = vm.step(1, 4).unwrap() else {
            panic!("expected BGM commands")
        };
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            MinoriAudioCommand::LoadResource {
                sequence: 1,
                stream_id: 0,
                resource_uri: "minori:/bgm/theme.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            MinoriAudioCommand::Play {
                sequence: 2,
                stream_id: 0,
                volume: 0.4,
                pan: -0.25,
                repeat: true,
                fade_in_ms: 2,
            }
        );
        let state = vm.state().audio.get(&0).unwrap();
        assert_eq!(state.volume_milli, 400);
        assert_eq!(state.pan_milli, -250);
    }

    #[test]
    fn audio_control_token_stops_the_bound_bus_with_fade_out() {
        let source = b".playBGM theme.ogg\r\n.playBGM * * 25\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert!(matches!(
            vm.step(1, 1).unwrap(),
            Some(MinoriVmEvent::Audio { .. })
        ));
        let Some(MinoriVmEvent::Audio { commands }) = vm.step(2, 1).unwrap() else {
            panic!("expected BGM stop command")
        };
        assert_eq!(
            commands,
            vec![MinoriAudioCommand::Stop {
                sequence: 3,
                stream_id: 0,
                fade_ms: 25,
            }]
        );
        assert!(!vm.state().audio.get(&0).unwrap().playing);
    }

    #[test]
    fn play_voice_control_token_is_a_bounded_noop_without_active_voice() {
        let source = b".playVoice * false 2 30\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 1).unwrap(),
            Some(MinoriVmEvent::Audio {
                commands: Vec::new()
            })
        );
    }

    #[test]
    fn play_se_preserves_repeat_bus_and_resource_metadata() {
        let source = b".playSE click.ogg[75,20] true * 30\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Audio { commands }) = vm.step(1, 4).unwrap() else {
            panic!("expected SE commands")
        };
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            MinoriAudioCommand::LoadResource {
                sequence: 1,
                stream_id: 1,
                resource_uri: "minori:/se/click.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            MinoriAudioCommand::Play {
                sequence: 2,
                stream_id: 1,
                volume: 0.75,
                pan: 0.2,
                repeat: true,
                fade_in_ms: 2,
            }
        );
        let state = vm.state().audio.get(&1).unwrap();
        assert_eq!(state.bus, "se");
        assert!(state.looped);
    }

    #[test]
    fn chain_is_a_bounded_tail_transfer_without_a_return_frame() {
        let source = b".set local = 1\r\n.chain K01.sc\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::Chain {
                target: "K01.sc".into()
            })
        );

        let next = b".end\r\n";
        vm.replace_script(
            "minori:/scr/K01.sc".into(),
            Hash256::from_sha256(next),
            parse_sc(next, &ScOpcodeCatalog::observed_minori()).unwrap(),
        )
        .unwrap();
        assert!(vm.state().variables.is_empty());
        assert_eq!(vm.step(2, 2).unwrap(), Some(MinoriVmEvent::Terminal));
    }

    #[test]
    fn chain_rejects_path_escape() {
        let source = b".chain ../outside.sc\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 1).unwrap_err(), MinoriRuntimeError::ChainTarget);
    }

    #[test]
    fn assignment_uses_verified_three_and_five_token_forms() {
        let source = b".set base = 6\r\n.set sum = base + 4\r\n.set bits = sum | 1\r\n.set rem = sum % 4\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 8).unwrap(), Some(MinoriVmEvent::Terminal));
        assert_eq!(vm.state().variables.get("sum"), Some(&10));
        assert_eq!(vm.state().variables.get("bits"), Some(&11));
        assert_eq!(vm.state().variables.get("rem"), Some(&2));

        let unsupported = b".set count += 1\r\n";
        let script = parse_sc(unsupported, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(unsupported),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 1).unwrap_err(), MinoriRuntimeError::Operand);
    }
}
