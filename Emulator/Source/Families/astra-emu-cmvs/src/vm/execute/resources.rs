use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StartResourceChannel { bank },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::TaggedStringReference, resource)],
        ) => {
            // The recovered setup handlers return without creating a player
            // when the channel is outside each bank's fixed slot table.
            let Ok(channel) = u8::try_from(*channel) else {
                return Ok(None);
            };
            if channel >= bank.slot_count() {
                return Ok(None);
            }
            let resource = tag_zero_reference(*resource)?;
            state
                .resource_channels
                .entry(bank)
                .or_default()
                .insert(channel, resource);
            Ok(Some(CmvsPs2aVmAction::StartResourceChannel {
                bank,
                channel,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::PlayChannelSound {
                channel_table_dword_index,
                max_channel,
                play_error_mask: _,
                channel_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            let channel_key = (channel_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&channel_key) {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::PlayChannelSound {
                channel: *channel,
                name: tag_zero_reference(*name)?,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot {
                max_slot,
                gate_field_offset,
                error_flag_field_offset,
                error_flag_mask,
                ..
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled), (CmvsPs2aCommandStackWordKind::BooleanU32, loop_playback), (CmvsPs2aCommandStackWordKind::OpaqueU32, volume)],
        ) => {
            // `sub_48A110` rejects slots above 5 by raising the error-flag
            // mask without touching the record table.
            if *slot > u32::from(max_slot) {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let resource = tag_zero_reference(*name)?;
            // The playback path (`sub_48DB70`) only runs when the gate field
            // is non-zero; that path is not recovered yet.
            if state
                .interpreter_words
                .get(&u32::from(gate_field_offset))
                .copied()
                .unwrap_or(0)
                != 0
            {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_RESOURCE_CHANNEL",
                    "CMVS resource channel playback is not recovered",
                ));
            }
            state.resource_channel_slots.insert(
                *slot,
                CmvsResourceChannelSlotRecord {
                    enabled: *enabled != 0,
                    volume: *volume,
                    loop_playback: *loop_playback != 0,
                    resource,
                },
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearResourceSlot {
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            // `sub_47F8D0` rejects slot indices at or above 8 with the
            // error mask; a valid index frees the slot, which is a no-op
            // when nothing occupies it.
            let Ok(slot) = u8::try_from(*slot) else {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            };
            if slot > max_slot {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            }
            state.resource_slots.remove(&slot);
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
