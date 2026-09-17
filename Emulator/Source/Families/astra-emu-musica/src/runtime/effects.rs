use super::*;

pub(super) fn execute_effect(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Effect)?;
    if tokens
        .first()
        .is_some_and(|name| matches!(name.as_str(), "Snow" | "Firefly" | "end" | "fadeout"))
    {
        return super::particles::execute_primary(&tokens, state);
    }
    if tokens.first().is_some_and(|name| name == "WScroll2") {
        return super::wscroll2::execute(&tokens, state);
    }
    if tokens.is_empty()
        || tokens.len() > 5
        || !matches!(tokens[0].as_str(), "*" | "CrossFade2" | "CrossFade")
    {
        return Err(MusicaRuntimeError::Effect);
    }
    if tokens[0] == "*" || tokens.len() == 1 || (tokens.len() == 4 && tokens[1] == "*") {
        next_effect_sequence(state)?;
        state.effect = None;
        state.firefly = None;
        state.wscroll2 = None;
        return Ok(Some(MusicaVmEvent::EffectCleared));
    }
    let resources = tokens[1]
        .split(':')
        .map(|resource| {
            if resource == "*" {
                Ok(None)
            } else {
                validate_scene_filename(resource)?;
                Ok(Some(format!("musica:/bg/{resource}")))
            }
        })
        .collect::<Result<Vec<_>, MusicaRuntimeError>>()?;
    if resources.len() < 2 || resources.len() > 64 || resources.iter().all(Option::is_none) {
        return Err(MusicaRuntimeError::Effect);
    }
    let alpha_step = parse_effect_integer(tokens.get(2), -1)?;
    let interval_ms = parse_effect_integer(tokens.get(3), -1)?;
    let unused = parse_effect_integer(tokens.get(4), -1)?;
    if alpha_step <= 0 || interval_ms <= 0 || unused != -1 {
        return Err(MusicaRuntimeError::Effect);
    }
    let mut effect = MusicaEffectState {
        kind: MusicaEffectKind::CrossFade2,
        resources,
        current_index: 0,
        next_index: 1,
        alpha_255: 0,
        alpha_step: u32::try_from(alpha_step).map_err(|_| MusicaRuntimeError::Effect)?,
        interval_ms: u32::try_from(interval_ms).map_err(|_| MusicaRuntimeError::Effect)?,
        elapsed_ns: 0,
        visible_current_index: 0,
        visible_next_index: 1,
        visible_alpha_255: 0,
    };
    // Creation immediately performs the first zero-alpha update in the
    // original effect object, then advances the accumulator.
    effect.alpha_255 = effect.alpha_step;
    state.effect = Some(effect);
    state.firefly = None;
    state.wscroll2 = None;
    next_effect_sequence(state)?;
    let mut frame = effect_frame(state.effect.as_ref().ok_or(MusicaRuntimeError::Effect)?)?;
    frame.alpha_255 = 0;
    frame.sequence = state.effect_sequence;
    Ok(Some(MusicaVmEvent::Effect(frame)))
}

pub(super) fn execute_panel(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Panel)?;
    let panel = match tokens.as_slice() {
        [mode] if mode == "0" => None,
        [mode] if mode == "1" => Some(MusicaPanelState {
            mode: 1,
            resource_uri: "musica:/sys/msgPanel.png".into(),
        }),
        [mode, transition, filename] if mode == "1" && transition == "*" => {
            validate_scene_filename(filename).map_err(|_| MusicaRuntimeError::Panel)?;
            Some(MusicaPanelState {
                mode: 1,
                resource_uri: format!("musica:/sys/{filename}"),
            })
        }
        [mode] if mode == "3" => Some(MusicaPanelState {
            mode: 3,
            resource_uri: "musica:/sys/fullPanel.png".into(),
        }),
        _ => return Err(MusicaRuntimeError::Panel),
    };
    state.panel = panel;
    next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::Panel {
        sequence: state.effect_sequence,
    }))
}

fn parse_effect_integer(token: Option<&String>, default: i32) -> Result<i32, MusicaRuntimeError> {
    token.map_or(Ok(default), |value| {
        value.parse().map_err(|_| MusicaRuntimeError::Effect)
    })
}

pub(super) fn next_effect_index(current: u32, len: usize) -> Result<u32, MusicaRuntimeError> {
    let len = u32::try_from(len).map_err(|_| MusicaRuntimeError::Overflow)?;
    current
        .checked_add(1)
        .map(|next| next % len)
        .ok_or(MusicaRuntimeError::Overflow)
}

pub(super) fn effect_frame(
    effect: &MusicaEffectState,
) -> Result<MusicaEffectFrame, MusicaRuntimeError> {
    let current = usize::try_from(effect.current_index).map_err(|_| MusicaRuntimeError::Effect)?;
    let next = usize::try_from(effect.next_index).map_err(|_| MusicaRuntimeError::Effect)?;
    Ok(MusicaEffectFrame {
        sequence: 0,
        current_resource_uri: effect
            .resources
            .get(current)
            .ok_or(MusicaRuntimeError::Effect)?
            .clone(),
        next_resource_uri: effect
            .resources
            .get(next)
            .ok_or(MusicaRuntimeError::Effect)?
            .clone(),
        alpha_255: effect.alpha_255.min(255) as u16,
    })
}

pub(super) fn execute_transition(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Operand)?;
    let [mode, resource, duration] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::Operand);
    };
    let mode = mode
        .parse::<i32>()
        .map_err(|_| MusicaRuntimeError::Operand)?;
    let duration_ticks = duration
        .parse::<u32>()
        .map_err(|_| MusicaRuntimeError::Operand)?;
    let resource = if resource == "*" {
        None
    } else {
        validate_scene_filename(resource)?;
        Some(resource.clone())
    };
    state.transition = MusicaTransitionState {
        mode,
        resource,
        duration_ticks,
    };
    state.screen_shake = None;
    Ok(None)
}

pub(super) fn validate_scene_filename(value: &str) -> Result<(), MusicaRuntimeError> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value.contains(['/', '\\', ':', '\0'])
        || value == "."
        || value == ".."
    {
        return Err(MusicaRuntimeError::Operand);
    }
    Ok(())
}
