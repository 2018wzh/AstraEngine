use super::*;
pub(super) fn new_snow_h_state(
    random_state: &mut u64,
) -> Result<MusicaSecondaryEffectState, MusicaRuntimeError> {
    let mut particles = Vec::with_capacity(MUSICA_SNOW_H_PARTICLE_COUNT);
    for _ in 0..MUSICA_SNOW_H_PARTICLE_COUNT {
        let mut particle = MusicaSnowHParticle {
            fixed_position: [0, 0],
            horizontal_velocity: 0,
            vertical_velocity: 0,
            vertical_positive: false,
            kind: 0,
            position: [0, 0],
            active: false,
        };
        initialize_snow_h_particle(&mut particle, random_state)?;
        particles.push(particle);
    }
    Ok(MusicaSecondaryEffectState {
        kind: MusicaSecondaryEffectKind::SnowHorizontal,
        resources: [
            "musica:/sys/snowS.png".into(),
            "musica:/sys/snowM.png".into(),
            "musica:/sys/snowL.png".into(),
        ],
        ending: false,
        alpha_256: 0,
        fade_elapsed_ns: 0,
        motion_elapsed_ns: 0,
        particles,
    })
}

pub(super) fn new_snow_v_state(
    random_state: &mut u64,
) -> Result<MusicaSecondaryEffectState, MusicaRuntimeError> {
    let mut particles = Vec::with_capacity(MUSICA_SNOW_H_PARTICLE_COUNT);
    for _ in 0..MUSICA_SNOW_H_PARTICLE_COUNT {
        let mut particle = MusicaSnowHParticle {
            fixed_position: [0, 0],
            horizontal_velocity: 0,
            vertical_velocity: 0,
            vertical_positive: false,
            kind: 0,
            position: [0, 0],
            active: false,
        };
        initialize_snow_v_particle(&mut particle, random_state)?;
        particles.push(particle);
    }
    Ok(MusicaSecondaryEffectState {
        kind: MusicaSecondaryEffectKind::SnowVertical,
        resources: [
            "musica:/sys/snowS.png".into(),
            "musica:/sys/snowM.png".into(),
            "musica:/sys/snowL.png".into(),
        ],
        ending: false,
        alpha_256: 0,
        fade_elapsed_ns: 0,
        motion_elapsed_ns: 0,
        particles,
    })
}

pub(super) fn initialize_snow_v_particle(
    particle: &mut MusicaSnowHParticle,
    random_state: &mut u64,
) -> Result<(), MusicaRuntimeError> {
    let x = next_native_random_15(random_state)
        % u32::try_from(MUSICA_SNOW_H_STAGE_WIDTH + 100)
            .map_err(|_| MusicaRuntimeError::Overflow)?;
    let y = next_native_random_15(random_state)
        % u32::try_from(MUSICA_SNOW_H_STAGE_HEIGHT + 60)
            .map_err(|_| MusicaRuntimeError::Overflow)?;
    let vertical_velocity = next_native_random_15(random_state)
        .checked_mul(2)
        .and_then(|value| value.checked_add(0x4000))
        .ok_or(MusicaRuntimeError::Overflow)?;
    let horizontal_velocity = next_native_random_15(random_state) % 0x2000;
    let horizontal_positive = next_native_random_15(random_state) > 0x3fff;
    let kind = if vertical_velocity < 0x5000 {
        0
    } else if vertical_velocity < 0xa000 {
        1
    } else {
        2
    };
    particle.fixed_position = [i64::from(x) << 16, i64::from(y) << 16];
    particle.horizontal_velocity = horizontal_velocity;
    particle.vertical_velocity = vertical_velocity;
    particle.vertical_positive = horizontal_positive;
    particle.kind = kind;
    particle.position = snow_h_visible_position(particle.fixed_position)?;
    particle.active = true;
    Ok(())
}

pub(super) fn advance_snow_v_particle(
    particle: &mut MusicaSnowHParticle,
    elapsed_ms: u64,
) -> Result<(), MusicaRuntimeError> {
    if !particle.active {
        return Err(MusicaRuntimeError::SecondaryEffect);
    }
    let horizontal_delta = u64::from(particle.horizontal_velocity)
        .checked_mul(elapsed_ms)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(MusicaRuntimeError::Overflow)?;
    let vertical_delta = u64::from(particle.vertical_velocity)
        .checked_mul(elapsed_ms)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(MusicaRuntimeError::Overflow)?;
    particle.fixed_position[0] = if particle.vertical_positive {
        particle.fixed_position[0].checked_add(horizontal_delta)
    } else {
        particle.fixed_position[0].checked_sub(horizontal_delta)
    }
    .ok_or(MusicaRuntimeError::Overflow)?;
    particle.fixed_position[1] = particle.fixed_position[1]
        .checked_add(vertical_delta)
        .ok_or(MusicaRuntimeError::Overflow)?;
    particle.position = snow_h_visible_position(particle.fixed_position)?;
    Ok(())
}

