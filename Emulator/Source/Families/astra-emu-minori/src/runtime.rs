mod effects;
mod errors;
mod model;
use effects::*;
pub use errors::MinoriRuntimeError;
pub use model::*;
mod audio_commands;
mod choices;
use audio_commands::*;
use choices::execute_select;
use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;

use crate::{script::tokenize_operands, ScCommand, ScControlFlow, ScLineKind, ScOperand, ScScript};

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

    pub fn encode_native_save(&self) -> Result<Vec<u8>, MinoriRuntimeError> {
        postcard::to_allocvec(&self.state).map_err(|_| MinoriRuntimeError::NativeSaveFormat)
    }

    pub fn decode_native_save(bytes: &[u8]) -> Result<MinoriRuntimeState, MinoriRuntimeError> {
        let state: MinoriRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MinoriRuntimeError::NativeSaveFormat)?;
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
        label: Option<&str>,
    ) -> Result<(), MinoriRuntimeError> {
        let labels = build_labels(&script)?;
        let pc = match label {
            Some(label) => *labels.get(label).ok_or(MinoriRuntimeError::Label)?,
            None => 0,
        };
        self.script = script;
        self.labels = labels;
        self.state.script_uri = script_uri;
        self.state.script_hash = script_hash;
        self.state.pc_line = pc;
        self.state.variables.clear();
        self.state.wait = None;
        self.state.message = None;
        self.state.choice = None;
        self.state.terminal = false;
        Ok(())
    }

    pub fn restore_native_save(
        &mut self,
        bytes: &[u8],
        next_fixed_tick: u64,
    ) -> Result<(), MinoriRuntimeError> {
        let restored: MinoriRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MinoriRuntimeError::NativeSaveFormat)?;
        if restored.schema != MINORI_RUNTIME_STATE_SCHEMA
            || restored.script_uri != self.state.script_uri
            || restored.script_hash != self.state.script_hash
            || restored.session_seed != self.state.session_seed
            || restored.pc_line as usize > self.script.lines.len()
        {
            return Err(MinoriRuntimeError::State);
        }
        choices::validate_choice(&self.script, &restored)?;
        let restored_tick = next_fixed_tick
            .checked_sub(1)
            .ok_or(MinoriRuntimeError::State)?;
        self.state = restored;
        self.state.fixed_tick = restored_tick;
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

    pub fn step(&mut self, fixed_tick: u64) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
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
        let mut visited = BTreeSet::new();
        let mut operations = 0u32;
        loop {
            operations += 1;
            if operations > 10_000 {
                return Err(MinoriRuntimeError::NonYieldingCycle);
            }
            let control_state = (
                self.state.pc_line,
                self.state.variables.clone(),
                self.state.global_variables.clone(),
            );
            if !visited.insert(control_state) {
                return Err(MinoriRuntimeError::NonYieldingCycle);
            }
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
            let event =
                execute_control(command, &self.labels, &mut self.state).inspect_err(|cause| {
                    tracing::error!(
                        event = "astra.emu.minori.command.failed",
                        code = cause.diagnostic_code(),
                        ordinal = command.ordinal,
                        line = line_index,
                        offset = command.span.offset,
                        operand_count = command.operands.len(),
                        script_hash = %self.state.script_hash,
                        tick = fixed_tick,
                    );
                })?;
            if let Some(event) = event {
                return Ok(Some(event));
            }
        }
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
        "deletevar" => {
            let [ScOperand::Symbol { value: key }] = command.operands.as_slice() else {
                return Err(MinoriRuntimeError::Operand);
            };
            state.variables.remove(key);
            state.global_variables.remove(key);
            Ok(None)
        }
        "playbgm" => execute_play_bgm(command, state, 0, "bgm"),
        "select" => execute_select(command, &mut *state),
        "playbgm2" => execute_play_bgm(command, state, 5, "bgm2"),
        "playse" => execute_play_se(command, state, 1, "se"),
        "playse2" => execute_play_se(command, state, 2, "se2"),
        "playse3" => execute_play_se(command, state, 3, "se3"),
        "playse4" => execute_play_se(command, state, 6, "se4"),
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

fn execute_message(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let (message_id, speaker, text) = if tokens.len() >= 4 {
        let message_id = tokens[0]
            .parse::<i64>()
            .map_err(|_| MinoriRuntimeError::Operand)?;
        (
            message_id,
            (!tokens[2].is_empty()).then(|| tokens[2].clone()),
            tokens[3..].join(" "),
        )
    } else {
        // The original CommandMessage parser leaves constructor defaults intact when fewer
        // than four operands are present, then still executes the empty message update.
        (-1, None, String::new())
    };
    state.message = Some(MinoriMessageState {
        source: command.span,
        message_id,
    });
    let presentation_sequence = next_effect_sequence(state)?;
    let capture_sequence = next_effect_sequence(state)?;
    let wait = MinoriWaitState::Input {
        token_id: format!("minori.message.{}", state.instruction_count),
    };
    state.wait = Some(wait.clone());
    Ok(Some(MinoriVmEvent::Message {
        presentation_sequence,
        capture_sequence,
        text,
        speaker,
        wait,
    }))
}

fn validate_chain_target(target: &str) -> Result<(), MinoriRuntimeError> {
    crate::script::chain_target_parts(target)
        .map(|_| ())
        .ok_or(MinoriRuntimeError::ChainTarget)
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
#[path = "runtime/tests.rs"]
mod tests;

fn next_effect_sequence(state: &mut MinoriRuntimeState) -> Result<u64, MinoriRuntimeError> {
    state.effect_sequence = state
        .effect_sequence
        .checked_add(1)
        .ok_or(MinoriRuntimeError::Overflow)?;
    Ok(state.effect_sequence)
}
