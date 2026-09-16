use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::SelectEffectChild {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _child)],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SelectEffectChild {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectTextSurface { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::TaggedStringReference, first), (CmvsPs2aCommandStackWordKind::TaggedStringReference, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_488E70` rejects an index at or above 12 and an
            // unregistered playback record with 0x1000, then forwards to
            // `sub_462C10`, which stores the value and both strings.
            let occupied = *index < 12 && state.slot_objects.contains_key(&(750_u32 << 8 | *index));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let first_reference = tag_zero_reference(*first)?;
            let second_reference = tag_zero_reference(*second)?;
            let record = state.effect_playback.entry(*index as u8).or_default();
            record.text_surface.value = *value;
            record.text_surface.first = Some(first_reference);
            record.text_surface.second = Some(second_reference);
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectTextSurface {
                effect: *index,
                first: first_reference,
                second: second_reference,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementVisible { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            // `sub_47D850` requires channel and element; `sub_445EA0` stores
            // the boolean at the element's visibility dword.
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, *child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            element.visible = u32::from(*value != 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectElementExists {
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            // `sub_47D630` only requires the channel; a missing element simply
            // reports zero.
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let exists = state
                .effect_elements
                .contains_key(&(*channel as u8, *child));
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(exists));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementSize { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, *child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            element.width = *first as i32;
            element.height = *second as i32;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            // `sub_47D590` -> `sub_445E40` forwards the element to the
            // graphics object; the recovered subset keeps the channel and
            // element checks, which are the VM-visible effects.
            if !effect_channel_is_occupied(state, *channel)
                || !state
                    .effect_elements
                    .contains_key(&(*channel as u8, *child))
            {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementAuxiliaryPair { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            // `sub_47D5E0` -> `sub_445E60`, as above.
            if !effect_channel_is_occupied(state, *channel)
                || !state
                    .effect_elements
                    .contains_key(&(*channel as u8, *child))
            {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), ..],
        ) => {
            // `sub_47DC80` -> `sub_445340` + `sub_42BF50`, forwarding the
            // element's texture object; the channel and element checks are
            // the VM-visible effects.
            if !effect_channel_is_occupied(state, *channel)
                || !state
                    .effect_elements
                    .contains_key(&(*channel as u8, *child))
            {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::EffectChildCommand { .. },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), ..],
        ) => {
            // Stack words: channel, child selector, then opaque action
            // words; all are consumed and only occupancy is observable.
            if *channel > 7 {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::EffectChildCommand {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChildEnabled {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _child), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SetEffectChildEnabled {
                channel: *channel,
                enabled: *enabled != 0,
            }))
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
