use super::*;
const MUSICA_SCREEN_SHAKE_MAX_AMPLITUDE: i32 = 1280;
const MUSICA_SCREEN_SHAKE_MAX_INTERVAL_MS: u32 = 60_000;

impl MusicaVm {
    pub fn advance_screen_shake_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaScreenShakeFrame>, MusicaRuntimeError> {
        let Some(shake) = self.state.screen_shake.as_mut() else {
            return Ok(None);
        };
        shake.elapsed_ns = shake
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let interval_ns = u64::from(shake.interval_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
        if shake.elapsed_ns < interval_ns {
            return Ok(None);
        }
        shake.elapsed_ns = 0;
        let amplitude = shake.amplitude;
        shake.offset = match shake.kind {
            MusicaScreenShakeKind::Vertical => {
                if shake.update_index.is_multiple_of(2) {
                    [0, -amplitude]
                } else {
                    [0, amplitude]
                }
            }
            MusicaScreenShakeKind::Random => {
                let pattern = next_native_random_15(&mut self.state.random_state) % 8;
                random_screen_shake_offset(pattern, amplitude)?
            }
        };
        shake.update_index = shake
            .update_index
            .checked_add(1)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MusicaScreenShakeFrame { sequence }))
    }
}
pub(super) fn execute_screen_shake(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = command
        .tokens()
        .map_err(|_| MusicaRuntimeError::ScreenShake)?;
    let [kind, amplitude, interval_ms] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::ScreenShake);
    };
    let kind = match kind.as_str() {
        "R" | "r" => MusicaScreenShakeKind::Random,
        "V" | "v" => MusicaScreenShakeKind::Vertical,
        _ => return Err(MusicaRuntimeError::ScreenShake),
    };
    let amplitude = amplitude
        .parse::<i32>()
        .ok()
        .filter(|value| (1..=MUSICA_SCREEN_SHAKE_MAX_AMPLITUDE).contains(value))
        .ok_or(MusicaRuntimeError::ScreenShake)?;
    let interval_ms = interval_ms
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=MUSICA_SCREEN_SHAKE_MAX_INTERVAL_MS).contains(value))
        .ok_or(MusicaRuntimeError::ScreenShake)?;
    state.screen_shake = Some(MusicaScreenShakeState {
        kind,
        amplitude,
        interval_ms,
        elapsed_ns: 0,
        update_index: 0,
        offset: [0, 0],
    });
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MusicaVmEvent::ScreenShake(MusicaScreenShakeFrame {
        sequence,
    })))
}

fn random_screen_shake_offset(
    pattern: u32,
    amplitude: i32,
) -> Result<[i32; 2], MusicaRuntimeError> {
    let offset = match pattern {
        // These eight cases preserve the native switch fall-through. Cases
        // 2, 4, and 6 apply two buffer-copy helpers and therefore produce a
        // diagonal displacement rather than a single-axis approximation.
        0 => [0, -amplitude],
        1 => [-amplitude, 0],
        2 => [amplitude, amplitude],
        3 => [amplitude, 0],
        4 => [-amplitude, amplitude],
        5 => [0, amplitude],
        6 => [amplitude, -amplitude],
        7 => [0, -amplitude],
        _ => return Err(MusicaRuntimeError::ScreenShake),
    };
    Ok(offset)
}

pub(crate) fn validate_screen_shake_state(
    shake: &MusicaScreenShakeState,
) -> Result<(), MusicaRuntimeError> {
    if !(1..=MUSICA_SCREEN_SHAKE_MAX_AMPLITUDE).contains(&shake.amplitude)
        || !(1..=MUSICA_SCREEN_SHAKE_MAX_INTERVAL_MS).contains(&shake.interval_ms)
    {
        return Err(MusicaRuntimeError::ScreenShake);
    }
    let interval_ns = u64::from(shake.interval_ms)
        .checked_mul(1_000_000)
        .ok_or(MusicaRuntimeError::ScreenShake)?;
    let expected_offset = match shake.kind {
        MusicaScreenShakeKind::Vertical if shake.update_index == 0 => Some([0, 0]),
        MusicaScreenShakeKind::Vertical if (shake.update_index - 1).is_multiple_of(2) => {
            Some([0, -shake.amplitude])
        }
        MusicaScreenShakeKind::Vertical => Some([0, shake.amplitude]),
        MusicaScreenShakeKind::Random if shake.update_index == 0 => Some([0, 0]),
        MusicaScreenShakeKind::Random => None,
    };
    let random_offset_is_valid = (0..8).any(|pattern| {
        random_screen_shake_offset(pattern, shake.amplitude)
            .is_ok_and(|offset| offset == shake.offset)
    });
    if !(1..=MUSICA_SCREEN_SHAKE_MAX_AMPLITUDE).contains(&shake.amplitude)
        || !(1..=MUSICA_SCREEN_SHAKE_MAX_INTERVAL_MS).contains(&shake.interval_ms)
        || shake.elapsed_ns >= interval_ns
        || shake
            .offset
            .iter()
            .any(|value| value.unsigned_abs() > shake.amplitude as u32)
        || expected_offset.is_some_and(|expected| shake.offset != expected)
        || (shake.kind == MusicaScreenShakeKind::Random
            && shake.update_index != 0
            && !random_offset_is_valid)
    {
        return Err(MusicaRuntimeError::ScreenShake);
    }
    Ok(())
}

