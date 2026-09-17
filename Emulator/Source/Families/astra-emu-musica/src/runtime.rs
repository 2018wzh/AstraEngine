mod backlog;
mod config;
mod control;
mod effects;
mod errors;
pub(crate) mod gallery;
mod model;
pub(crate) mod particles;
mod playback;
pub(crate) mod progress;
mod read_state;
mod save_pages;
mod scroll;
pub(crate) mod scroll_xf;
pub(crate) mod shake;
mod stage;
pub(crate) mod title;
mod wscroll2;
use effects::*;
pub use errors::MusicaRuntimeError;
pub use model::*;
use stage::*;
mod audio_commands;
mod character;
mod message;
mod movie;
use message::execute_message;
mod choices;
use audio_commands::*;
use choices::execute_select;
use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;

use crate::{ScCommand, ScControlFlow, ScLineKind, ScOperand, ScScript};

/// Reproduces the bounded part of the original audio resource parser:
/// `resource[volume,pan]`, with volume clamped to 0..100 and pan to -100..100.
/// A missing closing bracket leaves the default metadata intact, as observed in
/// the original handler. URI resolution remains a separate fail-closed step.
pub fn parse_audio_resource_spec(
    token: &str,
) -> Result<MusicaAudioResourceSpec, MusicaRuntimeError> {
    if token.is_empty() || token.len() > 4 * 1024 || token.contains('\0') {
        return Err(MusicaRuntimeError::AudioResource);
    }
    let Some(open) = token.find('[') else {
        return Ok(MusicaAudioResourceSpec {
            resource: token.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let resource = &token[..open];
    if resource.is_empty() {
        return Err(MusicaRuntimeError::AudioResource);
    }
    let Some(relative_close) = token[open + 1..].find(']') else {
        return Ok(MusicaAudioResourceSpec {
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
    Ok(MusicaAudioResourceSpec {
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

pub struct MusicaVm {
    script: ScScript,
    labels: BTreeMap<String, u32>,
    state: MusicaRuntimeState,
    control_pressed: bool,
    config: crate::MusicaConfigState,
    config_edit: Option<config::ConfigEdit>,
}

impl MusicaVm {
    pub(crate) fn set_voice_preferences(
        &mut self,
        value: crate::voice_preferences::VoicePreferences,
    ) {
        self.config.backlog_voice_playback = value.backlog_voice_playback;
        self.config.character_voice_enabled = value.character_voice_enabled;
    }
    pub(crate) fn voice_preferences(&self) -> crate::voice_preferences::VoicePreferences {
        self.config.voice_preferences()
    }

    pub fn new(
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
        session_seed: u64,
    ) -> Result<Self, MusicaRuntimeError> {
        let labels = build_labels(&script)?;
        let state = MusicaRuntimeState {
            schema: MUSICA_RUNTIME_STATE_SCHEMA.into(),
            script_uri,
            script_hash,
            script_encoding: script.encoding,
            pc_line: 0,
            launch_mode: MusicaLaunchMode::Direct,
            variables: BTreeMap::new(),
            global_variables: BTreeMap::new(),
            wait: None,
            message: None,
            message_loads: Vec::new(),
            read_message_identities: Vec::new(),
            gallery_unlocks: Vec::new(),
            backlog: Vec::new(),
            backlog_bytes: 0,
            choice: None,
            stage: None,
            transition: MusicaTransitionState::default(),
            effect: None,
            screen_shake: None,
            axis_scroll: None,
            linear_scroll: None,
            scroll_xf: None,
            wscroll2: None,
            characters: BTreeMap::new(),
            firefly: None,
            secondary_effect: None,
            panel: None,
            audio: BTreeMap::new(),
            movie: None,
            system_ui: MusicaSystemUiState::default(),
            fixed_tick: 0,
            session_seed,
            random_state: session_seed,
            instruction_count: 0,
            effect_sequence: 0,
            terminal: false,
        };
        Ok(Self {
            control_pressed: false,
            config: Default::default(),
            config_edit: None,
            script,
            labels,
            state,
        })
    }

    pub fn state(&self) -> &MusicaRuntimeState {
        &self.state
    }

    pub fn encode_native_save(&self) -> Result<Vec<u8>, MusicaRuntimeError> {
        save_pages::validate_state(&self.state)?;
        movie::validate_movie_state(&self.state)?;
        validate_stage_state(self.state.stage.as_ref())?;
        scroll::validate_state(&self.state)?;
        if let Some(shake) = &self.state.screen_shake {
            shake::validate_screen_shake_state(shake)?;
        }
        postcard::to_allocvec(&self.state).map_err(|_| MusicaRuntimeError::NativeSaveFormat)
    }

    pub fn decode_native_save(bytes: &[u8]) -> Result<MusicaRuntimeState, MusicaRuntimeError> {
        let state: MusicaRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MusicaRuntimeError::NativeSaveFormat)?;
        if state.schema != MUSICA_RUNTIME_STATE_SCHEMA {
            return Err(MusicaRuntimeError::State);
        }
        validate_stage_state(state.stage.as_ref())?;
        save_pages::validate_state(&state)?;
        movie::validate_movie_state(&state)?;
        scroll::validate_state(&state)?;
        if let Some(shake) = &state.screen_shake {
            shake::validate_screen_shake_state(shake)?;
        }
        Ok(state)
    }

    pub fn replace_script(
        &mut self,
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
        label: Option<&str>,
    ) -> Result<(), MusicaRuntimeError> {
        let labels = build_labels(&script)?;
        let pc = match label {
            Some(label) => *labels.get(label).ok_or(MusicaRuntimeError::Label)?,
            None => 0,
        };
        self.state.script_encoding = script.encoding;
        self.script = script;
        self.labels = labels;
        self.state.script_uri = script_uri;
        self.state.script_hash = script_hash;
        self.state.pc_line = pc;
        self.state.variables.clear();
        self.state.wait = None;
        self.state.message = None;
        self.state.message_loads.clear();
        self.state.choice = None;
        self.state.movie = None;
        self.state.screen_shake = None;
        self.state.axis_scroll = None;
        self.state.linear_scroll = None;
        self.state.wscroll2 = None;
        self.state.firefly = None;
        self.state.secondary_effect = None;
        self.state.terminal = false;
        Ok(())
    }

    pub fn restore_native_save(
        &mut self,
        bytes: &[u8],
        next_fixed_tick: u64,
    ) -> Result<(), MusicaRuntimeError> {
        let restored: MusicaRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MusicaRuntimeError::NativeSaveFormat)?;
        if restored.schema != MUSICA_RUNTIME_STATE_SCHEMA
            || restored.script_uri != self.state.script_uri
            || restored.script_hash != self.state.script_hash
            || restored.script_encoding != self.script.encoding
            || restored.session_seed != self.state.session_seed
            || restored.pc_line as usize > self.script.lines.len()
        {
            return Err(MusicaRuntimeError::State);
        }
        choices::validate_choice(&self.script, &restored)?;
        save_pages::validate_state(&restored)?;
        movie::validate_movie_state(&restored)?;
        validate_stage_state(restored.stage.as_ref())?;
        scroll::validate_state(&restored)?;
        if let Some(shake) = &restored.screen_shake {
            shake::validate_screen_shake_state(shake)?;
        }
        let restored_tick = next_fixed_tick
            .checked_sub(1)
            .ok_or(MusicaRuntimeError::State)?;
        self.state = restored;
        self.config_edit = None;
        self.state.fixed_tick = restored_tick;
        Ok(())
    }

    pub fn resolve_wait(&mut self, token_id: &str) -> Result<(), MusicaRuntimeError> {
        let current = self
            .state
            .wait
            .as_ref()
            .ok_or(MusicaRuntimeError::Waiting)?;
        let expected = match current {
            MusicaWaitState::Voice { token_id, .. }
            | MusicaWaitState::CharacterTransition { token_id, .. }
            | MusicaWaitState::LinearScroll { token_id, .. }
            | MusicaWaitState::AxisScroll { token_id, .. }
            | MusicaWaitState::Time { token_id, .. }
            | MusicaWaitState::Input { token_id }
            | MusicaWaitState::Media { token_id, .. }
            | MusicaWaitState::Presentation { token_id, .. }
            | MusicaWaitState::Provider { token_id, .. } => token_id,
        };
        if expected != token_id {
            return Err(MusicaRuntimeError::Waiting);
        }
        if matches!(self.state.wait, Some(MusicaWaitState::Media { .. })) {
            save_pages::validate_state(&self.state)?;
            movie::validate_movie_state(&self.state)?;
            self.state.movie = None;
        }
        if token_id.starts_with("musica.message.") {
            self.mark_active_message_read()?;
            message::finish_message_loads(&mut self.state)?;
        }
        if matches!(
            self.state.wait,
            Some(MusicaWaitState::CharacterTransition { .. })
        ) {
            character::complete_character_transition_state(&mut self.state)?;
        }
        self.state.wait = None;
        Ok(())
    }

    pub fn advance_waiting_tick(&mut self, fixed_tick: u64) -> Result<(), MusicaRuntimeError> {
        if self.state.wait.is_none() || fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MusicaRuntimeError::State);
        }
        self.state.fixed_tick = fixed_tick;
        Ok(())
    }

    pub fn advance_effect_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaEffectFrame>, MusicaRuntimeError> {
        let Some(effect) = self.state.effect.as_mut() else {
            return Ok(None);
        };
        effect.elapsed_ns = effect
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let interval_ns = u64::from(effect.interval_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
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
            .ok_or(MusicaRuntimeError::Overflow)?;
        next_effect_sequence(&mut self.state)?;
        frame.sequence = self.state.effect_sequence;
        Ok(Some(frame))
    }

    pub fn step(&mut self, fixed_tick: u64) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
        if fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MusicaRuntimeError::State);
        }
        if self.state.wait.is_some() {
            return Err(MusicaRuntimeError::Waiting);
        }
        if self.state.terminal {
            return Ok(Some(MusicaVmEvent::Terminal));
        }
        self.state.fixed_tick = fixed_tick;
        let mut visited = BTreeSet::new();
        let mut operations = 0u32;
        loop {
            operations += 1;
            if operations > 10_000 {
                return Err(MusicaRuntimeError::NonYieldingCycle);
            }
            let control_state = (
                self.state.pc_line,
                self.state.variables.clone(),
                self.state.global_variables.clone(),
            );
            if !visited.insert(control_state) {
                return Err(MusicaRuntimeError::NonYieldingCycle);
            }
            let line_index = self.state.pc_line as usize;
            let line = self
                .script
                .lines
                .get(line_index)
                .ok_or(MusicaRuntimeError::ProgramCounter)?;
            self.state.pc_line = self
                .state
                .pc_line
                .checked_add(1)
                .ok_or(MusicaRuntimeError::Overflow)?;
            if line
                .language_guard
                .is_some_and(|language| language != self.script.encoding.language())
            {
                continue;
            }
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            self.state.instruction_count = self
                .state
                .instruction_count
                .checked_add(1)
                .ok_or(MusicaRuntimeError::Overflow)?;
            let event = execute_control(
                command,
                &self.labels,
                &mut self.state,
                self.control_pressed,
                &self.config.voice_preferences(),
                self.config.message_speed_auto_play,
            )
            .inspect_err(|cause| {
                tracing::error!(
                    event = "astra.emu.musica.command.failed",
                    code = cause.diagnostic_code(),
                    ordinal = command.ordinal,
                    line = line_index,
                    offset = command.span.offset,
                    operand_count = command.operands.len(),
                    script_hash = self.state.script_hash.to_hex().as_str(),
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
    state: &mut MusicaRuntimeState,
    control_pressed: bool,
    voice_preferences: &crate::voice_preferences::VoicePreferences,
    auto_delay_units: u8,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    match command.opcode.as_str() {
        "label" => Ok(None),
        "pragma" => control::execute_pragma(command, state),
        "set" | "setglobal" => {
            let (key, value) = evaluate_assignment(&command.operands, state)?;
            if command.opcode == "set" {
                state.variables.insert(key.to_owned(), value);
            } else {
                state.global_variables.insert(key.to_owned(), value);
                progress::record(state, key, value);
            }
            Ok(None)
        }
        "goto" => {
            let target = branch_target(&command.control_flow)?;
            state.pc_line = *labels.get(target).ok_or(MusicaRuntimeError::Label)?;
            Ok(None)
        }
        "if" => {
            let [ScOperand::Symbol { value: key }, ScOperand::Operator { value: operator }, ScOperand::Integer { value }, ScOperand::Symbol { value: target }] =
                command.operands.as_slice()
            else {
                return Err(MusicaRuntimeError::Operand);
            };
            let current = state
                .variables
                .get(key)
                .or_else(|| state.global_variables.get(key))
                .copied()
                .unwrap_or(0);
            if compare(current, operator, *value)? {
                state.pc_line = *labels.get(target).ok_or(MusicaRuntimeError::Label)?;
            }
            Ok(None)
        }
        "wait" => {
            let [ScOperand::Integer { value }] = command.operands.as_slice() else {
                return Err(MusicaRuntimeError::Operand);
            };
            let timer_ticks = u32::try_from(*value).map_err(|_| MusicaRuntimeError::Operand)?;
            let milliseconds = timer_ticks
                .checked_mul(10)
                .ok_or(MusicaRuntimeError::Overflow)?;
            if control::fast_forward_active(state, control_pressed) {
                return Ok(None);
            }
            let token_id = format!("musica.wait.{}", state.instruction_count);
            let wait = MusicaWaitState::Time {
                token_id,
                timer_ticks,
                milliseconds,
            };
            state.wait = Some(wait.clone());
            Ok(Some(MusicaVmEvent::Wait(wait)))
        }
        "message" => execute_message(command, state, voice_preferences, auto_delay_units),
        "movie" => movie::execute_movie(command, state),
        "deletevar" => {
            let [ScOperand::Symbol { value: key }] = command.operands.as_slice() else {
                return Err(MusicaRuntimeError::Operand);
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
        "hscroll" => scroll::execute_axis_scroll(command, state, MusicaAxisScrollAxis::Horizontal),
        "vscroll" => scroll::execute_axis_scroll(command, state, MusicaAxisScrollAxis::Vertical),
        "scrollxf" => scroll_xf::execute_scroll_xf(command, state),
        "scroll" => scroll::execute_linear_scroll(command, state),
        "endscroll" => scroll::execute_end_scroll(command, state),
        "shakescreen" => shake::execute_screen_shake(command, state),
        "char" => character::execute_character(command, state),
        "effect2" => particles::execute_secondary_effect(command, state),
        "effect" => execute_effect(command, state),
        "panel" => execute_panel(command, state),
        "chain" => {
            let ScControlFlow::Chain { target } = &command.control_flow else {
                return Err(MusicaRuntimeError::Operand);
            };
            validate_chain_target(target)?;
            Ok(Some(MusicaVmEvent::Chain {
                target: target.clone(),
            }))
        }
        "end" => {
            if state.launch_mode == MusicaLaunchMode::Title {
                title::return_to_title(state);
            } else {
                state.terminal = true;
            }
            Ok(Some(MusicaVmEvent::Terminal))
        }
        _ => Err(MusicaRuntimeError::UnsupportedOpcode {
            opcode: command.opcode.clone(),
            ordinal: command.ordinal,
        }),
    }
}

fn validate_chain_target(target: &str) -> Result<(), MusicaRuntimeError> {
    crate::script::chain_target_parts(target)
        .map(|_| ())
        .ok_or(MusicaRuntimeError::ChainTarget)
}

fn evaluate_assignment<'a>(
    operands: &'a [ScOperand],
    state: &MusicaRuntimeState,
) -> Result<(&'a str, i64), MusicaRuntimeError> {
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
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "-" => left
                    .checked_sub(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "*" => left
                    .checked_mul(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "/" if right != 0 => left
                    .checked_div(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "%" if right != 0 => left
                    .checked_rem(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                _ => return Err(MusicaRuntimeError::Operand),
            };
            Ok((key, value))
        }
        _ => Err(MusicaRuntimeError::Operand),
    }
}

fn resolve_integer(
    operand: &ScOperand,
    state: &MusicaRuntimeState,
) -> Result<i64, MusicaRuntimeError> {
    match operand {
        ScOperand::Integer { value } => Ok(*value),
        ScOperand::Symbol { value } => Ok(state
            .variables
            .get(value)
            .or_else(|| state.global_variables.get(value))
            .copied()
            .unwrap_or(0)),
        _ => Err(MusicaRuntimeError::Operand),
    }
}

fn compare(left: i64, operator: &str, right: i64) -> Result<bool, MusicaRuntimeError> {
    Ok(match operator {
        "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        _ => return Err(MusicaRuntimeError::Operand),
    })
}

fn branch_target(control_flow: &ScControlFlow) -> Result<&str, MusicaRuntimeError> {
    match control_flow {
        ScControlFlow::Jump { target } | ScControlFlow::ConditionalJump { target } => Ok(target),
        _ => Err(MusicaRuntimeError::Operand),
    }
}

fn build_labels(script: &ScScript) -> Result<BTreeMap<String, u32>, MusicaRuntimeError> {
    if script.schema != crate::MUSICA_SCRIPT_IR_SCHEMA {
        return Err(MusicaRuntimeError::State);
    }
    let mut labels = BTreeMap::new();
    for (line_index, line) in script.lines.iter().enumerate() {
        let ScLineKind::Command { command } = &line.kind else {
            continue;
        };
        if command.encoding != script.encoding {
            return Err(MusicaRuntimeError::State);
        }
        if line
            .language_guard
            .is_some_and(|language| language != script.encoding.language())
        {
            continue;
        }
        if let ScControlFlow::Label { id } = &command.control_flow {
            let target = u32::try_from(line_index + 1).map_err(|_| MusicaRuntimeError::Overflow)?;
            if labels.insert(id.clone(), target).is_some() {
                return Err(MusicaRuntimeError::Label);
            }
        }
    }
    Ok(labels)
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;

fn next_effect_sequence(state: &mut MusicaRuntimeState) -> Result<u64, MusicaRuntimeError> {
    state.effect_sequence = state
        .effect_sequence
        .checked_add(1)
        .ok_or(MusicaRuntimeError::Overflow)?;
    Ok(state.effect_sequence)
}
