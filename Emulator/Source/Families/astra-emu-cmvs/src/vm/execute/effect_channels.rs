use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StoreChannelVisibilityRecord {
                enabled_field_base_offset,
                touched_field_base_offset,
                record_stride_bytes,
                notify_gate_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            if *channel > 5 {
                let flags = state
                    .interpreter_flag_words
                    .entry(INTERPRETER_ERROR_FLAG_OFFSET)
                    .or_insert(0);
                *flags |= 0x200;
                return Ok(None);
            }
            let channel = u8::try_from(*channel).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_CHANNEL_RECORD",
                    "CMVS channel visibility record index overflowed",
                )
            })?;
            let stride = u16::from(record_stride_bytes);
            let enabled_offset = enabled_field_base_offset + stride * u16::from(channel);
            let touched_offset = touched_field_base_offset + stride * u16::from(channel);
            state
                .interpreter_words
                .insert(u32::from(enabled_offset), u32::from(*enabled != 0));
            state.interpreter_words.insert(u32::from(touched_offset), 1);
            // The notification call only happens when the gate field is
            // non-zero. The gate is unrecovered engine state retained in the
            // opaque word map and defaults to zero until proven otherwise.
            let gate = state
                .interpreter_words
                .get(&u32::from(notify_gate_field_offset))
                .copied()
                .unwrap_or(0);
            if gate != 0 {
                let target_index = channel.checked_add(9).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_CHANNEL_RECORD",
                        "CMVS channel visibility notify index overflowed",
                    )
                })?;
                Ok(Some(CmvsPs2aVmAction::NotifyChannelVisibility {
                    target_index,
                    enabled: *enabled != 0,
                }))
            } else {
                Ok(None)
            }
        }
        (
            CmvsPs2aCommandEffectKind::ResetChannelVisibilityRecord {
                name_buffer_base_offset,
                touched_field_base_offset,
                record_stride_bytes,
                error_flag_mask,
                teardown_slot_table_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *channel > 5 {
                let flags = state
                    .interpreter_flag_words
                    .entry(INTERPRETER_ERROR_FLAG_OFFSET)
                    .or_insert(0);
                *flags |= error_flag_mask;
                return Ok(None);
            }
            let channel = u8::try_from(*channel).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_CHANNEL_RECORD",
                    "CMVS channel visibility record index overflowed",
                )
            })?;
            // The teardown helper waits until the slot at channel+9 drains;
            // its postcondition is an empty slot, so model the completed
            // teardown instead of the wait itself.
            let teardown_index = u32::from(channel.checked_add(9).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_CHANNEL_RECORD",
                    "CMVS channel teardown slot index overflowed",
                )
            })?);
            let teardown_key = slot_object_key(teardown_slot_table_offset, teardown_index)?;
            state.slot_objects.remove(&teardown_key);
            state.slot_object_seed_words.remove(&teardown_key);
            let stride = u16::from(record_stride_bytes);
            let buffer_offset = name_buffer_base_offset + stride * u16::from(channel);
            let touched_offset = touched_field_base_offset + stride * u16::from(channel);
            // The reset copies the unrecovered engine-owned `Default`
            // constant, so no script-owned content survives in the buffer.
            state
                .interpreter_string_buffers
                .insert(buffer_offset, Vec::new());
            state.interpreter_words.insert(u32::from(touched_offset), 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value)],
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
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelWord {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelOrigin { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, mode), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if *channel > 7 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_4678C0`: mode 0 stores the coordinates directly, mode 1
            // adds them to the current origin, and any other mode stores the
            // first coordinate into both axes.
            let object = state.effect_channels.entry(*channel as u8).or_default();
            let (origin_x, origin_y) = object.origin();
            let (x, y) = match *mode {
                0 => (*first as i32, *second as i32),
                1 => (
                    origin_x.wrapping_add(*first as i32),
                    origin_y.wrapping_add(*second as i32),
                ),
                _ => (*first as i32, *first as i32),
            };
            object.set_origin(x, y);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_488FC0`/`sub_488F80`/`sub_488F40` reject an index at or
            // above 12 and an unregistered playback record with 0x1000.
            let occupied = *index < 12 && state.slot_objects.contains_key(&(750_u32 << 8 | *index));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state
                .effect_playback
                .entry(*index as u8)
                .or_default()
                .set_field(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackMode { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, mode), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_playback_mode(*mode as u16, *first as u16 as i16, *second as u16 as i16);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelValuePair { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_value_pair(*first, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyEffectChannelOperation { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            // `sub_47FB20` -> `sub_467A10` -> `sub_464C00(channel[631])`; the
            // channel check is the VM-visible effect.
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectState {
                first_field_offset,
                second_field_offset,
                third_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0002_0000)?;
                return Ok(None);
            }
            state.interpreter_words.insert(first_field_offset, 0);
            state.interpreter_words.insert(second_field_offset, 0);
            state.interpreter_words.insert(third_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag {
                effect_table_dword_index,
                error_mask,
                activity_state_key_base,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, flag)],
        ) => {
            if *channel > 7 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                // The original dereferences the channel object without a
                // guard; the recovered subset fails fast instead.
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let state_key = activity_state_key_base + *channel;
            if state
                .interpreter_words
                .get(&state_key)
                .copied()
                .unwrap_or_default()
                != *flag
            {
                state.interpreter_words.insert(state_key, *flag);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectChannelActivity {
                activity_state_key_base,
                max_channel,
                effect_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            let effect_key = (effect_table_dword_index << 8) | *channel;
            let live = *channel <= max_channel && state.slot_objects.contains_key(&effect_key);
            if !live {
                // The original raises the channel error mask and leaves the
                // result field untouched.
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_47F270` reads the channel object's transient field +8,
            // which the recovered playback flag models in the interpreter
            // word the case-334 handler maintains.
            let activity = state
                .interpreter_words
                .get(&(activity_state_key_base + *channel))
                .copied()
                .unwrap_or(0);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(activity != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryChannelOccupancy {
                result_field_offset,
                channel_table_dword_index,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *channel > 7 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (channel_table_dword_index << 8) | *channel;
            let occupied = state.slot_objects.contains_key(&effect_key);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(occupied));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _), (CmvsPs2aCommandStackWordKind::OpaqueU32, _), (CmvsPs2aCommandStackWordKind::OpaqueU32, _), (CmvsPs2aCommandStackWordKind::OpaqueU32, _)],
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
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelPair {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _first), (CmvsPs2aCommandStackWordKind::OpaqueU32, _second)],
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
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelPair {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelBlock {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value)],
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
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelBlock {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelEnabled {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
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
            // `sub_47F160` -> `sub_467CE0` stores the boolean at the channel
            // record's flag dword 622 alongside the recovered enable action.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_flag(*enabled != 0);
            Ok(Some(CmvsPs2aVmAction::SetEffectChannelEnabled {
                channel: *channel,
                enabled: *enabled != 0,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::CreateEffectChannel {
                effect_table_dword_index,
                playback_table_dword_index,
                max_channel,
                max_effect,
                channel_error_mask,
                effect_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, effect), ..],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            if *effect > max_effect {
                raise_error_flag(state, 10532, effect_error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            let playback_key = (playback_table_dword_index << 8) | *effect;
            let identity = state.next_slot_object_id;
            state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                    "CMVS slot object identity space overflowed",
                )
            })?;
            state.slot_objects.insert(effect_key, identity);
            state.slot_objects.insert(playback_key, identity);
            // The recovered channel object starts as a zero-initialized
            // 0x3088-byte image; later cases write its quad records.
            state.effect_channels.entry(*channel as u8).or_default();
            state.effect_playback.entry(*effect as u8).or_default();
            Ok(Some(CmvsPs2aVmAction::CreateEffectChannel {
                channel: *channel,
                effect: *effect,
            }))
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
