use super::*;
impl MusicaVm {
    pub fn advance_linear_scroll_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaLinearScrollFrame>, MusicaRuntimeError> {
        let (linear_scroll, stage) = (&mut self.state.linear_scroll, &mut self.state.stage);
        let Some(scroll) = linear_scroll.as_mut() else {
            return Ok(None);
        };
        if scroll.completed {
            return Ok(None);
        }
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let previous = scroll.current;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?
            .min(duration_ns);
        scroll.current = linear_scroll_visible_position(scroll)?;
        scroll.completed = scroll.elapsed_ns == duration_ns;
        set_stage_position(
            stage.as_mut().ok_or(MusicaRuntimeError::LinearScroll)?,
            scroll.current,
        )?;
        if scroll.current == previous && !scroll.completed {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaLinearScrollFrame { sequence }))
    }
}
pub(in crate::runtime) fn execute_linear_scroll(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::LinearScroll)?;
    let [target_x, target_y, speed_tenths] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::LinearScroll);
    };
    if state.wscroll2.is_some()
        || state.scroll_xf.is_some()
        || state.axis_scroll.is_some()
        || state
            .linear_scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.completed)
    {
        return Err(MusicaRuntimeError::LinearScroll);
    }
    let parse_coordinate = |value: &str| {
        value
            .parse::<i32>()
            .ok()
            .filter(|value| value.unsigned_abs() <= MUSICA_AXIS_SCROLL_MAX_COORDINATE as u32)
            .ok_or(MusicaRuntimeError::LinearScroll)
    };
    let target = [parse_coordinate(target_x)?, parse_coordinate(target_y)?];
    let speed_tenths = speed_tenths
        .parse::<u32>()
        .ok()
        .filter(|value| *value != 0 && *value <= MUSICA_AXIS_SCROLL_MAX_SPEED_TENTHS as u32)
        .ok_or(MusicaRuntimeError::LinearScroll)?;
    let stage = state
        .stage
        .as_ref()
        .ok_or(MusicaRuntimeError::LinearScroll)?;
    let start = stage_position(stage)?;
    let duration_ms = linear_scroll_duration_ms(start, target, speed_tenths)?;
    let completed = start == target;
    let scroll = MusicaLinearScrollState {
        start,
        target,
        speed_tenths,
        duration_ms,
        elapsed_ns: 0,
        current: start,
        completed,
    };
    validate_linear_scroll_state(&scroll, stage)?;
    state.linear_scroll = Some(scroll);
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::LinearScroll(MusicaLinearScrollFrame {
        sequence,
    })))
}
fn stage_position(stage: &MusicaStageCommand) -> Result<[i32; 2], MusicaRuntimeError> {
    let background = stage
        .background
        .as_ref()
        .ok_or(MusicaRuntimeError::LinearScroll)?;
    Ok([background.x, background.y])
}
fn set_stage_position(
    stage: &mut MusicaStageCommand,
    value: [i32; 2],
) -> Result<(), MusicaRuntimeError> {
    let background = stage
        .background
        .as_mut()
        .ok_or(MusicaRuntimeError::LinearScroll)?;
    background.x = value[0];
    background.y = value[1];
    Ok(())
}
fn linear_scroll_duration_ms(
    start: [i32; 2],
    target: [i32; 2],
    speed_tenths: u32,
) -> Result<u32, MusicaRuntimeError> {
    if speed_tenths == 0 {
        return Err(MusicaRuntimeError::LinearScroll);
    }
    let distance = start
        .iter()
        .zip(target)
        .map(|(start, target)| (i64::from(target) - i64::from(*start)).unsigned_abs())
        .max()
        .unwrap_or(0);
    if distance == 0 {
        return Ok(0);
    }
    let numerator = distance
        .checked_mul(10)
        .ok_or(MusicaRuntimeError::Overflow)?;
    u32::try_from(numerator.div_ceil(u64::from(speed_tenths)))
        .map_err(|_| MusicaRuntimeError::Overflow)
}
fn linear_scroll_visible_position(
    scroll: &MusicaLinearScrollState,
) -> Result<[i32; 2], MusicaRuntimeError> {
    let distance = scroll
        .start
        .iter()
        .zip(scroll.target)
        .map(|(start, target)| (i64::from(target) - i64::from(*start)).unsigned_abs())
        .max()
        .unwrap_or(0);
    if distance == 0 {
        return Ok(scroll.target);
    }
    let elapsed_ms = scroll.elapsed_ns / 1_000_000;
    let travelled = elapsed_ms
        .checked_mul(u64::from(scroll.speed_tenths))
        .ok_or(MusicaRuntimeError::Overflow)?
        / 10;
    let travelled = travelled.min(distance);
    let interpolate = |start: i32, target: i32| {
        let delta = i128::from(target) - i128::from(start);
        let offset = delta
            .checked_mul(i128::from(travelled))
            .ok_or(MusicaRuntimeError::Overflow)?
            / i128::from(distance);
        i32::try_from(i128::from(start) + offset).map_err(|_| MusicaRuntimeError::Overflow)
    };
    Ok([
        interpolate(scroll.start[0], scroll.target[0])?,
        interpolate(scroll.start[1], scroll.target[1])?,
    ])
}
pub(super) fn complete_linear_scroll_state(
    state: &mut MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    let (linear_scroll, stage) = (&mut state.linear_scroll, &mut state.stage);
    let scroll = linear_scroll
        .as_mut()
        .ok_or(MusicaRuntimeError::LinearScroll)?;
    scroll.elapsed_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    scroll.current = scroll.target;
    scroll.completed = true;
    set_stage_position(
        stage.as_mut().ok_or(MusicaRuntimeError::LinearScroll)?,
        scroll.target,
    )
}
pub(super) fn validate_linear_scroll_state(
    scroll: &MusicaLinearScrollState,
    stage: &MusicaStageCommand,
) -> Result<(), MusicaRuntimeError> {
    if scroll
        .start
        .iter()
        .chain(&scroll.target)
        .chain(&scroll.current)
        .any(|value| value.unsigned_abs() > MUSICA_AXIS_SCROLL_MAX_COORDINATE as u32)
        || scroll.speed_tenths == 0
        || scroll.speed_tenths > MUSICA_AXIS_SCROLL_MAX_SPEED_TENTHS as u32
        || scroll.duration_ms
            != linear_scroll_duration_ms(scroll.start, scroll.target, scroll.speed_tenths)?
    {
        return Err(MusicaRuntimeError::LinearScroll);
    }
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    if scroll.elapsed_ns > duration_ns
        || scroll.completed != (scroll.elapsed_ns == duration_ns)
        || scroll.current != linear_scroll_visible_position(scroll)?
        || stage_position(stage)? != scroll.current
    {
        return Err(MusicaRuntimeError::LinearScroll);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_sc, ScOpcodeCatalog};
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/linear.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    #[test]
    fn linear_scroll_interpolates_restores_and_waits_for_completion() {
        let source = b".stage * BG.png 0 0\r\n.scroll 8 -4 1\r\n.endscroll false\r\n.end\r\n";
        let mut original = vm(source);
        original.step(1).unwrap();
        original.step(2).unwrap();
        original.advance_linear_scroll_clock(40_500_000).unwrap();
        assert_eq!(
            original.state().linear_scroll.as_ref().unwrap().current,
            [4, -2]
        );
        assert!(matches!(
            original.step(3).unwrap(),
            Some(MusicaVmEvent::Wait(MusicaWaitState::LinearScroll { .. }))
        ));
        let mut restored = vm(source);
        restored
            .restore_native_save(&original.encode_native_save().unwrap(), 4)
            .unwrap();
        for delta in [500_000, 39_000_000] {
            assert_eq!(
                original.advance_linear_scroll_clock(delta).unwrap(),
                restored.advance_linear_scroll_clock(delta).unwrap()
            );
            assert_eq!(
                original.state().linear_scroll,
                restored.state().linear_scroll
            );
            assert_eq!(original.state().stage, restored.state().stage);
        }
        assert!(original.state().linear_scroll.as_ref().unwrap().completed);
        assert_eq!(
            original.state().linear_scroll.as_ref().unwrap().current,
            [8, -4]
        );
    }
    #[test]
    fn linear_scroll_force_finish_replacement_and_corrupt_state() {
        let source=b".stage * BG.png 0 0\r\n.scroll 8 4 1\r\n.endscroll true\r\n.stage * BG.png 0 0\r\n.end\r\n";
        let mut machine = vm(source);
        machine.step(1).unwrap();
        machine.step(2).unwrap();
        let valid = machine.encode_native_save().unwrap();
        for mutation in 0..3 {
            let mut state = machine.state().clone();
            let scroll = state.linear_scroll.as_mut().unwrap();
            match mutation {
                0 => scroll.current = [1, 0],
                1 => scroll.speed_tenths = 0,
                _ => scroll.elapsed_ns = u64::MAX,
            }
            let bad = postcard::to_allocvec(&state).unwrap();
            assert_eq!(
                MusicaVm::decode_native_save(&bad).unwrap_err(),
                MusicaRuntimeError::LinearScroll
            );
            assert_eq!(
                machine.restore_native_save(&bad, 3).unwrap_err(),
                MusicaRuntimeError::LinearScroll
            );
            assert_eq!(machine.encode_native_save().unwrap(), valid);
        }
        machine.step(3).unwrap();
        assert_eq!(
            machine.state().linear_scroll.as_ref().unwrap().current,
            [8, 4]
        );
        machine.step(4).unwrap();
        assert!(machine.state().linear_scroll.is_none());
    }
}
