use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ReadInputLatches {
                release_field_offset,
                held_field_offset,
            },
            [],
        ) => {
            state
                .interpreter_words
                .insert(release_field_offset, u32::from(state.input_advance_release));
            state
                .interpreter_words
                .insert(held_field_offset, u32::from(state.input_confirm_held));
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch, []) => {
            state.input_advance_release = false;
            state.input_advance_press = false;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreTextureReadyFlag {
                result_field_offset,
                override_field_offset,
            },
            [],
        ) => {
            // The headless texture pipeline decodes synchronously, so the
            // recovered readiness pair (`sub_45CC20`/`sub_45D2D0`) always
            // reports ready; the override field still forces success, which
            // is already one here.
            let _overridden = state
                .interpreter_words
                .get(&override_field_offset)
                .copied()
                .unwrap_or_default()
                != 0;
            state.interpreter_words.insert(result_field_offset, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearTextureChannelTable {
                channel_table_base_offset,
            },
            [],
        ) => {
            let table = u16::try_from(channel_table_base_offset).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_TEXTURE",
                    "texture table offset overflowed",
                )
            })?;
            state
                .slot_objects
                .retain(|key, _| (*key >> 16) != u32::from(table));
            state
                .slot_object_seed_words
                .retain(|key, _| (*key >> 16) != u32::from(table));
            state.texture_children.clear();
            state.texture_parents.clear();
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentResource {
                texture_table_base_offset,
                max_parent_slot,
                result_field_offset,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            // A 0x80-tagged name names a global message slot whose content
            // is the resolved texture name; unfold nested slots down to the
            // single pool string (same resolution as command 128 names).
            let resource = script_name_reference(state, *name)?;
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let parent_slot = *parent as u8;
            let parent_state = state.texture_parents.get_mut(&parent_slot).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_TEXTURE",
                    "CMVS live texture parent has no specialized state",
                )
            })?;
            parent_state.resource = Some(resource);
            parent_state.resource_frame = Some(state.current_frame);
            // The host blocks the step if VFS resolution or decode fails;
            // continuation therefore observes the original non-zero loader
            // result only after this action has been accepted.
            state.interpreter_words.insert(result_field_offset, 1);
            Ok(Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentResourceExtended {
                texture_table_base_offset,
                result_field_offset,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _word_a), (CmvsPs2aCommandStackWordKind::OpaqueU32, _word_b), (CmvsPs2aCommandStackWordKind::OpaqueU32, _word_c)],
        ) => {
            let resource = tag_zero_reference(*name)?;
            if *parent >= 256 {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let parent_slot = *parent as u8;
            // `sub_47ACA0` creates the container when absent and replaces
            // any previous owner before the load; only occupancy and the
            // resource reference are observable.
            let key = slot_object_key(
                u16::try_from(texture_table_base_offset).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot table offset overflowed",
                    )
                })?,
                *parent,
            )?;
            state.slot_objects.entry(key).or_insert_with(|| {
                let identity = state.next_slot_object_id;
                state.next_slot_object_id += 1;
                identity
            });
            let parent_state = state.texture_parents.entry(parent_slot).or_default();
            parent_state.resource = Some(resource);
            parent_state.resource_frame = Some(state.current_frame);
            state.interpreter_words.insert(result_field_offset, 1);
            state.interpreter_words.insert(3260, 1);
            Ok(Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::CommitTexturePresentation {
                result_field_offset,
                present_object_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, mode)],
        ) => {
            // `sub_47AF00`: without the presentation object the store is
            // zero and dispatch continues; with one, the texture is shown
            // and the frame ends (`0xC004`) or, in the zero mode, the
            // dispatch waits a frame (`0xA000`, delay counter increments).
            let exists = state
                .interpreter_words
                .get(&present_object_field_offset)
                .copied()
                .unwrap_or_default()
                != 0;
            if !exists {
                state.interpreter_words.insert(result_field_offset, 0);
                return Ok(None);
            }
            if mode == &0 {
                let delay = state
                    .interpreter_words
                    .get(&13224)
                    .copied()
                    .unwrap_or_default();
                state.interpreter_words.insert(13224, delay + 1);
                state.dispatch_stopped = true;
                return Ok(None);
            }
            state.interpreter_words.insert(result_field_offset, 1);
            state.dispatch_stopped = true;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::InitializeTextureParentSurface {
                texture_table_base_offset,
                max_parent_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            state
                .texture_parents
                .get_mut(&(*parent as u8))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_TEXTURE",
                        "CMVS live texture parent has no specialized state",
                    )
                })?
                .surface_initialized = true;
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
