use super::*;
const MUSICA_CHARACTER_MAX_SLOTS: usize = 64;
const MUSICA_CHARACTER_MAX_SLOT_ID: u32 = 4096;
const MUSICA_CHARACTER_RESOURCE_COUNT: usize = 1;
const MUSICA_CHARACTER_MAX_COORDINATE: i32 = 65_536;
const MUSICA_CHARACTER_MAX_TRANSITION_MS: u32 = 60_000;
impl MusicaVm {
    pub fn advance_character_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaCharacterFrame>, MusicaRuntimeError> {
        let Some((_, character)) = self.state.characters.iter_mut().find(|(_, character)| {
            character.transition.is_some() || character.replacement.is_some()
        }) else {
            return Ok(None);
        };
        if let Some(transition) = character.transition.as_mut() {
            if transition.completed {
                return Ok(None);
            }
            let duration_ns = u64::from(transition.duration_ms)
                .checked_mul(1_000_000)
                .ok_or(MusicaRuntimeError::Overflow)?;
            transition.elapsed_ns = transition
                .elapsed_ns
                .checked_add(delta_ns)
                .ok_or(MusicaRuntimeError::Overflow)?
                .min(duration_ns);
            character.opacity_256 = interpolate_character_opacity(transition, duration_ns)?;
            transition.completed = transition.elapsed_ns == duration_ns;
        } else {
            let replacement = character
                .replacement
                .as_mut()
                .ok_or(MusicaRuntimeError::Character)?;
            if replacement.completed {
                return Ok(None);
            }
            let duration_ns = u64::from(replacement.duration_ms)
                .checked_mul(1_000_000)
                .ok_or(MusicaRuntimeError::Overflow)?;
            replacement.elapsed_ns = replacement
                .elapsed_ns
                .checked_add(delta_ns)
                .ok_or(MusicaRuntimeError::Overflow)?
                .min(duration_ns);
            let (current_opacity, next_opacity) =
                interpolate_character_replacement(replacement, duration_ns)?;
            character.opacity_256 = current_opacity;
            replacement.next_opacity_256 = next_opacity;
            replacement.completed = replacement.elapsed_ns == duration_ns;
            if replacement.completed {
                complete_character_replacement(character)?;
            }
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaCharacterFrame { sequence }))
    }
}
pub(super) fn execute_character(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = command
        .tokens()
        .map_err(|_| MusicaRuntimeError::Character)?;
    let mode = tokens
        .first()
        .map(|value| value.to_ascii_lowercase())
        .ok_or(MusicaRuntimeError::Character)?;
    match mode.as_str() {
        "load" => {
            let [_, slot, resource] = tokens.as_slice() else {
                return Err(MusicaRuntimeError::Character);
            };
            let signed_slot = parse_character_slot(slot)?;
            let slot_id = signed_slot.unsigned_abs();
            if !state.characters.contains_key(&slot_id)
                && state.characters.len() >= MUSICA_CHARACTER_MAX_SLOTS
            {
                return Err(MusicaRuntimeError::Character);
            }
            validate_scene_filename(resource).map_err(|_| MusicaRuntimeError::Character)?;
            let resource_uris = vec![format!("musica:/st/{resource}")];
            state.characters.insert(
                slot_id,
                MusicaCharacterState {
                    slot_id,
                    positive_orientation: signed_slot >= 0,
                    resource_uris,
                    anchor_position: [0, 0],
                    visible: true,
                    opacity_256: 256,
                    transition: None,
                    replacement: None,
                    pending_stage: true,
                    keep_once: false,
                },
            );
        }
        "pos" => {
            let [_, slot, x, y] = tokens.as_slice() else {
                return Err(MusicaRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            let x = parse_character_coordinate(x)?;
            let y = parse_character_coordinate(y)?;
            let character = state
                .characters
                .get_mut(&slot_id)
                .ok_or(MusicaRuntimeError::Character)?;
            character.anchor_position = [x, y];
        }
        "trans" => {
            let [_, slot, duration, opacity] = tokens.as_slice() else {
                return Err(MusicaRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            let duration_ms = parse_character_transition_duration(duration)?;
            let target_opacity_256 = parse_character_opacity(opacity)?;
            let character = state
                .characters
                .get_mut(&slot_id)
                .ok_or(MusicaRuntimeError::Character)?;
            if character.transition.is_some() || character.replacement.is_some() {
                return Err(MusicaRuntimeError::Character);
            }
            if duration_ms == 0 {
                character.opacity_256 = target_opacity_256;
            } else {
                character.transition = Some(MusicaCharacterTransitionState {
                    start_opacity_256: character.opacity_256,
                    target_opacity_256,
                    duration_ms,
                    elapsed_ns: 0,
                    completed: false,
                });
                let wait = MusicaWaitState::CharacterTransition {
                    token_id: format!("musica.character.{slot_id}.{}", state.instruction_count),
                    slot_id,
                    milliseconds: duration_ms,
                };
                state.wait = Some(wait.clone());
                return Ok(Some(MusicaVmEvent::Wait(wait)));
            }
        }
        "vis" => {
            let [_, slot, visible] = tokens.as_slice() else {
                return Err(MusicaRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            let visible = parse_native_bool(visible);
            let character = state
                .characters
                .get_mut(&slot_id)
                .ok_or(MusicaRuntimeError::Character)?;
            character.visible = visible;
        }
        "keep" => {
            let [_, slot] = tokens.as_slice() else {
                return Err(MusicaRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            if let Some(character) = state.characters.get_mut(&slot_id) {
                character.keep_once = true;
            }
        }
        _ => return Err(MusicaRuntimeError::Character),
    }
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::Character(MusicaCharacterFrame {
        sequence,
    })))
}
fn parse_character_slot(token: &str) -> Result<i32, MusicaRuntimeError> {
    let slot = token
        .parse::<i32>()
        .ok()
        .filter(|slot| *slot != 0 && slot.unsigned_abs() <= MUSICA_CHARACTER_MAX_SLOT_ID)
        .ok_or(MusicaRuntimeError::Character)?;
    Ok(slot)
}
fn parse_character_coordinate(token: &str) -> Result<i32, MusicaRuntimeError> {
    token
        .parse::<i32>()
        .ok()
        .filter(|value| value.unsigned_abs() <= MUSICA_CHARACTER_MAX_COORDINATE as u32)
        .ok_or(MusicaRuntimeError::Character)
}
fn parse_character_transition_duration(token: &str) -> Result<u32, MusicaRuntimeError> {
    token
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= MUSICA_CHARACTER_MAX_TRANSITION_MS)
        .ok_or(MusicaRuntimeError::Character)
}
fn parse_character_opacity(token: &str) -> Result<u16, MusicaRuntimeError> {
    token
        .parse::<u16>()
        .ok()
        .filter(|value| *value <= 255)
        .ok_or(MusicaRuntimeError::Character)
}
fn parse_native_bool(token: &str) -> bool {
    token
        .as_bytes()
        .first()
        .is_some_and(|value| matches!(value, b'1'..=b'9' | b't' | b'T'))
}
pub(super) fn validate_character_state(
    characters: &BTreeMap<u32, MusicaCharacterState>,
) -> Result<(), MusicaRuntimeError> {
    if characters.len() > MUSICA_CHARACTER_MAX_SLOTS {
        return Err(MusicaRuntimeError::Character);
    }
    let mut transition_count = 0usize;
    for (slot_id, character) in characters {
        if *slot_id == 0
            || *slot_id > MUSICA_CHARACTER_MAX_SLOT_ID
            || character.slot_id != *slot_id
            || character.resource_uris.len() != MUSICA_CHARACTER_RESOURCE_COUNT
            || character.opacity_256 > 256
            || character
                .anchor_position
                .iter()
                .any(|value| value.unsigned_abs() > MUSICA_CHARACTER_MAX_COORDINATE as u32)
        {
            return Err(MusicaRuntimeError::Character);
        }
        if character.transition.is_some() && character.replacement.is_some() {
            return Err(MusicaRuntimeError::Character);
        }
        if let Some(transition) = character.transition.as_ref() {
            transition_count = transition_count
                .checked_add(1)
                .ok_or(MusicaRuntimeError::Overflow)?;
            let duration_ns = u64::from(transition.duration_ms)
                .checked_mul(1_000_000)
                .ok_or(MusicaRuntimeError::Overflow)?;
            if transition.duration_ms == 0
                || transition.duration_ms > MUSICA_CHARACTER_MAX_TRANSITION_MS
                || transition.start_opacity_256 > 256
                || transition.target_opacity_256 > 255
                || transition.elapsed_ns > duration_ns
                || transition.completed != (transition.elapsed_ns == duration_ns)
                || character.opacity_256 != interpolate_character_opacity(transition, duration_ns)?
            {
                return Err(MusicaRuntimeError::Character);
            }
        }
        if let Some(replacement) = character.replacement.as_ref() {
            transition_count = transition_count
                .checked_add(1)
                .ok_or(MusicaRuntimeError::Overflow)?;
            let duration_ns = u64::from(replacement.duration_ms)
                .checked_mul(1_000_000)
                .ok_or(MusicaRuntimeError::Overflow)?;
            let (current_opacity, next_opacity) =
                interpolate_character_replacement(replacement, duration_ns)?;
            if replacement.duration_ms == 0
                || replacement.duration_ms > MUSICA_CHARACTER_MAX_TRANSITION_MS
                || replacement.start_opacity_256 > 256
                || replacement.target_opacity_256 > 256
                || replacement.next_opacity_256 > 256
                || replacement.elapsed_ns >= duration_ns
                || replacement.completed
                || character.opacity_256 != current_opacity
                || replacement.next_opacity_256 != next_opacity
                || super::stage::validate_uri(&replacement.resource_uri, "musica:/st/").is_err()
            {
                return Err(MusicaRuntimeError::Character);
            }
        }
        for resource_uri in &character.resource_uris {
            super::stage::validate_uri(resource_uri, "musica:/st/")
                .map_err(|_| MusicaRuntimeError::Character)?;
        }
    }
    if transition_count > 1 {
        return Err(MusicaRuntimeError::Character);
    }
    Ok(())
}
fn interpolate_character_opacity(
    transition: &MusicaCharacterTransitionState,
    duration_ns: u64,
) -> Result<u16, MusicaRuntimeError> {
    if duration_ns == 0 || transition.elapsed_ns > duration_ns {
        return Err(MusicaRuntimeError::Character);
    }
    let start = i128::from(transition.start_opacity_256);
    let delta = i128::from(transition.target_opacity_256) - start;
    let elapsed = i128::from(transition.elapsed_ns);
    let duration = i128::from(duration_ns);
    let value = start
        .checked_add(
            delta
                .checked_mul(elapsed)
                .ok_or(MusicaRuntimeError::Overflow)?
                / duration,
        )
        .ok_or(MusicaRuntimeError::Overflow)?;
    u16::try_from(value).map_err(|_| MusicaRuntimeError::Character)
}
fn interpolate_character_replacement(
    replacement: &MusicaCharacterReplacementState,
    duration_ns: u64,
) -> Result<(u16, u16), MusicaRuntimeError> {
    if duration_ns == 0 || replacement.elapsed_ns > duration_ns {
        return Err(MusicaRuntimeError::Character);
    }
    let elapsed = u128::from(replacement.elapsed_ns);
    let duration = u128::from(duration_ns);
    let remaining = duration
        .checked_sub(elapsed)
        .ok_or(MusicaRuntimeError::Overflow)?;
    let current = u128::from(replacement.start_opacity_256)
        .checked_mul(remaining)
        .ok_or(MusicaRuntimeError::Overflow)?
        / duration;
    let next = u128::from(replacement.target_opacity_256)
        .checked_mul(elapsed)
        .ok_or(MusicaRuntimeError::Overflow)?
        / duration;
    Ok((
        u16::try_from(current).map_err(|_| MusicaRuntimeError::Character)?,
        u16::try_from(next).map_err(|_| MusicaRuntimeError::Character)?,
    ))
}
pub(super) fn complete_character_replacement(
    character: &mut MusicaCharacterState,
) -> Result<(), MusicaRuntimeError> {
    let Some(replacement) = character.replacement.take() else {
        return Ok(());
    };
    super::stage::validate_uri(&replacement.resource_uri, "musica:/st/")?;
    character.resource_uris = vec![replacement.resource_uri];
    character.opacity_256 = replacement.target_opacity_256;
    Ok(())
}
pub(super) fn complete_character_transition_state(
    state: &mut MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    let slot_id = match state.wait.as_ref() {
        Some(MusicaWaitState::CharacterTransition { slot_id, .. }) => *slot_id,
        _ => return Err(MusicaRuntimeError::Character),
    };
    let character = state
        .characters
        .get_mut(&slot_id)
        .ok_or(MusicaRuntimeError::Character)?;
    let transition = character
        .transition
        .take()
        .ok_or(MusicaRuntimeError::Character)?;
    character.opacity_256 = transition.target_opacity_256;
    Ok(())
}
pub(super) fn validate_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    validate_character_state(&state.characters)?;
    let transition_slot = state.characters.iter().find_map(|(slot, character)| {
        (character.transition.is_some() || character.replacement.is_some()).then_some(*slot)
    });
    match (&state.wait, transition_slot) {
        (Some(MusicaWaitState::CharacterTransition { slot_id, .. }), Some(slot))
            if *slot_id == slot =>
        {
            Ok(())
        }
        (
            Some(
                MusicaWaitState::Input { token_id }
                | MusicaWaitState::Time { token_id, .. }
                | MusicaWaitState::Voice { token_id, .. },
            ),
            Some(_),
        ) if token_id.starts_with("musica.message.") => Ok(()),
        (Some(MusicaWaitState::CharacterTransition { .. }), _) | (_, Some(_)) => {
            Err(MusicaRuntimeError::Character)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
