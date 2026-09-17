use super::*;
use crate::{parse_musica_message_markup, MusicaMessageControl};
pub(super) fn execute_message(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
    preferences: &crate::voice_preferences::VoicePreferences,
    auto_delay_units: u8,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Operand)?;
    let (message_id, voice, speaker, authored_text) = if tokens.len() >= 4 {
        let message_id = tokens[0]
            .parse::<i64>()
            .map_err(|_| MusicaRuntimeError::Operand)?;
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
    let markup = parse_musica_message_markup(&authored_text)?;
    let auto_advance = markup.auto_advance();
    let wait_for_voice = markup.waits_for_voice();
    let controls = markup.controls;
    let text = markup.visible_text;
    if !state.message_loads.is_empty() {
        return Err(MusicaRuntimeError::Character);
    }
    state.message_loads = controls
        .iter()
        .filter_map(|control| match control {
            MusicaMessageControl::LoadCharacter {
                delay_ms,
                slot_id,
                resource,
                transition_ms,
                opacity_255,
                ..
            } => Some(MusicaMessageLoadState {
                delay_ms: *delay_ms,
                elapsed_ns: 0,
                slot_id: *slot_id,
                resource_uri: format!("musica:/st/{resource}"),
                transition_ms: *transition_ms,
                opacity_256: if *opacity_255 == 255 {
                    256
                } else {
                    u16::from(*opacity_255)
                },
            }),
            MusicaMessageControl::WaitForVoice { .. }
            | MusicaMessageControl::AutoAdvance { .. } => None,
        })
        .collect();
    let mut audio_commands = Vec::new();
    if state.audio.get(&4).is_some_and(|voice| voice.playing) {
        audio_commands.push(MusicaAudioCommand::Stop {
            sequence: next_effect_sequence(state)?,
            stream_id: 4,
            fade_ms: 0,
        });
    }
    let voice_metadata = voice
        .as_ref()
        .map(|token| {
            let spec = parse_audio_resource_spec(token)?;
            super::audio_commands::validate_audio_relative_path(&spec.resource)?;
            Ok::<_, MusicaRuntimeError>(MusicaMessageVoice {
                resource_uri: format!("musica:/voice/{}", spec.resource),
                volume_milli: spec.volume_percent * 10,
                pan_milli: spec.pan_percent * 10,
            })
        })
        .transpose()?;
    if let Some(value) = &voice_metadata {
        let enabled = preferences.enabled(&value.resource_uri);
        if enabled {
            super::audio_commands::append_audio_load_and_play(
                state,
                &mut audio_commands,
                4,
                &value.resource_uri,
                value.volume_milli,
                value.pan_milli,
                false,
                0,
            )?;
        } else if wait_for_voice {
            // The source waits for authored voice duration even when this character is muted.
            audio_commands.push(MusicaAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: 4,
                resource_uri: value.resource_uri.clone(),
            });
        }
        state.audio.insert(
            4,
            MusicaAudioState {
                bus: "voice".into(),
                continuation_pts: 0,
                resource_uri: value.resource_uri.clone(),
                looped: false,
                volume_milli: value.volume_milli,
                pan_milli: value.pan_milli,
                playing: enabled,
            },
        );
    } else if let Some(voice) = state.audio.get_mut(&4) {
        voice.playing = false;
    }
    super::backlog::append_backlog_entry(
        state,
        MusicaBacklogEntry {
            source: command.span,
            message_id,
            text: text.clone(),
            speaker: speaker.clone(),
            text_hash: Hash256::from_sha256(text.as_bytes()),
            speaker_hash: speaker
                .as_ref()
                .map(|value| Hash256::from_sha256(value.as_bytes())),
            voice_hash: voice
                .as_ref()
                .map(|value| Hash256::from_sha256(value.as_bytes())),
            voice: voice_metadata,
        },
    )?;
    state.message = Some(MusicaMessageState {
        auto_advance,
        wait_for_voice,
        source: command.span,
        message_id,
    });
    let presentation_sequence = next_effect_sequence(state)?;
    let capture_sequence = next_effect_sequence(state)?;
    let token_id = format!("musica.message.{}", state.instruction_count);
    let wait = if wait_for_voice && voice.is_some() {
        MusicaWaitState::Voice {
            token_id,
            stream_id: 4,
            milliseconds: None,
        }
    } else if auto_advance || wait_for_voice {
        MusicaWaitState::Time {
            token_id,
            timer_ticks: 1,
            milliseconds: 10,
        }
    } else if state.system_ui.auto_mode {
        super::playback::auto_wait(token_id, auto_delay_units)
    } else {
        MusicaWaitState::Input { token_id }
    };
    state.wait = Some(wait.clone());
    Ok(Some(MusicaVmEvent::Message {
        presentation_sequence,
        capture_sequence,
        audio_commands,
        text,
        speaker,
        wait,
    }))
}