fn next_native_random_15(state: &mut u64) -> u32 {
    next_firefly_random(state) & 0x7fff
}

fn next_firefly_random(state: &mut u64) -> u32 {
    // SplitMix64 is small, deterministic on every supported target, and keeps
    // the adapter independent from the process-global C rand() state used by
    // the original executable.
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (value ^ (value >> 31)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_sc, ScOpcodeCatalog};
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/shake.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    #[test]
    fn screen_shake_clock_restore_and_transition_replacement() {
        for mode in ["V", "R"] {
            let source = format!(".shakescreen {mode} 4 30\r\n.transition 0 * 0\r\n.end\r\n");
            let mut original = vm(source.as_bytes());
            assert!(matches!(
                original.step(1).unwrap(),
                Some(MusicaVmEvent::ScreenShake(_))
            ));
            assert!(original
                .advance_screen_shake_clock(29_000_000)
                .unwrap()
                .is_none());
            let mut restored = vm(source.as_bytes());
            restored
                .restore_native_save(&original.encode_native_save().unwrap(), 2)
                .unwrap();
            for delta in [1_000_000, 100_000_000, 29_000_000, 1_000_000] {
                assert_eq!(
                    original.advance_screen_shake_clock(delta).unwrap(),
                    restored.advance_screen_shake_clock(delta).unwrap()
                );
                assert_eq!(original.state().screen_shake, restored.state().screen_shake);
                assert_eq!(original.state().random_state, restored.state().random_state);
            }
            assert_eq!(
                original.state().screen_shake.as_ref().unwrap().update_index,
                3
            );
            assert_eq!(original.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
            assert!(original.state().screen_shake.is_none());
            assert!(original
                .advance_screen_shake_clock(100_000_000)
                .unwrap()
                .is_none());
        }
    }
    #[test]
    fn screen_shake_rejects_corrupt_save_before_mutating_live_state() {
        let mut machine = vm(b".shakescreen V 4 30\r\n.end\r\n");
        machine.step(1).unwrap();
        machine.advance_screen_shake_clock(30_000_000).unwrap();
        assert_eq!(
            machine.state().screen_shake.as_ref().unwrap().offset,
            [0, -4]
        );
        let valid = machine.encode_native_save().unwrap();
        for mutation in 0..4 {
            let mut state = machine.state().clone();
            let shake = state.screen_shake.as_mut().unwrap();
            match mutation {
                0 => shake.amplitude = i32::MIN,
                1 => shake.interval_ms = 0,
                2 => shake.offset = [4, 4],
                _ => shake.elapsed_ns = 30_000_000,
            }
            let bad = postcard::to_allocvec(&state).unwrap();
            assert_eq!(
                MusicaVm::decode_native_save(&bad).unwrap_err(),
                MusicaRuntimeError::ScreenShake
            );
            assert_eq!(
                machine.restore_native_save(&bad, 2).unwrap_err(),
                MusicaRuntimeError::ScreenShake
            );
            assert_eq!(machine.encode_native_save().unwrap(), valid);
        }
        for source in [
            b".shakescreen X 4 30\r\n".as_slice(),
            b".shakescreen V 0 30\r\n",
            b".shakescreen R 4 0\r\n",
            b".shakescreen 0\r\n",
        ] {
            assert_eq!(
                vm(source).step(1).unwrap_err(),
                MusicaRuntimeError::ScreenShake
            );
        }
    }
}
