use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ResetTextureChild {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            if *parent > u32::from(max_parent_slot)
                || !state.slot_objects.contains_key(&slot_object_key(
                    u16::try_from(texture_table_base_offset).map_err(|_| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_TEXTURE",
                            "texture table offset overflowed",
                        )
                    })?,
                    *parent,
                )?)
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            if *child > u32::from(max_child_id) {
                return Ok(None);
            }
            state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .insert(*child as u16, CmvsTextureChildState::default());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureChildResource {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            let resource = tag_zero_reference(*name)?;
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            if *child > u32::from(max_child_id) {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                    "CMVS texture resource targets an out-of-range child id",
                ));
            }
            let child_id = *child as u16;
            let child_state = state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .get_mut(&child_id)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                        "CMVS texture resource targets a child that was not created",
                    )
                })?;
            child_state.resource = Some(resource);
            child_state.resource_frame = Some(state.current_frame);
            Ok(Some(CmvsPs2aVmAction::LoadTextureResource {
                parent_slot: *parent as u8,
                child_id,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::InitializeTextureChildSurface {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let child_id = u16::try_from(*child).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                    "CMVS texture surface child id cannot be represented",
                )
            })?;
            if child_id > max_child_id {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                    "CMVS texture surface targets an out-of-range child id",
                ));
            }
            state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .get_mut(&child_id)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                        "CMVS texture surface targets a child that was not created",
                    )
                })?
                .surface_initialized = true;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildRect {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let words = [*first, *second, *third, *fourth];
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .rect_words = Some(words);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture rectangle targets a child that was not created",
                        )
                    })?
                    .rect_words = Some(words);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let words = [*first, *second];
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .position_words = Some(words);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture position targets a child that was not created",
                        )
                    })?
                    .position_words = Some(words);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let words = [*first, *second];
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .auxiliary_pair_words = Some(words);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture auxiliary pair targets a child that was not created",
                        )
                    })?
                    .auxiliary_pair_words = Some(words);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, word)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let child_id = if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .auxiliary_word = Some(*word);
                None
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture auxiliary word targets a child that was not created",
                        )
                    })?
                    .auxiliary_word = Some(*word);
                Some(*child as u16)
            };
            Ok(Some(CmvsPs2aVmAction::CommitTextureSurface {
                parent_slot: *parent as u8,
                child_id,
            }))
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
