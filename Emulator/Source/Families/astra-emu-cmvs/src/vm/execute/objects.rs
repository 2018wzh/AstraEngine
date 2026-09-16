use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::CreateSlotObject { table_byte_offset },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            if *slot >= SLOT_OBJECT_COUNT {
                raise_interpreter_error_flag(state);
                return Ok(None);
            }
            let key = slot_object_key(table_byte_offset, *slot)?;
            let identity = state.next_slot_object_id;
            state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                    "CMVS slot object identity space overflowed",
                )
            })?;
            state.slot_objects.insert(key, identity);
            if table_byte_offset == 1924 {
                state
                    .texture_parents
                    .insert(*slot as u8, CmvsTextureParentState::default());
                state.texture_children.insert(*slot as u8, BTreeMap::new());
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CreateSingletonSlotObject {
                table_byte_offset,
                slot,
                field_offset,
                value,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            // The recovered case-544 handler (`sub_47A4F0`) frees the previous
            // occupant, builds a fresh object from the four consumed words,
            // stores its handle in the slot-table word and stores one at the
            // flag offset. Only occupancy, identity, the construction words
            // and the handle word are observable in the recovered subset.
            let key = slot_object_key(table_byte_offset, u32::from(slot))?;
            let identity = state.next_slot_object_id;
            state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                    "CMVS slot object identity space overflowed",
                )
            })?;
            state.slot_objects.insert(key, identity);
            state
                .slot_object_seed_words
                .insert(key, vec![*first, *second, *third, *fourth]);
            if table_byte_offset == 3252 {
                // `sub_47A4F0` publishes the owner handle at this+813 and
                // case 548 dispatches on it.
                let handle = u32::try_from(identity).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot object identity exceeds the handle word",
                    )
                })?;
                state
                    .interpreter_words
                    .insert(u32::from(table_byte_offset), handle);
            }
            state.interpreter_words.insert(field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroySingletonSlotObject {
                table_byte_offset,
                slot,
            },
            [],
        ) => {
            let key = slot_object_key(table_byte_offset, u32::from(slot))?;
            state.slot_objects.remove(&key);
            state.slot_object_seed_words.remove(&key);
            if table_byte_offset == 3252 {
                state
                    .interpreter_words
                    .insert(u32::from(table_byte_offset), 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroySlotObject { table_byte_offset },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            if *slot >= SLOT_OBJECT_COUNT {
                raise_interpreter_error_flag(state);
                return Ok(None);
            }
            let key = slot_object_key(table_byte_offset, *slot)?;
            state.slot_objects.remove(&key);
            state.slot_object_seed_words.remove(&key);
            if table_byte_offset == 1924 {
                state.texture_parents.remove(&(*slot as u8));
                state.texture_children.remove(&(*slot as u8));
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::MoveSlotObject { table_byte_offset },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, destination), (CmvsPs2aCommandStackWordKind::OpaqueU32, source)],
        ) => {
            if *destination >= SLOT_OBJECT_COUNT || *source >= SLOT_OBJECT_COUNT {
                raise_interpreter_error_flag(state);
                return Ok(None);
            }
            let destination_key = slot_object_key(table_byte_offset, *destination)?;
            let source_key = slot_object_key(table_byte_offset, *source)?;
            let moved = state.slot_objects.remove(&source_key);
            let moved_seed = state.slot_object_seed_words.remove(&source_key);
            let moved_texture_children = if table_byte_offset == 1924 {
                state.texture_children.remove(&(*source as u8))
            } else {
                None
            };
            let moved_texture_parent = if table_byte_offset == 1924 {
                state.texture_parents.remove(&(*source as u8))
            } else {
                None
            };
            state.slot_objects.remove(&destination_key);
            state.slot_object_seed_words.remove(&destination_key);
            if table_byte_offset == 1924 {
                state.texture_parents.remove(&(*destination as u8));
                state.texture_children.remove(&(*destination as u8));
            }
            if let Some(identity) = moved {
                state.slot_objects.insert(destination_key, identity);
            }
            if let Some(seed) = moved_seed {
                state.slot_object_seed_words.insert(destination_key, seed);
            }
            if let Some(children) = moved_texture_children {
                state.texture_children.insert(*destination as u8, children);
            }
            if let Some(parent) = moved_texture_parent {
                state.texture_parents.insert(*destination as u8, parent);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyBoundedSlotObject {
                table_dword_index,
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            if *slot > max_slot {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            // The three-slot table has no recovered slot-object key mapping;
            // the release is only observable through the trace action.
            Ok(Some(CmvsPs2aVmAction::DestroyBoundedSlotObject {
                table_dword_index,
                slot: *slot,
            }))
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
