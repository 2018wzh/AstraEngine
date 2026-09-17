use super::*;
pub(super) fn next_firefly_random(state: &mut u64) -> u32 {
    // SplitMix64 is small, deterministic on every supported target, and keeps
    // the adapter independent from the process-global C rand() state used by
    // the original executable.
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (value ^ (value >> 31)) as u32
}

pub(super) fn next_native_random_15(state: &mut u64) -> u32 {
    next_firefly_random(state) & 0x7fff
}

pub(super) fn firefly_random_bounded(
    state: &mut u64,
    bound: u32,
) -> Result<i32, MusicaRuntimeError> {
    if bound == 0 {
        return Err(MusicaRuntimeError::Firefly);
    }
    Ok((next_firefly_random(state) % bound) as i32)
}

pub(super) fn respawn_firefly_particle(
    particle: &mut MusicaFireflyParticle,
    duration_ms: u32,
    random_state: &mut u64,
) -> Result<(), MusicaRuntimeError> {
    if duration_ms == 0 {
        return Err(MusicaRuntimeError::Firefly);
    }
    for point in &mut particle.control_points {
        let x = firefly_random_bounded(
            random_state,
            u32::try_from(MUSICA_FIREFLY_STAGE_WIDTH * 2)
                .map_err(|_| MusicaRuntimeError::Overflow)?,
        )? - MUSICA_FIREFLY_STAGE_WIDTH / 2;
        let y = firefly_random_bounded(
            random_state,
            u32::try_from(MUSICA_FIREFLY_STAGE_HEIGHT * 2)
                .map_err(|_| MusicaRuntimeError::Overflow)?,
        )? - MUSICA_FIREFLY_STAGE_HEIGHT / 2;
        *point = [x, y];
    }
    let lifetime_offset = next_firefly_random(random_state) % duration_ms;
    let lifetime_ms = duration_ms
        .checked_add(lifetime_offset)
        .and_then(|value| value.checked_sub(duration_ms / 2))
        .ok_or(MusicaRuntimeError::Overflow)?;
    particle.kind = (next_firefly_random(random_state) % 3) as u8;
    particle.elapsed_ns = 0;
    particle.lifetime_ns = u64::from(lifetime_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    particle.position = particle.control_points[0];
    particle.opacity_255 = 0;
    particle.active = true;
    Ok(())
}

pub(super) fn fixed_firefly_parameter(
    elapsed_ns: u64,
    lifetime_ns: u64,
) -> Result<u32, MusicaRuntimeError> {
    if lifetime_ns == 0 {
        return Err(MusicaRuntimeError::Firefly);
    }
    let numerator = u128::from(elapsed_ns.min(lifetime_ns))
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    u32::try_from(numerator / u128::from(lifetime_ns)).map_err(|_| MusicaRuntimeError::Overflow)
}

pub(super) fn firefly_curve_position(
    control_points: &[[i32; 2]; MUSICA_FIREFLY_CONTROL_POINTS],
    parameter: u32,
) -> Result<[i32; 2], MusicaRuntimeError> {
    const SCALE: i128 = 1_000_000;
    if parameter > SCALE as u32 {
        return Err(MusicaRuntimeError::Firefly);
    }
    let t = i128::from(parameter);
    let one_minus_t = SCALE - t;
    let mut points = control_points.map(|point| [i128::from(point[0]), i128::from(point[1])]);
    // De Casteljau is the exact degree-six Bernstein curve used by the
    // original seven control-point particle path, evaluated in fixed-point so
    // the snapshot hash is independent of a platform's floating-point mode.
    for level in 1..MUSICA_FIREFLY_CONTROL_POINTS {
        for index in 0..(MUSICA_FIREFLY_CONTROL_POINTS - level) {
            let (left, right) = points.split_at_mut(index + 1);
            let current = &mut left[index];
            let next = &right[0];
            for (value, next_value) in current.iter_mut().zip(next.iter()) {
                *value = ((*value)
                    .checked_mul(one_minus_t)
                    .and_then(|left| {
                        next_value
                            .checked_mul(t)
                            .and_then(|right| left.checked_add(right))
                    })
                    .ok_or(MusicaRuntimeError::Overflow)?)
                    / SCALE;
            }
        }
    }
    Ok([
        i32::try_from(points[0][0]).map_err(|_| MusicaRuntimeError::Overflow)?,
        i32::try_from(points[0][1]).map_err(|_| MusicaRuntimeError::Overflow)?,
    ])
}

pub(super) fn firefly_particle_opacity(parameter: u32) -> u16 {
    let value = if parameter < 255_000 {
        parameter.saturating_mul(255) / 255_000
    } else if parameter > 755_000 {
        (1_000_000u32.saturating_sub(parameter)).saturating_mul(255) / 245_000
    } else {
        255
    };
    value.min(255) as u16
}

pub(super) fn new_firefly_state(
    prefix: &str,
    target_count: u32,
    duration_ms: u32,
    random_state: &mut u64,
) -> Result<MusicaFireflyState, MusicaRuntimeError> {
    if !(1..=MUSICA_FIREFLY_MAX_PARTICLES_U32).contains(&target_count)
        || !(1..=MUSICA_FIREFLY_MAX_DURATION_MS).contains(&duration_ms)
    {
        return Err(MusicaRuntimeError::Firefly);
    }
    let resources = [
        format!("musica:/sys/{prefix}S.png"),
        format!("musica:/sys/{prefix}M.png"),
        format!("musica:/sys/{prefix}L.png"),
    ];
    let mut particles = Vec::with_capacity(target_count as usize);
    for _ in 0..target_count {
        let mut particle = MusicaFireflyParticle {
            control_points: [[0; 2]; MUSICA_FIREFLY_CONTROL_POINTS],
            kind: 0,
            elapsed_ns: 0,
            lifetime_ns: 0,
            position: [0, 0],
            opacity_255: 0,
            active: false,
        };
        respawn_firefly_particle(&mut particle, duration_ms, random_state)?;
        particles.push(particle);
    }
    Ok(MusicaFireflyState {
        resources,
        target_count,
        duration_ms,
        ending: false,
        fade_alpha_256: 0,
        fade_elapsed_ns: 0,
        particles,
    })
}
