use super::*;
mod firefly;
mod snow;
use firefly::*;
use snow::*;
const MUSICA_FIREFLY_STAGE_WIDTH: i32 = 1280;
const MUSICA_FIREFLY_STAGE_HEIGHT: i32 = 720;
const MUSICA_FIREFLY_CONTROL_POINTS: usize = 7;
const MUSICA_FIREFLY_MAX_PARTICLES: usize = 256;
const MUSICA_FIREFLY_MAX_PARTICLES_U32: u32 = 256;
const MUSICA_FIREFLY_MAX_DURATION_MS: u32 = 60_000;
const MUSICA_FIREFLY_FADE_SCALE: u16 = 256;
const MUSICA_FIREFLY_FADE_STEP_NS: u64 = 16_000_000;
const MUSICA_SNOW_H_PARTICLE_COUNT: usize = 50;
const MUSICA_SNOW_H_FADE_SCALE: u16 = 256;
const MUSICA_SNOW_H_FADE_STEP_NS: u64 = 16_000_000;
const MUSICA_SNOW_H_STAGE_WIDTH: i32 = 1280;
const MUSICA_SNOW_H_STAGE_HEIGHT: i32 = 720;
impl MusicaVm {
    pub fn advance_firefly_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
        let mut random_state = self.state.random_state;
        let mut clear = false;
        let Some(firefly) = self.state.firefly.as_mut() else {
            return Ok(None);
        };
        firefly.fade_elapsed_ns = firefly
            .fade_elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let fade_steps = firefly.fade_elapsed_ns / MUSICA_FIREFLY_FADE_STEP_NS;
        firefly.fade_elapsed_ns %= MUSICA_FIREFLY_FADE_STEP_NS;
        let fade_steps = u16::try_from(fade_steps.min(u64::from(u16::MAX)))
            .map_err(|_| MusicaRuntimeError::Overflow)?;
        if firefly.ending {
            firefly.fade_alpha_256 = firefly.fade_alpha_256.saturating_sub(fade_steps);
            if firefly.fade_alpha_256 == 0 {
                clear = true;
            }
        } else {
            firefly.fade_alpha_256 = firefly
                .fade_alpha_256
                .saturating_add(fade_steps)
                .min(MUSICA_FIREFLY_FADE_SCALE);
        }
        if !clear {
            for particle in &mut firefly.particles {
                particle.elapsed_ns = particle
                    .elapsed_ns
                    .checked_add(delta_ns)
                    .ok_or(MusicaRuntimeError::Overflow)?;
                if particle.elapsed_ns >= particle.lifetime_ns {
                    respawn_firefly_particle(particle, firefly.duration_ms, &mut random_state)?;
                }
                let t = fixed_firefly_parameter(particle.elapsed_ns, particle.lifetime_ns)?;
                particle.position = firefly_curve_position(&particle.control_points, t)?;
                particle.opacity_255 = firefly_particle_opacity(t);
            }
        }
        self.state.random_state = random_state;
        if clear {
            self.state.firefly = None;
            let sequence = next_effect_sequence(&mut self.state)?;
            return Ok(Some(MusicaVmEvent::FireflyCleared { sequence }));
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaVmEvent::Firefly(MusicaFireflyFrame {
            sequence,
        })))
    }
    pub fn advance_secondary_effect_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
        let mut random_state = self.state.random_state;
        let mut clear = false;
        let mut changed = false;
        let Some(effect) = self.state.secondary_effect.as_mut() else {
            return Ok(None);
        };
        effect.fade_elapsed_ns = effect
            .fade_elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let fade_steps = effect.fade_elapsed_ns / MUSICA_SNOW_H_FADE_STEP_NS;
        effect.fade_elapsed_ns %= MUSICA_SNOW_H_FADE_STEP_NS;
        if fade_steps != 0 {
            changed = true;
            let fade_steps = u16::try_from(fade_steps.min(u64::from(u16::MAX)))
                .map_err(|_| MusicaRuntimeError::Overflow)?;
            if effect.ending {
                effect.alpha_256 = effect.alpha_256.saturating_sub(fade_steps);
                clear = effect.alpha_256 == 0;
            } else {
                effect.alpha_256 = effect
                    .alpha_256
                    .saturating_add(fade_steps)
                    .min(MUSICA_SNOW_H_FADE_SCALE);
            }
        }
        effect.motion_elapsed_ns = effect
            .motion_elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let elapsed_ms = effect.motion_elapsed_ns / 1_000_000;
        effect.motion_elapsed_ns %= 1_000_000;
        if elapsed_ms != 0 && !clear {
            changed = true;
            for particle in &mut effect.particles {
                match effect.kind {
                    MusicaSecondaryEffectKind::SnowVertical => {
                        advance_snow_v_particle(particle, elapsed_ms)?;
                        if particle.position[1] >= MUSICA_SNOW_H_STAGE_HEIGHT {
                            initialize_snow_v_particle(particle, &mut random_state)?;
                        }
                    }
                    MusicaSecondaryEffectKind::SnowHorizontal => {
                        advance_snow_h_particle(particle, elapsed_ms)?;
                        if particle.position[0] >= MUSICA_SNOW_H_STAGE_WIDTH {
                            initialize_snow_h_particle(particle, &mut random_state)?;
                        }
                    }
                }
            }
        }
        self.state.random_state = random_state;
        if clear {
            self.state.secondary_effect = None;
            let sequence = next_effect_sequence(&mut self.state)?;
            return Ok(Some(MusicaVmEvent::SecondaryEffectCleared { sequence }));
        }
        if !changed {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaVmEvent::SecondaryEffect(
            MusicaSecondaryEffectFrame { sequence },
        )))
    }
}
pub(super) fn execute_secondary_effect(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::SecondaryEffect)?;
    let [kind] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::SecondaryEffect);
    };
    match kind.as_str() {
        "SnowH" => {
            state.secondary_effect = Some(new_snow_h_state(&mut state.random_state)?);
        }
        "Snow" => {
            state.secondary_effect = Some(new_snow_v_state(&mut state.random_state)?);
        }
        "fadeout" => {
            let effect = state
                .secondary_effect
                .as_mut()
                .ok_or(MusicaRuntimeError::SecondaryEffect)?;
            if matches!(
                effect.kind,
                MusicaSecondaryEffectKind::SnowHorizontal | MusicaSecondaryEffectKind::SnowVertical
            ) {
                effect.ending = true;
            } else {
                return Err(MusicaRuntimeError::SecondaryEffect);
            }
        }
        _ => {
            tracing::info!(
                target: "astra_emu_musica::runtime",
                event = "astra_emu_musica_secondary_effect_kind_unsupported",
                effect_identity = %Hash256::from_sha256(kind.as_bytes()),
                "Musica secondary effect kind is not implemented"
            );
            return Err(MusicaRuntimeError::SecondaryEffect);
        }
    }
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::SecondaryEffect(
        MusicaSecondaryEffectFrame { sequence },
    )))
}
pub(super) fn execute_primary(
    tokens: &[String],
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    if tokens[0] == "Snow" {
        // ef's opening scenes drive the vertical snowfall through the primary
        // effect slot.  The verified secondary snow pool provides the same
        // presentation; the primary crossfade slot itself stays free.
        state.secondary_effect = Some(new_snow_v_state(&mut state.random_state)?);
        let sequence = next_effect_sequence(state)?;
        return Ok(Some(MusicaVmEvent::SecondaryEffect(
            MusicaSecondaryEffectFrame { sequence },
        )));
    }
    if tokens[0] == "end" {
        if tokens.len() != 1 {
            return Err(MusicaRuntimeError::Firefly);
        }
        if let Some(firefly) = state.firefly.as_mut() {
            firefly.ending = true;
            next_effect_sequence(state)?;
            return Ok(Some(MusicaVmEvent::Firefly(MusicaFireflyFrame {
                sequence: state.effect_sequence,
            })));
        }
        state.effect = None;
        state.wscroll2 = None;
        next_effect_sequence(state)?;
        return Ok(Some(MusicaVmEvent::EffectCleared));
    }
    if tokens[0] == "fadeout" {
        if tokens.len() != 1 {
            return Err(MusicaRuntimeError::Firefly);
        }
        let firefly = state.firefly.as_mut().ok_or(MusicaRuntimeError::Firefly)?;
        firefly.ending = true;
        next_effect_sequence(state)?;
        return Ok(Some(MusicaVmEvent::Firefly(MusicaFireflyFrame {
            sequence: state.effect_sequence,
        })));
    }
    if tokens[0] == "Firefly" {
        if tokens.len() != 4 {
            return Err(MusicaRuntimeError::Firefly);
        }
        validate_scene_filename(&tokens[1]).map_err(|_| MusicaRuntimeError::Firefly)?;
        let target_count = tokens[2]
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=MUSICA_FIREFLY_MAX_PARTICLES_U32).contains(value))
            .ok_or(MusicaRuntimeError::Firefly)?;
        let duration_ms = tokens[3]
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=MUSICA_FIREFLY_MAX_DURATION_MS).contains(value))
            .ok_or(MusicaRuntimeError::Firefly)?;
        state.effect = None;
        state.wscroll2 = None;
        let firefly = new_firefly_state(
            &tokens[1],
            target_count,
            duration_ms,
            &mut state.random_state,
        )?;
        state.firefly = Some(firefly);
        next_effect_sequence(state)?;
        return Ok(Some(MusicaVmEvent::Firefly(MusicaFireflyFrame {
            sequence: state.effect_sequence,
        })));
    }
    Err(MusicaRuntimeError::Effect)
}
pub(crate) fn validate_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if usize::from(state.effect.is_some())
        + usize::from(state.firefly.is_some())
        + usize::from(state.wscroll2.is_some())
        > 1
    {
        return Err(MusicaRuntimeError::State);
    }
    if let Some(effect) = &state.secondary_effect {
        validate_secondary_effect_state(effect)?;
    }
    let Some(firefly) = state.firefly.as_ref() else {
        return Ok(());
    };
    if state.effect.is_some()
        || !(1..=MUSICA_FIREFLY_MAX_PARTICLES_U32).contains(&firefly.target_count)
        || usize::try_from(firefly.target_count).map_err(|_| MusicaRuntimeError::Overflow)?
            != firefly.particles.len()
        || firefly.particles.len() > MUSICA_FIREFLY_MAX_PARTICLES
        || !(1..=MUSICA_FIREFLY_MAX_DURATION_MS).contains(&firefly.duration_ms)
        || firefly.fade_alpha_256 > MUSICA_FIREFLY_FADE_SCALE
        || firefly.fade_elapsed_ns >= MUSICA_FIREFLY_FADE_STEP_NS
    {
        return Err(MusicaRuntimeError::Firefly);
    }

    let suffixes = ["S.png", "M.png", "L.png"];
    let mut common_prefix: Option<&str> = None;
    for (resource, suffix) in firefly.resources.iter().zip(suffixes) {
        let filename = resource
            .strip_prefix("musica:/sys/")
            .ok_or(MusicaRuntimeError::Firefly)?;
        validate_scene_filename(filename).map_err(|_| MusicaRuntimeError::Firefly)?;
        let prefix = filename
            .strip_suffix(suffix)
            .filter(|prefix| !prefix.is_empty())
            .ok_or(MusicaRuntimeError::Firefly)?;
        if common_prefix
            .replace(prefix)
            .is_some_and(|value| value != prefix)
        {
            return Err(MusicaRuntimeError::Firefly);
        }
    }

    let min_x = -MUSICA_FIREFLY_STAGE_WIDTH / 2;
    let max_x = MUSICA_FIREFLY_STAGE_WIDTH + MUSICA_FIREFLY_STAGE_WIDTH / 2 - 1;
    let min_y = -MUSICA_FIREFLY_STAGE_HEIGHT / 2;
    let max_y = MUSICA_FIREFLY_STAGE_HEIGHT + MUSICA_FIREFLY_STAGE_HEIGHT / 2 - 1;
    for particle in &firefly.particles {
        if !particle.active
            || particle.kind >= 3
            || particle.lifetime_ns == 0
            || particle.elapsed_ns >= particle.lifetime_ns
            || particle.opacity_255 > 255
            || !(min_x..=max_x).contains(&particle.position[0])
            || !(min_y..=max_y).contains(&particle.position[1])
            || particle.control_points.iter().any(|point| {
                !(min_x..=max_x).contains(&point[0]) || !(min_y..=max_y).contains(&point[1])
            })
        {
            return Err(MusicaRuntimeError::Firefly);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
