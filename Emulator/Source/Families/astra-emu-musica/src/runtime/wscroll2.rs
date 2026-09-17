use super::*;
const MUSICA_WSCROLL2_TICKS_PER_SECOND: u64 = 60;
const MUSICA_WSCROLL2_MAX_PERIOD_TICKS: u32 = 60_000;
const MUSICA_WSCROLL2_MAX_SPEED_TENTHS: i32 = 10_000;
impl MusicaVm {
    pub fn advance_wscroll2_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaWScroll2Frame>, MusicaRuntimeError> {
        let Some(scroll) = self.state.wscroll2.as_mut() else {
            return Ok(None);
        };
        let previous_ticks = scroll.elapsed_ticks;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let (elapsed_ticks, foreground_offset, background_offset, background_remainder) =
            wscroll2_visible_state(scroll.elapsed_ns, scroll.speed_tenths)?;
        scroll.elapsed_ticks = elapsed_ticks;
        scroll.foreground_offset = foreground_offset;
        scroll.background_offset = background_offset;
        scroll.background_remainder = background_remainder;
        if elapsed_ticks == previous_ticks {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaWScroll2Frame { sequence }))
    }
}
pub(super) fn execute(
    tokens: &[String],
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    if tokens.len() != 4
        || state.stage.is_none()
        || state.scroll_xf.is_some()
        || state.axis_scroll.is_some()
        || state.linear_scroll.is_some()
    {
        return Err(MusicaRuntimeError::WScroll2);
    }
    let sync_resource = tokens[1]
        .strip_prefix("sync:")
        .ok_or(MusicaRuntimeError::WScroll2)?;
    validate_scene_filename(sync_resource).map_err(|_| MusicaRuntimeError::WScroll2)?;
    let period_ticks = tokens[2]
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=MUSICA_WSCROLL2_MAX_PERIOD_TICKS).contains(value))
        .ok_or(MusicaRuntimeError::WScroll2)?;
    let speed_tenths = tokens[3]
        .parse::<i32>()
        .ok()
        .filter(|value| value.unsigned_abs() <= MUSICA_WSCROLL2_MAX_SPEED_TENTHS as u32)
        .ok_or(MusicaRuntimeError::WScroll2)?;
    state.effect = None;
    state.firefly = None;

    state.wscroll2 = Some(MusicaWScroll2State {
        sync_resource_uri: format!("musica:/st/{sync_resource}"),
        period_ticks,
        speed_tenths,
        elapsed_ns: 0,
        elapsed_ticks: 0,
        foreground_offset: 0,
        background_offset: 0,
        background_remainder: 0,
    });
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::WScroll2(MusicaWScroll2Frame {
        sequence,
    })))
}
pub(super) fn validate_wscroll2_state(
    scroll: &MusicaWScroll2State,
) -> Result<(), MusicaRuntimeError> {
    super::stage::validate_uri(&scroll.sync_resource_uri, "musica:/st/")
        .map_err(|_| MusicaRuntimeError::WScroll2)?;
    if !(1..=MUSICA_WSCROLL2_MAX_PERIOD_TICKS).contains(&scroll.period_ticks)
        || scroll.speed_tenths.unsigned_abs() > MUSICA_WSCROLL2_MAX_SPEED_TENTHS as u32
    {
        return Err(MusicaRuntimeError::WScroll2);
    }
    let (elapsed_ticks, foreground_offset, background_offset, background_remainder) =
        wscroll2_visible_state(scroll.elapsed_ns, scroll.speed_tenths)?;
    if scroll.elapsed_ticks != elapsed_ticks
        || scroll.foreground_offset != foreground_offset
        || scroll.background_offset != background_offset
        || scroll.background_remainder != background_remainder
    {
        return Err(MusicaRuntimeError::WScroll2);
    }
    Ok(())
}
fn wscroll2_visible_state(
    elapsed_ns: u64,
    speed_tenths: i32,
) -> Result<(u64, i64, i64, i64), MusicaRuntimeError> {
    let elapsed_ticks = u64::try_from(
        u128::from(elapsed_ns)
            .checked_mul(u128::from(MUSICA_WSCROLL2_TICKS_PER_SECOND))
            .ok_or(MusicaRuntimeError::Overflow)?
            / 1_000_000_000u128,
    )
    .map_err(|_| MusicaRuntimeError::Overflow)?;
    let foreground_offset = i64::try_from(
        i128::from(elapsed_ticks)
            .checked_mul(i128::from(speed_tenths))
            .ok_or(MusicaRuntimeError::Overflow)?
            / 10,
    )
    .map_err(|_| MusicaRuntimeError::Overflow)?;
    let background_offset = if foreground_offset == 0 {
        0
    } else {
        (foreground_offset - foreground_offset.signum()) / 5
    };
    let background_remainder = foreground_offset
        .checked_sub(
            background_offset
                .checked_mul(5)
                .ok_or(MusicaRuntimeError::Overflow)?,
        )
        .ok_or(MusicaRuntimeError::Overflow)?;
    Ok((
        elapsed_ticks,
        foreground_offset,
        background_offset,
        background_remainder,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_sc, ScOpcodeCatalog};
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    #[test]
    fn wscroll2_clock_restore_replacement_and_end() {
        let source =
            b".stage BG.png BG.png 0 0\r\n.effect WScroll2 sync:walk.txt 60 -8\r\n.effect end\r\n";
        let mut machine = vm(source);
        machine.step(1).unwrap();
        machine.step(2).unwrap();
        assert!(machine.advance_wscroll2_clock(1_000).unwrap().is_none());
        machine.advance_wscroll2_clock(999_999_000).unwrap();
        let scroll = machine.state().wscroll2.as_ref().unwrap();
        assert_eq!(
            (
                scroll.elapsed_ticks,
                scroll.foreground_offset,
                scroll.background_offset,
                scroll.background_remainder
            ),
            (60, -48, -9, -3)
        );
        let saved = machine.encode_native_save().unwrap();
        let mut restored = vm(source);
        restored.restore_native_save(&saved, 3).unwrap();
        assert_eq!(
            machine.advance_wscroll2_clock(16_666_667).unwrap(),
            restored.advance_wscroll2_clock(16_666_667).unwrap()
        );
        assert_eq!(machine.state().wscroll2, restored.state().wscroll2);
        machine.step(3).unwrap();
        assert!(machine.state().wscroll2.is_none());
        assert!(machine.advance_wscroll2_clock(1_000_000).unwrap().is_none());
        let mut corrupt = MusicaVm::decode_native_save(&saved).unwrap();
        corrupt.wscroll2.as_mut().unwrap().foreground_offset -= 1;
        assert_eq!(
            MusicaVm::decode_native_save(&postcard::to_allocvec(&corrupt).unwrap()).unwrap_err(),
            MusicaRuntimeError::WScroll2
        );
    }
    #[test]
    fn wscroll2_invalid_replacement_preserves_active_trajectory() {
        for command in [
            "WScroll2 char:walk 60 -8",
            "WScroll2 sync:walk.txt 0 -8",
            "WScroll2 sync:walk.txt 60 10001",
            "WScroll2 sync:walk.txt 60",
            "WScroll2 sync:../walk.txt 60 1",
        ] {
            let source = format!(".stage BG.png BG.png 0 0\r\n.effect WScroll2 sync:walk.txt 60 8\r\n.effect {command}\r\n");
            let mut machine = vm(source.as_bytes());
            machine.step(1).unwrap();
            machine.step(2).unwrap();
            machine.advance_wscroll2_clock(100_000_000).unwrap();
            let before = machine.state().wscroll2.clone();
            assert_eq!(machine.step(3).unwrap_err(), MusicaRuntimeError::WScroll2);
            assert_eq!(machine.state().wscroll2, before);
        }
    }
}
