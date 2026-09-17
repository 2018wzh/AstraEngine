mod linear;
use super::*;
pub(super) use linear::execute_linear_scroll;
const MUSICA_AXIS_SCROLL_MAX_COORDINATE: i32 = 65_536;
const MUSICA_AXIS_SCROLL_MAX_SPEED_TENTHS: i32 = 10_000;
impl MusicaVm {
    pub fn advance_axis_scroll_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaAxisScrollFrame>, MusicaRuntimeError> {
        let (axis_scroll, stage) = (&mut self.state.axis_scroll, &mut self.state.stage);
        let Some(scroll) = axis_scroll.as_mut() else {
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
        scroll.current = axis_scroll_visible_position(scroll)?;
        scroll.completed = scroll.elapsed_ns == duration_ns;
        set_stage_axis_position(
            stage.as_mut().ok_or(MusicaRuntimeError::AxisScroll)?,
            scroll.axis,
            scroll.current,
        )?;
        if scroll.current == previous && !scroll.completed {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaAxisScrollFrame { sequence }))
    }
}
pub(super) fn execute_axis_scroll(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
    axis: MusicaAxisScrollAxis,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = command
        .tokens()
        .map_err(|_| MusicaRuntimeError::AxisScroll)?;
    if tokens.len() > 2 {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    if state.wscroll2.is_some()
        || state.scroll_xf.is_some()
        || state.linear_scroll.is_some()
        || state
            .axis_scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.completed)
    {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    let target = tokens
        .first()
        .map_or(Ok(0), |value| value.parse::<i32>())
        .map_err(|_| MusicaRuntimeError::AxisScroll)?;
    let speed_tenths = tokens
        .get(1)
        .map_or(Ok(10), |value| value.parse::<i32>())
        .map_err(|_| MusicaRuntimeError::AxisScroll)?;
    if !(-MUSICA_AXIS_SCROLL_MAX_COORDINATE..=MUSICA_AXIS_SCROLL_MAX_COORDINATE).contains(&target)
        || speed_tenths == 0
        || !(-MUSICA_AXIS_SCROLL_MAX_SPEED_TENTHS..=MUSICA_AXIS_SCROLL_MAX_SPEED_TENTHS)
            .contains(&speed_tenths)
    {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    let stage = state.stage.as_ref().ok_or(MusicaRuntimeError::AxisScroll)?;
    let start = stage_axis_position(stage, axis)?;
    if !(-MUSICA_AXIS_SCROLL_MAX_COORDINATE..=MUSICA_AXIS_SCROLL_MAX_COORDINATE).contains(&start)
        || (start < target && speed_tenths < 0)
        || (start > target && speed_tenths > 0)
    {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    let duration_ms = axis_scroll_duration_ms(start, target, speed_tenths)?;
    let completed = start == target;
    let scroll = MusicaAxisScrollState {
        axis,
        start,
        target,
        speed_tenths,
        duration_ms,
        elapsed_ns: 0,
        current: start,
        completed,
    };
    validate_axis_scroll_state(&scroll, stage)?;
    state.axis_scroll = Some(scroll);
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::AxisScroll(MusicaAxisScrollFrame {
        sequence,
    })))
}
fn stage_axis_position(
    stage: &MusicaStageCommand,
    axis: MusicaAxisScrollAxis,
) -> Result<i32, MusicaRuntimeError> {
    let background = stage
        .background
        .as_ref()
        .ok_or(MusicaRuntimeError::AxisScroll)?;
    Ok(match axis {
        MusicaAxisScrollAxis::Horizontal => background.x,
        MusicaAxisScrollAxis::Vertical => background.y,
    })
}
fn set_stage_axis_position(
    stage: &mut MusicaStageCommand,
    axis: MusicaAxisScrollAxis,
    value: i32,
) -> Result<(), MusicaRuntimeError> {
    let background = stage
        .background
        .as_mut()
        .ok_or(MusicaRuntimeError::AxisScroll)?;
    match axis {
        MusicaAxisScrollAxis::Horizontal => background.x = value,
        MusicaAxisScrollAxis::Vertical => background.y = value,
    }
    Ok(())
}
fn axis_scroll_duration_ms(
    start: i32,
    target: i32,
    speed_tenths: i32,
) -> Result<u32, MusicaRuntimeError> {
    if speed_tenths == 0 {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    let distance = (i64::from(target) - i64::from(start)).unsigned_abs();
    if distance == 0 {
        return Ok(0);
    }
    let numerator = distance
        .checked_mul(10)
        .ok_or(MusicaRuntimeError::Overflow)?;
    let speed = u64::from(speed_tenths.unsigned_abs());
    u32::try_from(numerator.div_ceil(speed)).map_err(|_| MusicaRuntimeError::Overflow)
}
fn axis_scroll_visible_position(scroll: &MusicaAxisScrollState) -> Result<i32, MusicaRuntimeError> {
    if scroll.start == scroll.target {
        return Ok(scroll.target);
    }
    let elapsed_ms =
        i64::try_from(scroll.elapsed_ns / 1_000_000).map_err(|_| MusicaRuntimeError::Overflow)?;
    let delta = elapsed_ms
        .checked_mul(i64::from(scroll.speed_tenths))
        .ok_or(MusicaRuntimeError::Overflow)?
        / 10;
    let raw = i64::from(scroll.start)
        .checked_add(delta)
        .ok_or(MusicaRuntimeError::Overflow)?;
    let clamped = if scroll.speed_tenths > 0 {
        raw.min(i64::from(scroll.target))
    } else {
        raw.max(i64::from(scroll.target))
    };
    i32::try_from(clamped).map_err(|_| MusicaRuntimeError::Overflow)
}
fn complete_axis_scroll_state(state: &mut MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    let (axis_scroll, stage) = (&mut state.axis_scroll, &mut state.stage);
    let scroll = axis_scroll.as_mut().ok_or(MusicaRuntimeError::AxisScroll)?;
    scroll.elapsed_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    scroll.current = scroll.target;
    scroll.completed = true;
    set_stage_axis_position(
        stage.as_mut().ok_or(MusicaRuntimeError::AxisScroll)?,
        scroll.axis,
        scroll.target,
    )
}
fn validate_axis_scroll_state(
    scroll: &MusicaAxisScrollState,
    stage: &MusicaStageCommand,
) -> Result<(), MusicaRuntimeError> {
    if scroll
        .start
        .unsigned_abs()
        .max(scroll.target.unsigned_abs())
        .max(scroll.current.unsigned_abs())
        > MUSICA_AXIS_SCROLL_MAX_COORDINATE as u32
        || scroll.speed_tenths == 0
        || scroll.speed_tenths.unsigned_abs() > MUSICA_AXIS_SCROLL_MAX_SPEED_TENTHS as u32
        || (scroll.start < scroll.target && scroll.speed_tenths < 0)
        || (scroll.start > scroll.target && scroll.speed_tenths > 0)
        || scroll.duration_ms
            != axis_scroll_duration_ms(scroll.start, scroll.target, scroll.speed_tenths)?
    {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    if scroll.elapsed_ns > duration_ns
        || scroll.completed != (scroll.elapsed_ns == duration_ns)
        || scroll.current != axis_scroll_visible_position(scroll)?
        || stage_axis_position(stage, scroll.axis)? != scroll.current
    {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    Ok(())
}
pub(super) fn validate_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if let Some(scroll) = &state.axis_scroll {
        validate_axis_scroll_state(
            scroll,
            state.stage.as_ref().ok_or(MusicaRuntimeError::AxisScroll)?,
        )?;
    } else if matches!(state.wait, Some(MusicaWaitState::AxisScroll { .. })) {
        return Err(MusicaRuntimeError::AxisScroll);
    }
    if state.axis_scroll.is_some() && state.linear_scroll.is_some() {
        return Err(MusicaRuntimeError::LinearScroll);
    }
    if let Some(scroll) = &state.linear_scroll {
        linear::validate_linear_scroll_state(
            scroll,
            state
                .stage
                .as_ref()
                .ok_or(MusicaRuntimeError::LinearScroll)?,
        )?;
    } else if matches!(state.wait, Some(MusicaWaitState::LinearScroll { .. })) {
        return Err(MusicaRuntimeError::LinearScroll);
    }
    super::message::validate_state(state)?;
    super::backlog::validate_state(state)?;
    super::read_state::validate(state)?;
    super::character::validate_state(state)?;
    super::particles::validate_state(state)?;
    if let Some(scroll) = &state.wscroll2 {
        if state.stage.is_none()
            || state.axis_scroll.is_some()
            || state.linear_scroll.is_some()
            || state.scroll_xf.is_some()
            || state.effect.is_some()
        {
            return Err(MusicaRuntimeError::WScroll2);
        }
        super::wscroll2::validate_wscroll2_state(scroll)?;
    }
    if let Some(scroll) = &state.scroll_xf {
        if state.stage.is_none() || state.axis_scroll.is_some() || state.linear_scroll.is_some() {
            return Err(MusicaRuntimeError::ScrollXf);
        }
        super::scroll_xf::validate_scroll_xf_state(scroll)?;
    }
    Ok(())
}

pub(super) fn execute_end_scroll(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = command
        .tokens()
        .map_err(|_| MusicaRuntimeError::AxisScroll)?;
    let [finish] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::AxisScroll);
    };
    let force_finish = finish
        .as_bytes()
        .first()
        .is_some_and(|value| matches!(value, b'1'..=b'9' | b't' | b'T'));
    if state
        .axis_scroll
        .as_ref()
        .is_some_and(|scroll| !scroll.completed)
    {
        if force_finish {
            complete_axis_scroll_state(state)?;
            let sequence = next_effect_sequence(state)?;
            return Ok(Some(MusicaVmEvent::AxisScroll(MusicaAxisScrollFrame {
                sequence,
            })));
        }
        let scroll = state
            .axis_scroll
            .as_ref()
            .ok_or(MusicaRuntimeError::AxisScroll)?;
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let remaining_ns = duration_ns
            .checked_sub(scroll.elapsed_ns)
            .filter(|remaining| *remaining > 0)
            .ok_or(MusicaRuntimeError::AxisScroll)?;
        let milliseconds = u32::try_from(remaining_ns.div_ceil(1_000_000))
            .map_err(|_| MusicaRuntimeError::Overflow)?;
        let wait = MusicaWaitState::AxisScroll {
            token_id: format!("musica.scroll.{}", state.instruction_count),
            milliseconds,
        };
        state.wait = Some(wait.clone());
        return Ok(Some(MusicaVmEvent::Wait(wait)));
    }
    if state
        .linear_scroll
        .as_ref()
        .is_some_and(|scroll| !scroll.completed)
    {
        if force_finish {
            linear::complete_linear_scroll_state(state)?;
            let sequence = next_effect_sequence(state)?;
            return Ok(Some(MusicaVmEvent::LinearScroll(MusicaLinearScrollFrame {
                sequence,
            })));
        }
        let scroll = state
            .linear_scroll
            .as_ref()
            .ok_or(MusicaRuntimeError::LinearScroll)?;
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let remaining_ns = duration_ns
            .checked_sub(scroll.elapsed_ns)
            .filter(|remaining| *remaining > 0)
            .ok_or(MusicaRuntimeError::LinearScroll)?;
        let milliseconds = u32::try_from(remaining_ns.div_ceil(1_000_000))
            .map_err(|_| MusicaRuntimeError::Overflow)?;
        let wait = MusicaWaitState::LinearScroll {
            token_id: format!("musica.scroll.{}", state.instruction_count),
            milliseconds,
        };
        state.wait = Some(wait.clone());
        return Ok(Some(MusicaVmEvent::Wait(wait)));
    }
    let Some(scroll) = state.scroll_xf.as_mut() else {
        return Ok(None);
    };
    if !force_finish || scroll.completed {
        return Ok(None);
    }
    scroll.elapsed_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::Overflow)?;
    scroll.completed = true;
    scroll.visible_extent = scroll.end_extent;
    scroll.visible_offset = scroll.end_offset;
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::ScrollXf(MusicaScrollXfFrame {
        sequence,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_sc, ScOpcodeCatalog};
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/scroll.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    #[test]
    fn axis_scroll_restores_fractional_clock_and_finishes_exactly() {
        for (op, target, speed) in [("hscroll", 13, 3), ("vscroll", -13, -3)] {
            let source = format!(
                ".stage * BG.png 0 0\r\n.{op} {target} {speed}\r\n.endscroll false\r\n.end\r\n"
            );
            let mut original = vm(source.as_bytes());
            original.step(1).unwrap();
            original.step(2).unwrap();
            original.advance_axis_scroll_clock(10_500_000).unwrap();
            assert_eq!(
                original.state().axis_scroll.as_ref().unwrap().current,
                if target > 0 { 3 } else { -3 }
            );
            assert!(matches!(
                original.step(3).unwrap(),
                Some(MusicaVmEvent::Wait(MusicaWaitState::AxisScroll { .. }))
            ));
            let mut restored = vm(source.as_bytes());
            restored
                .restore_native_save(&original.encode_native_save().unwrap(), 4)
                .unwrap();
            for delta in [500_000, 32_000_000, 1_000_000] {
                assert_eq!(
                    original.advance_axis_scroll_clock(delta).unwrap(),
                    restored.advance_axis_scroll_clock(delta).unwrap()
                );
                assert_eq!(original.state().axis_scroll, restored.state().axis_scroll);
                assert_eq!(original.state().stage, restored.state().stage);
            }
            let scroll = original.state().axis_scroll.as_ref().unwrap();
            assert!(scroll.completed);
            assert_eq!(scroll.current, target);
            assert!(original
                .advance_axis_scroll_clock(10_000_000)
                .unwrap()
                .is_none());
        }
    }
    #[test]
    fn axis_scroll_force_finish_stage_replacement_and_invalid_save() {
        let source=b".stage * BG.png 0 0\r\n.hscroll 100 10\r\n.endscroll true\r\n.stage * BG.png 0 0\r\n.end\r\n";
        let mut machine = vm(source);
        machine.step(1).unwrap();
        machine.step(2).unwrap();
        let valid = machine.encode_native_save().unwrap();
        let mut state = machine.state().clone();
        state.axis_scroll.as_mut().unwrap().current = 90;
        let bad = postcard::to_allocvec(&state).unwrap();
        assert_eq!(
            machine.restore_native_save(&bad, 3).unwrap_err(),
            MusicaRuntimeError::AxisScroll
        );
        assert_eq!(machine.encode_native_save().unwrap(), valid);
        machine.step(3).unwrap();
        assert_eq!(
            machine
                .state()
                .stage
                .as_ref()
                .unwrap()
                .background
                .as_ref()
                .unwrap()
                .x,
            100
        );
        assert!(machine.state().axis_scroll.as_ref().unwrap().completed);
        machine.step(4).unwrap();
        assert!(machine.state().axis_scroll.is_none());
        for command in [
            "hscroll 10 -1",
            "vscroll -10 1",
            "hscroll 100 0",
            "hscroll 65537 10",
        ] {
            let source = format!(".stage * BG.png 0 0\r\n.{command}\r\n");
            let mut machine = vm(source.as_bytes());
            machine.step(1).unwrap();
            assert_eq!(machine.step(2).unwrap_err(), MusicaRuntimeError::AxisScroll);
        }
    }
}
