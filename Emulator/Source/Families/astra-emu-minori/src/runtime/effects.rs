use super::*;

pub(super) fn execute_effect(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Effect)?;
    if tokens.is_empty()
        || tokens.len() > 5
        || !matches!(tokens[0].as_str(), "CrossFade2" | "CrossFade")
    {
        return Err(MinoriRuntimeError::Effect);
    }
    if tokens.len() == 1 || (tokens.len() == 4 && tokens[1] == "*") {
        next_effect_sequence(state)?;
        state.effect = None;
        return Ok(Some(MinoriVmEvent::EffectCleared));
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

pub(super) fn execute_panel(
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

pub(super) fn next_effect_index(current: u32, len: usize) -> Result<u32, MinoriRuntimeError> {
    let len = u32::try_from(len).map_err(|_| MinoriRuntimeError::Overflow)?;
    current
        .checked_add(1)
        .map(|next| next % len)
        .ok_or(MinoriRuntimeError::Overflow)
}

pub(super) fn effect_frame(
    effect: &MinoriEffectState,
) -> Result<MinoriEffectFrame, MinoriRuntimeError> {
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

pub(super) fn execute_transition(
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

pub(super) fn execute_stage(
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
