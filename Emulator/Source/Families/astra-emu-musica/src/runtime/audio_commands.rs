use super::*;

pub(super) fn execute_play_se(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
    loop_stream_id: u32,
    bus: &str,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MusicaRuntimeError::Operand);
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
    let resource_uri = format!("musica:/se/{}", spec.resource);
    let stream_id = if repeat {
        loop_stream_id
    } else {
        let ordinal =
            u32::try_from(state.instruction_count).map_err(|_| MusicaRuntimeError::Overflow)?;
        0x1000_0000u32
            .checked_add(
                ordinal
                    .checked_mul(4)
                    .and_then(|value| value.checked_add(loop_stream_id))
                    .ok_or(MusicaRuntimeError::Overflow)?,
            )
            .ok_or(MusicaRuntimeError::Overflow)?
    };
    let mut commands = Vec::new();
    if repeat {
        match state.audio.get(&loop_stream_id).cloned() {
            Some(current) if current.playing && current.resource_uri == resource_uri => {
                if current.volume_milli != volume_milli || current.pan_milli != pan_milli {
                    commands.push(MusicaAudioCommand::SetParams {
                        sequence: next_effect_sequence(state)?,
                        stream_id,
                        volume: f32::from(volume_milli) / 1000.0,
                        pan: f32::from(pan_milli) / 1000.0,
                        repeat,
                    });
                }
            }
            Some(current) if current.playing => {
                commands.push(MusicaAudioCommand::Stop {
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
        MusicaAudioState {
            bus: bus.into(),
            resource_uri,
            looped: repeat,
            volume_milli,
            pan_milli,
            playing: true,
            continuation_pts: 0,
        },
    );
    Ok(Some(MusicaVmEvent::Audio { commands }))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_audio_load_and_play(
    state: &mut MusicaRuntimeState,
    commands: &mut Vec<MusicaAudioCommand>,
    stream_id: u32,
    resource_uri: &str,
    volume_milli: u16,
    pan_milli: i16,
    repeat: bool,
    fade_in_ms: u32,
) -> Result<(), MusicaRuntimeError> {
    commands.push(MusicaAudioCommand::LoadResource {
        sequence: next_effect_sequence(state)?,
        stream_id,
        resource_uri: resource_uri.into(),
    });
    commands.push(MusicaAudioCommand::Play {
        sequence: next_effect_sequence(state)?,
        stream_id,
        volume: f32::from(volume_milli) / 1000.0,
        pan: f32::from(pan_milli) / 1000.0,
        repeat,
        fade_in_ms,
    });
    Ok(())
}

pub(super) fn execute_play_bgm(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
    bgm_stream_id: u32,
    bus: &str,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MusicaRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource == "*" {
        let fade_out_ms = parse_optional_command_integer(tokens.get(2), 1, 2)?;
        return stop_audio_stream(state, bgm_stream_id, fade_out_ms);
    }
    validate_audio_relative_path(&spec.resource)?;
    let fade_in_ms = parse_optional_command_integer(tokens.get(1), 1, 2)?;
    let fade_out_ms = parse_optional_command_integer(tokens.get(2), 1, 2)?;
    let command_volume = parse_optional_command_integer(tokens.get(3), 100, 100)?;
    if !(0..=400).contains(&command_volume) {
        return Err(MusicaRuntimeError::Operand);
    }
    let volume_milli =
        u16::try_from(i64::from(command_volume) * i64::from(spec.volume_percent) * 1000 / 10_000)
            .map_err(|_| MusicaRuntimeError::Overflow)?;
    let pan_milli = spec.pan_percent * 10;
    let resource_uri = format!("musica:/bgm/{}", spec.resource);
    let mut commands = Vec::new();
    match state.audio.get(&bgm_stream_id).cloned() {
        Some(current) if current.playing && current.resource_uri == resource_uri => {
            if current.volume_milli != volume_milli || current.pan_milli != pan_milli {
                commands.push(MusicaAudioCommand::SetParams {
                    sequence: next_effect_sequence(state)?,
                    stream_id: bgm_stream_id,
                    volume: f32::from(volume_milli) / 1000.0,
                    pan: f32::from(pan_milli) / 1000.0,
                    repeat: true,
                });
            }
        }
        Some(current) if current.playing => {
            commands.push(MusicaAudioCommand::Stop {
                sequence: next_effect_sequence(state)?,
                stream_id: bgm_stream_id,
                fade_ms: fade_out_ms,
            });
            commands.push(MusicaAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: bgm_stream_id,
                resource_uri: resource_uri.clone(),
            });
            commands.push(MusicaAudioCommand::Play {
                sequence: next_effect_sequence(state)?,
                stream_id: bgm_stream_id,
                volume: f32::from(volume_milli) / 1000.0,
                pan: f32::from(pan_milli) / 1000.0,
                repeat: true,
                fade_in_ms,
            });
        }
        _ => {
            commands.push(MusicaAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: bgm_stream_id,
                resource_uri: resource_uri.clone(),
            });
            commands.push(MusicaAudioCommand::Play {
                sequence: next_effect_sequence(state)?,
                stream_id: bgm_stream_id,
                volume: f32::from(volume_milli) / 1000.0,
                pan: f32::from(pan_milli) / 1000.0,
                repeat: true,
                fade_in_ms,
            });
        }
    }
    state.audio.insert(
        bgm_stream_id,
        MusicaAudioState {
            bus: bus.into(),
            resource_uri,
            looped: true,
            volume_milli,
            pan_milli,
            playing: true,
            continuation_pts: 0,
        },
    );
    Ok(Some(MusicaVmEvent::Audio { commands }))
}

pub(super) fn execute_play_voice(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    const VOICE_STREAM_ID: u32 = 4;
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MusicaRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource != "*" {
        // The authorized script census only contains the verified stop form.
        // Message-bound voice lookup is a separate command path and remains
        // fail-closed until its archive key mapping is proven.
        return Err(MusicaRuntimeError::UnsupportedOpcode {
            opcode: "playvoice.resource".into(),
            ordinal: command.ordinal,
        });
    }
    let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
    stop_audio_stream(state, VOICE_STREAM_ID, fade_out_ms)
}

fn stop_audio_stream(
    state: &mut MusicaRuntimeState,
    stream_id: u32,
    fade_ms: u32,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let mut commands = Vec::new();
    if state
        .audio
        .get(&stream_id)
        .is_some_and(|current| current.playing)
    {
        commands.push(MusicaAudioCommand::Stop {
            sequence: next_effect_sequence(state)?,
            stream_id,
            fade_ms,
        });
        if let Some(current) = state.audio.get_mut(&stream_id) {
            current.playing = false;
        }
    }
    Ok(Some(MusicaVmEvent::Audio { commands }))
}

fn parse_optional_command_integer(
    token: Option<&String>,
    missing: i32,
    malformed: i32,
) -> Result<u32, MusicaRuntimeError> {
    let value = token
        .map(|token| parse_c_decimal_prefix(token).unwrap_or(malformed))
        .unwrap_or(missing);
    u32::try_from(value).map_err(|_| MusicaRuntimeError::Operand)
}

pub(super) fn validate_audio_relative_path(value: &str) -> Result<(), MusicaRuntimeError> {
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
        return Err(MusicaRuntimeError::AudioResource);
    }
    Ok(())
}