impl MusicaVm {
    pub fn advance_message_load_clock(
        &mut self,
        delta_ns: u64,
        animations_enabled: bool,
    ) -> Result<Option<MusicaCharacterFrame>, MusicaRuntimeError> {
        if self.state.message_loads.is_empty() {
            return Ok(None);
        }
        let mut due = Vec::new();
        for load in &mut self.state.message_loads {
            load.elapsed_ns = load
                .elapsed_ns
                .checked_add(delta_ns)
                .ok_or(MusicaRuntimeError::Overflow)?;
            let delay_ns = u64::from(load.delay_ms)
                .checked_mul(1_000_000)
                .ok_or(MusicaRuntimeError::Overflow)?;
            if load.elapsed_ns >= delay_ns {
                due.push(load.clone());
            }
        }
        if due.is_empty() {
            return Ok(None);
        }
        self.state.message_loads.retain(|load| {
            let delay_ns = u64::from(load.delay_ms).saturating_mul(1_000_000);
            load.elapsed_ns < delay_ns
        });
        for load in due {
            apply_message_character_load(&mut self.state, &load, animations_enabled)?;
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaCharacterFrame { sequence }))
    }
    pub(crate) fn set_voice_duration(&mut self, duration: u32) -> Result<(), MusicaRuntimeError> {
        match self.state.wait.as_mut() {
            Some(MusicaWaitState::Voice { milliseconds, .. }) if milliseconds.is_none() => {
                *milliseconds = Some(duration);
                Ok(())
            }
            _ => Err(MusicaRuntimeError::Waiting),
        }
    }
}
fn apply_message_character_load(
    state: &mut MusicaRuntimeState,
    load: &MusicaMessageLoadState,
    animate: bool,
) -> Result<(), MusicaRuntimeError> {
    super::stage::validate_uri(&load.resource_uri, "musica:/st/")?;
    let character = state
        .characters
        .get_mut(&load.slot_id)
        .ok_or(MusicaRuntimeError::Character)?;
    if character.transition.is_some() || character.replacement.is_some() {
        return Err(MusicaRuntimeError::Character);
    }
    if animate && load.transition_ms != 0 {
        character.replacement = Some(MusicaCharacterReplacementState {
            resource_uri: load.resource_uri.clone(),
            start_opacity_256: character.opacity_256,
            target_opacity_256: load.opacity_256,
            next_opacity_256: 0,
            duration_ms: load.transition_ms,
            elapsed_ns: 0,
            completed: false,
        });
    } else {
        character.resource_uris = vec![load.resource_uri.clone()];
        character.opacity_256 = load.opacity_256;
        character.transition = None;
        character.replacement = None;
    }
    Ok(())
}
pub(super) fn finish_message_loads(
    state: &mut MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    let pending = std::mem::take(&mut state.message_loads);
    for load in pending {
        apply_message_character_load(state, &load, false)?;
    }
    for character in state.characters.values_mut() {
        if let Some(transition) = character.transition.take() {
            character.opacity_256 = transition.target_opacity_256;
        }
        super::character::complete_character_replacement(character)?;
    }
    Ok(())
}

pub(super) fn validate_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if state.message_loads.len() > 128
        || (!state.message_loads.is_empty() && state.message.is_none())
    {
        return Err(MusicaRuntimeError::Character);
    }
    for load in &state.message_loads {
        super::stage::validate_uri(&load.resource_uri, "musica:/st/")?;
        if load.slot_id == 0
            || load.slot_id > 4096
            || load.transition_ms > 60_000
            || load.delay_ms > 60_000
            || load.opacity_256 > 256
            || (load.delay_ms != 0 && load.elapsed_ns >= u64::from(load.delay_ms) * 1_000_000)
        {
            return Err(MusicaRuntimeError::Character);
        }
    }
    if let Some(MusicaWaitState::Voice { stream_id, .. }) = &state.wait {
        if *stream_id != 4
            || state.message.is_none()
            || !state
                .audio
                .get(stream_id)
                .is_some_and(|voice| voice.bus == "voice" && !voice.looped)
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    Ok(())
}
