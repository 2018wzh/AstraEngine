use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad), (CmvsPs2aCommandStackWordKind::OpaqueU32, sub), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth), (CmvsPs2aCommandStackWordKind::OpaqueU32, sixth)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466CB0` truncates every value to `__int16` and stores the
            // six-word geometry block at `38*quad + 6*sub + 22`.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .write_quad_geometry(
                    *quad,
                    *sub,
                    [
                        *first as u16 as i16,
                        *second as u16 as i16,
                        *third as u16 as i16,
                        *fourth as u16 as i16,
                        *fifth as u16 as i16,
                        *sixth as u16 as i16,
                    ],
                );
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D10` stores four `__int16` hit-rectangle words at
            // `38*quad + 52`.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .write_quad_rect(
                    *quad,
                    [
                        *first as u16 as i16,
                        *second as u16 as i16,
                        *third as u16 as i16,
                        *fourth as u16 as i16,
                    ],
                );
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SelectEffectQuad { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D50`: the quad record's visibility flag becomes 1 and
            // the bound element is shown.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_quad_visible(*quad, true);
            Ok(Some(CmvsPs2aVmAction::SelectEffectQuad {
                channel: *channel,
                quad: *quad,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::DeselectEffectQuad { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D90`: the quad record's visibility flag becomes 0 and
            // the bound element is hidden.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_quad_visible(*quad, false);
            Ok(Some(CmvsPs2aVmAction::DeselectEffectQuad {
                channel: *channel,
                quad: *quad,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ActivateEffectQuad { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_47F330` stores the selected animation frame at word
            // `38*quad + 38` and, when the frame actually changes, copies the
            // frame's geometry sub-record onto the bound element through
            // `sub_446B30`. The element surface is not modelled yet, so only
            // the frame word is persisted.
            let frame = *value as u16;
            let channel_object = state.effect_channels.entry(*channel as u8).or_default();
            if channel_object.quad_frame(*quad) != frame {
                channel_object.set_quad_frame(*quad, frame);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectQuadState {
                effect_table_dword_index,
                empty_error_mask,
                result0_field_offset,
                result1_field_offset,
                result2_field_offset,
                result3_field_offset,
                result8_field_offset,
                result9_field_offset,
                ..
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _index)],
        ) => {
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, empty_error_mask)?;
                return Ok(None);
            }
            // The headless effect graph holds no animated quad state, so the
            // selected sub-object reports zeros for all six fields.
            for offset in [
                result0_field_offset,
                result1_field_offset,
                result2_field_offset,
                result3_field_offset,
                result8_field_offset,
                result9_field_offset,
            ] {
                state.interpreter_words.insert(offset, 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectQuadActive {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad)],
        ) => {
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0002_0000)?;
                return Ok(None);
            }
            // `sub_47F470` reads the quad record's byte at offset
            // `76*quad + 40` and stores `(byte == 0)`: 1 while the quad is
            // hidden, 0 once case 402 shows it. The script gates the
            // quad-update path on this flag, so it must follow the
            // recovered visibility transitions rather than a constant.
            let hidden = state
                .effect_channels
                .get(&(*channel as u8))
                .is_none_or(|object| !object.quad_visible(*quad));
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hidden));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectPosition {
                result_x_field_offset,
                result_y_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            // The headless engine has no animated effect position.
            state.interpreter_words.insert(result_x_field_offset, 0);
            state.interpreter_words.insert(result_y_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectPointerHit {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_x), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_y), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_w), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_h)],
        ) => {
            // `sub_47F2C0` requires an occupied channel and forwards the
            // rectangle to the hit test (`sub_466A50`). The stage scale and
            // offset are identity in headless, so the test compares the
            // streamed pointer position words against the rectangle.
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            let pointer_x = state.pointer_x.max(0) as u32;
            let pointer_y = state.pointer_y.max(0) as u32;
            let x = *rect_x;
            let y = *rect_y;
            let hit = x <= pointer_x
                && y <= pointer_y
                && x.saturating_add(*rect_w) > pointer_x
                && y.saturating_add(*rect_h) > pointer_y;
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hit));
            tracing::trace!(
                event = "astra.emu.cmvs.vm.hit_trace",
                pc = state.program_counter,
                frame = state.current_frame
            );
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
