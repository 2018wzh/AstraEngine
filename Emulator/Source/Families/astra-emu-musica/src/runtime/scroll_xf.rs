use super::*;
const MUSICA_SCROLL_XF_MAX_EXTENT: i32 = 16_384;
const MUSICA_SCROLL_XF_MAX_DURATION_MS: u32 = 60_000;
const MUSICA_SCROLL_XF_SCALE: u64 = 1_000_000;
impl MusicaVm {
    pub fn advance_scroll_xf_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaScrollXfFrame>, MusicaRuntimeError> {
        let Some(scroll) = self.state.scroll_xf.as_mut() else {
            return Ok(None);
        };
        if scroll.completed {
            return Ok(None);
        }
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?
            .min(duration_ns);
        let (visible_extent, visible_offset) = scroll_xf_visible_state(scroll)?;
        scroll.visible_extent = visible_extent;
        scroll.visible_offset = visible_offset;
        scroll.completed = scroll.elapsed_ns == duration_ns;
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaScrollXfFrame { sequence }))
    }
}
pub(super) fn execute_scroll_xf(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::ScrollXf)?;
    let [start_width, start_height, end_width, end_height, start_x, start_y, end_x, end_y, duration, easing] =
        tokens.as_slice()
    else {
        return Err(MusicaRuntimeError::ScrollXf);
    };
    if state.wscroll2.is_some()
        || state.stage.is_none()
        || state.linear_scroll.is_some()
        || state
            .axis_scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.completed)
    {
        return Err(MusicaRuntimeError::ScrollXf);
    }
    let parse_extent = |value: &str| {
        value
            .parse::<i32>()
            .ok()
            .filter(|value| (0..=MUSICA_SCROLL_XF_MAX_EXTENT).contains(value))
            .ok_or(MusicaRuntimeError::ScrollXf)
    };
    let duration_ms = duration
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=MUSICA_SCROLL_XF_MAX_DURATION_MS).contains(value))
        .ok_or(MusicaRuntimeError::ScrollXf)?;
    let easing = easing
        .parse::<u8>()
        .ok()
        .filter(|value| *value <= 2)
        .ok_or(MusicaRuntimeError::ScrollXf)?;
    let scroll = MusicaScrollXfState {
        start_extent: [parse_extent(start_width)?, parse_extent(start_height)?],
        end_extent: [parse_extent(end_width)?, parse_extent(end_height)?],
        start_offset: [parse_extent(start_x)?, parse_extent(start_y)?],
        end_offset: [parse_extent(end_x)?, parse_extent(end_y)?],
        duration_ms,
        easing,
        elapsed_ns: 0,
        completed: false,
        visible_extent: [parse_extent(start_width)?, parse_extent(start_height)?],
        visible_offset: [parse_extent(start_x)?, parse_extent(start_y)?],
    };
    validate_scroll_xf_state(&scroll)?;
    state.axis_scroll = None;
    state.scroll_xf = Some(scroll);
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::ScrollXf(MusicaScrollXfFrame {
        sequence,
    })))
}
pub(crate) fn validate_scroll_xf_state(
    scroll: &MusicaScrollXfState,
) -> Result<(), MusicaRuntimeError> {
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    if !(1..=MUSICA_SCROLL_XF_MAX_DURATION_MS).contains(&scroll.duration_ms)
        || scroll.easing > 2
        || scroll.elapsed_ns > duration_ns
        || scroll.completed != (scroll.elapsed_ns == duration_ns)
        || scroll
            .start_extent
            .iter()
            .chain(scroll.end_extent.iter())
            .chain(scroll.start_offset.iter())
            .chain(scroll.end_offset.iter())
            .any(|value| !(0..=MUSICA_SCROLL_XF_MAX_EXTENT).contains(value))
    {
        return Err(MusicaRuntimeError::ScrollXf);
    }
    let (visible_extent, visible_offset) = scroll_xf_visible_state(scroll)?;
    if scroll.visible_extent != visible_extent || scroll.visible_offset != visible_offset {
        return Err(MusicaRuntimeError::ScrollXf);
    }
    Ok(())
}
fn scroll_xf_visible_state(
    scroll: &MusicaScrollXfState,
) -> Result<([i32; 2], [i32; 2]), MusicaRuntimeError> {
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    let linear = u64::try_from(
        u128::from(scroll.elapsed_ns)
            .checked_mul(u128::from(MUSICA_SCROLL_XF_SCALE))
            .ok_or(MusicaRuntimeError::Overflow)?
            / u128::from(duration_ns),
    )
    .map_err(|_| MusicaRuntimeError::Overflow)?
    .min(MUSICA_SCROLL_XF_SCALE);
    let squared = u64::try_from(
        u128::from(linear)
            .checked_mul(u128::from(linear))
            .ok_or(MusicaRuntimeError::Overflow)?
            / u128::from(MUSICA_SCROLL_XF_SCALE),
    )
    .map_err(|_| MusicaRuntimeError::Overflow)?;
    let eased = match scroll.easing {
        0 => linear,
        1 => squared,
        2 => linear
            .checked_mul(2)
            .and_then(|value| value.checked_sub(squared))
            .ok_or(MusicaRuntimeError::Overflow)?,
        _ => return Err(MusicaRuntimeError::ScrollXf),
    };
    Ok((
        interpolate_scroll_pair(scroll.start_extent, scroll.end_extent, eased)?,
        interpolate_scroll_pair(scroll.start_offset, scroll.end_offset, eased)?,
    ))
}
fn interpolate_scroll_pair(
    start: [i32; 2],
    end: [i32; 2],
    t: u64,
) -> Result<[i32; 2], MusicaRuntimeError> {
    let interpolate = |start: i32, end: i32| {
        let delta = i64::from(end) - i64::from(start);
        let scaled = i128::from(delta)
            .checked_mul(i128::from(t))
            .ok_or(MusicaRuntimeError::Overflow)?
            / i128::from(MUSICA_SCROLL_XF_SCALE);
        i32::try_from(i128::from(start) + scaled).map_err(|_| MusicaRuntimeError::Overflow)
    };
    Ok([
        interpolate(start[0], end[0])?,
        interpolate(start[1], end[1])?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_sc, ScOpcodeCatalog};
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/xf.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    #[test]
    fn scrollxf_easing_restore_and_force_finish() {
        for (easing, extent, offset) in [(0, 12, 4), (1, 14, 2), (2, 10, 6)] {
            let source=format!(".stage * BG.png 0 0\r\n.scrollxf 16 16 8 8 0 0 8 8 100 {easing}\r\n.endscroll true\r\n.end\r\n");
            let mut original = vm(source.as_bytes());
            original.step(1).unwrap();
            original.step(2).unwrap();
            original.advance_scroll_xf_clock(50_000_000).unwrap();
            let state = original.state().scroll_xf.as_ref().unwrap();
            assert_eq!(state.visible_extent, [extent, extent]);
            assert_eq!(state.visible_offset, [offset, offset]);
            let mut restored = vm(source.as_bytes());
            restored
                .restore_native_save(&original.encode_native_save().unwrap(), 3)
                .unwrap();
            assert_eq!(
                original.advance_scroll_xf_clock(10_000_000).unwrap(),
                restored.advance_scroll_xf_clock(10_000_000).unwrap()
            );
            assert_eq!(original.state().scroll_xf, restored.state().scroll_xf);
            original.step(3).unwrap();
            let state = original.state().scroll_xf.as_ref().unwrap();
            assert!(state.completed);
            assert_eq!(state.visible_extent, [8, 8]);
            assert_eq!(state.visible_offset, [8, 8]);
        }
    }
    #[test]
    fn scrollxf_survives_script_chain_until_stage_replacement() {
        let mut machine = vm(b".stage * BG.png 0 0\r\n.scrollxf 16 16 8 8 0 0 8 8 100 0\r\n");
        machine.step(1).unwrap();
        machine.step(2).unwrap();
        machine.advance_scroll_xf_clock(50_000_000).unwrap();
        let previous = machine.state().scroll_xf.clone();
        let source = b".stage * NEXT.png 0 0\r\n";
        machine
            .replace_script(
                "musica:/scr/next.sc".into(),
                Hash256::from_sha256(source),
                parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
                None,
            )
            .unwrap();
        assert_eq!(machine.state().scroll_xf, previous);
        machine.advance_scroll_xf_clock(25_000_000).unwrap();
        assert_eq!(
            machine.state().scroll_xf.as_ref().unwrap().visible_offset,
            [6, 6]
        );
        let saved = machine.encode_native_save().unwrap();
        assert_eq!(
            MusicaVm::decode_native_save(&saved).unwrap().scroll_xf,
            machine.state().scroll_xf
        );
        machine.step(3).unwrap();
        assert!(machine.state().scroll_xf.is_none());
    }
    #[test]
    fn scrollxf_corrupt_state_and_invalid_replacement_are_rejected() {
        let mut machine=vm(b".stage * BG.png 0 0\r\n.scrollxf 16 16 8 8 0 0 8 8 100 0\r\n.scrollxf 1 1 1 1 1 1 1 1 0 0\r\n");
        machine.step(1).unwrap();
        machine.step(2).unwrap();
        let previous = machine.state().scroll_xf.clone();
        assert_eq!(machine.step(3).unwrap_err(), MusicaRuntimeError::ScrollXf);
        assert_eq!(machine.state().scroll_xf, previous);
        for mutation in 0..3 {
            let mut state = machine.state().clone();
            let xf = state.scroll_xf.as_mut().unwrap();
            match mutation {
                0 => xf.duration_ms = 0,
                1 => xf.visible_extent = [0, 0],
                _ => xf.easing = 3,
            }
            let bytes = postcard::to_allocvec(&state).unwrap();
            assert_eq!(
                MusicaVm::decode_native_save(&bytes).unwrap_err(),
                MusicaRuntimeError::ScrollXf
            );
        }
    }
}