pub(super) fn initialize_snow_h_particle(
    particle: &mut MusicaSnowHParticle,
    random_state: &mut u64,
) -> Result<(), MusicaRuntimeError> {
    let x = next_native_random_15(random_state)
        % u32::try_from(MUSICA_SNOW_H_STAGE_WIDTH + 100)
            .map_err(|_| MusicaRuntimeError::Overflow)?;
    let y = next_native_random_15(random_state)
        % u32::try_from(MUSICA_SNOW_H_STAGE_HEIGHT + 60)
            .map_err(|_| MusicaRuntimeError::Overflow)?;
    let horizontal_velocity = next_native_random_15(random_state)
        .checked_mul(2)
        .and_then(|value| value.checked_add(0x4000))
        .ok_or(MusicaRuntimeError::Overflow)?;
    let vertical_velocity = next_native_random_15(random_state) % 0x2000;
    let vertical_positive = next_native_random_15(random_state) > 0x3fff;
    let kind = if horizontal_velocity < 0x5000 {
        0
    } else if horizontal_velocity < 0xa000 {
        1
    } else {
        2
    };
    particle.fixed_position = [i64::from(x) << 16, i64::from(y) << 16];
    particle.horizontal_velocity = horizontal_velocity;
    particle.vertical_velocity = vertical_velocity;
    particle.vertical_positive = vertical_positive;
    particle.kind = kind;
    particle.position = snow_h_visible_position(particle.fixed_position)?;
    particle.active = true;
    Ok(())
}

pub(super) fn advance_snow_h_particle(
    particle: &mut MusicaSnowHParticle,
    elapsed_ms: u64,
) -> Result<(), MusicaRuntimeError> {
    if !particle.active {
        return Err(MusicaRuntimeError::SecondaryEffect);
    }
    let horizontal_delta = u64::from(particle.horizontal_velocity)
        .checked_mul(elapsed_ms)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(MusicaRuntimeError::Overflow)?;
    let vertical_delta = u64::from(particle.vertical_velocity)
        .checked_mul(elapsed_ms)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(MusicaRuntimeError::Overflow)?;
    particle.fixed_position[0] = particle.fixed_position[0]
        .checked_add(horizontal_delta)
        .ok_or(MusicaRuntimeError::Overflow)?;
    particle.fixed_position[1] = if particle.vertical_positive {
        particle.fixed_position[1].checked_add(vertical_delta)
    } else {
        particle.fixed_position[1].checked_sub(vertical_delta)
    }
    .ok_or(MusicaRuntimeError::Overflow)?;
    particle.position = snow_h_visible_position(particle.fixed_position)?;
    Ok(())
}

pub(super) fn snow_h_visible_position(fixed: [i64; 2]) -> Result<[i32; 2], MusicaRuntimeError> {
    let x = (fixed[0] >> 16)
        .checked_sub(50)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(MusicaRuntimeError::Overflow)?;
    let y = (fixed[1] >> 16)
        .checked_sub(30)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(MusicaRuntimeError::Overflow)?;
    Ok([x, y])
}

pub(super) fn validate_secondary_effect_state(
    effect: &MusicaSecondaryEffectState,
) -> Result<(), MusicaRuntimeError> {
    let expected_resources = [
        "musica:/sys/snowS.png",
        "musica:/sys/snowM.png",
        "musica:/sys/snowL.png",
    ];
    if !matches!(
        effect.kind,
        MusicaSecondaryEffectKind::SnowHorizontal | MusicaSecondaryEffectKind::SnowVertical
    ) || effect
        .resources
        .iter()
        .map(String::as_str)
        .ne(expected_resources)
        || effect.particles.len() != MUSICA_SNOW_H_PARTICLE_COUNT
        || effect.alpha_256 > MUSICA_SNOW_H_FADE_SCALE
        || effect.fade_elapsed_ns >= MUSICA_SNOW_H_FADE_STEP_NS
        || effect.motion_elapsed_ns >= 1_000_000
    {
        return Err(MusicaRuntimeError::SecondaryEffect);
    }
    for particle in &effect.particles {
        let (
            forward_velocity,
            sideways_velocity,
            forward_position,
            sideways_position,
            max_position,
        ) = match effect.kind {
            MusicaSecondaryEffectKind::SnowHorizontal => (
                particle.horizontal_velocity,
                particle.vertical_velocity,
                particle.position[0],
                particle.position[1],
                MUSICA_SNOW_H_STAGE_WIDTH + 50,
            ),
            MusicaSecondaryEffectKind::SnowVertical => (
                particle.vertical_velocity,
                particle.horizontal_velocity,
                particle.position[1],
                particle.position[0],
                MUSICA_SNOW_H_STAGE_HEIGHT + 30,
            ),
        };
        let min_position = match effect.kind {
            MusicaSecondaryEffectKind::SnowHorizontal => -50,
            MusicaSecondaryEffectKind::SnowVertical => -30,
        };
        let expected_kind = if forward_velocity < 0x5000 {
            0
        } else if forward_velocity < 0xa000 {
            1
        } else {
            2
        };
        let visible = snow_h_visible_position(particle.fixed_position)
            .map_err(|_| MusicaRuntimeError::SecondaryEffect)?;
        if !particle.active
            || !(0x4000..=0x13ffe).contains(&forward_velocity)
            || sideways_velocity >= 0x2000
            || particle.kind != expected_kind
            || particle.position != visible
            || !(min_position..max_position).contains(&forward_position)
            || !(-2048..=2048).contains(&sideways_position)
        {
            return Err(MusicaRuntimeError::SecondaryEffect);
        }
    }
    Ok(())
}
