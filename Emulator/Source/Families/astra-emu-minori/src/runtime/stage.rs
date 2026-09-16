use super::effects::validate_scene_filename;
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn execute_stage(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    // Native scripts may end the operand list with one empty field.
    let tokens = if tokens.last().is_some_and(String::is_empty) {
        &tokens[..tokens.len() - 1]
    } else {
        &tokens
    };
    if !(4..=26).contains(&tokens.len()) || tokens.last().is_some_and(String::is_empty) {
        return Err(MinoriRuntimeError::Operand);
    }
    let resource_sequence = tokens[0]
        .split(':')
        .map(|name| resource("bg", name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut cursor = 1;
    let reference_position = if tokens.len() >= 6 {
        match (tokens[1].parse::<i32>(), tokens[2].parse::<i32>()) {
            (Ok(x), Ok(y)) => {
                cursor = 3;
                Some([x, y])
            }
            _ => None,
        }
    } else {
        None
    };
    if cursor + 3 > tokens.len() || !(tokens.len() - cursor - 3).is_multiple_of(2) {
        return Err(MinoriRuntimeError::Operand);
    }
    let x = integer(&tokens[cursor + 1])?;
    let y = integer(&tokens[cursor + 2])?;
    let background = resource("bg", &tokens[cursor])?.map(|resource_uri| MinoriStageLayer {
        resource_uri,
        x,
        y,
    });
    cursor += 3;
    let mut stands = Vec::new();
    while cursor < tokens.len() {
        let resource_uri = resource("st", &tokens[cursor])?.ok_or(MinoriRuntimeError::Operand)?;
        let mut spec = tokens[cursor + 1].split(',');
        let position = integer(spec.next().ok_or(MinoriRuntimeError::Operand)?)?;
        let resource_parameter = spec.next().map(integer).transpose()?.unwrap_or(0);
        if spec.next().is_some() {
            return Err(MinoriRuntimeError::Operand);
        }
        stands.push(MinoriStandLayer {
            resource_uri,
            position,
            resource_parameter,
        });
        cursor += 2;
    }
    let stage = MinoriStageCommand {
        resource_sequence,
        reference_position,
        background,
        stands,
        transition: state.transition.clone(),
    };
    validate_stage_state(Some(&stage))?;
    next_effect_sequence(state)?;
    state.stage = Some(stage.clone());
    Ok(Some(MinoriVmEvent::Stage(stage)))
}

fn integer(value: &str) -> Result<i32, MinoriRuntimeError> {
    value.parse().map_err(|_| MinoriRuntimeError::Operand)
}

fn resource(role: &str, name: &str) -> Result<Option<String>, MinoriRuntimeError> {
    if name == "*" {
        return Ok(None);
    }
    validate_scene_filename(name)?;
    Ok(Some(format!("minori:/{role}/{name}")))
}

pub(super) fn validate_stage_state(
    stage: Option<&MinoriStageCommand>,
) -> Result<(), MinoriRuntimeError> {
    let Some(stage) = stage else {
        return Ok(());
    };
    if !(1..=2).contains(&stage.resource_sequence.len()) || stage.stands.len() > 10 {
        return Err(MinoriRuntimeError::State);
    }
    for uri in stage.resource_sequence.iter().flatten() {
        validate_uri(uri, "minori:/bg/")?;
    }
    if let Some(background) = &stage.background {
        validate_uri(&background.resource_uri, "minori:/bg/")?;
    }
    for stand in &stage.stands {
        validate_uri(&stand.resource_uri, "minori:/st/")?;
    }
    if let Some(name) = &stage.transition.resource {
        validate_scene_filename(name)?;
    }
    Ok(())
}

fn validate_uri(uri: &str, prefix: &str) -> Result<(), MinoriRuntimeError> {
    let name = uri.strip_prefix(prefix).ok_or(MinoriRuntimeError::State)?;
    if name == "*" {
        return Err(MinoriRuntimeError::State);
    }
    validate_scene_filename(name)
}
