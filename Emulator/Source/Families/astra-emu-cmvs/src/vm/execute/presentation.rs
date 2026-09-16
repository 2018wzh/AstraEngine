use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StorePresentationFrameFields {
                top_field_offset,
                second_field_offset,
                active_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, top), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            state
                .interpreter_words
                .insert(u32::from(top_field_offset), *top);
            state
                .interpreter_words
                .insert(u32::from(second_field_offset), *second);
            state.interpreter_words.insert(
                u32::from(active_field_offset),
                u32::from(*top != 0 && *second != 0),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreClampedPresentationSize {
                field_offset,
                negative_replacement,
                maximum,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            let signed = i32::from_ne_bytes(value.to_ne_bytes());
            let clamped = if signed < 0 {
                negative_replacement
            } else if u32::try_from(signed).unwrap_or(maximum) > maximum {
                maximum
            } else {
                u32::try_from(signed).unwrap_or(maximum)
            };
            state
                .interpreter_words
                .insert(u32::from(field_offset), clamped);
            Ok(Some(CmvsPs2aVmAction::ApplyPresentationLayout {
                size_field_offset: field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::CommitScreenParams { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, commit)],
        ) => {
            // `sub_484D00` -> `sub_457B90`: only a pending commit is applied.
            if *commit != 0 && state.screen_pending {
                state.screen_pending = false;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::WaitScreenCommit {
                error_mask: _,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _selector)],
        ) => {
            // `sub_484D40` blocks the frame with 0xA000 while a commit is
            // pending. The recovered subset has no asynchronous render state,
            // so a pending commit is completed here and reported through the
            // result field.
            let committed = state.screen_pending;
            state.screen_pending = false;
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(committed));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScreenPending {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_484CD0` publishes `sub_456E70`'s pending flag.
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(state.screen_pending));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRgb { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, red), (CmvsPs2aCommandStackWordKind::OpaqueU32, green), (CmvsPs2aCommandStackWordKind::OpaqueU32, blue)],
        ) => {
            // `sub_456DF0` stores `(float)(int)(value * 100.0) / 100.0` in the
            // screen object's fields 1..3.
            let hundredths = |bits: u32| -> u32 {
                ((f32::from_bits(bits) * 100.0) as i32 as f32 / 100.0).to_bits()
            };
            state.screen_words.insert(1, hundredths(*red));
            state.screen_words.insert(2, hundredths(*green));
            state.screen_words.insert(3, hundredths(*blue));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRotation { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, angle)],
        ) => {
            // `sub_456C00`: field 14 keeps `|value * 100| % 36000 / 100.0`.
            let scaled = (f32::from_bits(*angle) * 100.0) as i32;
            let magnitude = scaled.unsigned_abs() % 36_000;
            state
                .screen_words
                .insert(14, (magnitude as f32 / 100.0).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScale { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456D80`: fields 9/10 hold the values, 15/16 their halves.
            let first = f32::from_bits(*first);
            let second = f32::from_bits(*second);
            state.screen_words.insert(9, first.to_bits());
            state.screen_words.insert(10, second.to_bits());
            state.screen_words.insert(15, (first * 0.5).to_bits());
            state.screen_words.insert(16, (second * 0.5).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenOffset { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third)],
        ) => {
            // `sub_456DC0`: fields 7/8 and field 6.
            state.screen_words.insert(7, *first);
            state.screen_words.insert(8, *second);
            state.screen_words.insert(6, *third);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenField {
                error_mask: _,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_456690`/`sub_457C90` store one dword at the screen field.
            state.screen_words.insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePair { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456D60` stores the pair at fields 15/16.
            state.screen_words.insert(15, *first);
            state.screen_words.insert(16, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenFlag { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            // `sub_457CA0` stores the boolean at field 37.
            state.screen_words.insert(37, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenPair { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456BE0` stores the pair at fields 17/18.
            state.screen_words.insert(17, *first);
            state.screen_words.insert(18, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenOffsetDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third)],
        ) => {
            // `sub_456DC0` without a target selector.
            state.screen_words.insert(7, *first);
            state.screen_words.insert(8, *second);
            state.screen_words.insert(6, *third);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScaleDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456D80` without a target selector.
            let first = f32::from_bits(*first);
            let second = f32::from_bits(*second);
            state.screen_words.insert(9, first.to_bits());
            state.screen_words.insert(10, second.to_bits());
            state.screen_words.insert(15, (first * 0.5).to_bits());
            state.screen_words.insert(16, (second * 0.5).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRgbDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, red), (CmvsPs2aCommandStackWordKind::OpaqueU32, green), (CmvsPs2aCommandStackWordKind::OpaqueU32, blue)],
        ) => {
            let hundredths = |bits: u32| -> u32 {
                ((f32::from_bits(bits) * 100.0) as i32 as f32 / 100.0).to_bits()
            };
            state.screen_words.insert(1, hundredths(*red));
            state.screen_words.insert(2, hundredths(*green));
            state.screen_words.insert(3, hundredths(*blue));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRotationDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, angle)],
        ) => {
            let scaled = (f32::from_bits(*angle) * 100.0) as i32;
            let magnitude = scaled.unsigned_abs() % 36_000;
            state
                .screen_words
                .insert(14, (magnitude as f32 / 100.0).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenFieldDirect {
                error_mask: _,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.screen_words.insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePairDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            state.screen_words.insert(15, *first);
            state.screen_words.insert(16, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreScriptSlotPresentationState {
                slot_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            let slot_key = u16::try_from(slot_table_dword_index)
                .ok()
                .and_then(|table| slot_object_key(table, *slot).ok());
            let occupied = slot_key
                .as_ref()
                .is_some_and(|key| state.slot_objects.contains_key(key));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // A texture parent's decoded resource is the canonical
            // presentation state for a script slot (`sub_484020`'s status
            // triple collapses to ready-with-resource in headless).
            let has_resource = state
                .texture_parents
                .get(&(*slot as u8))
                .is_some_and(|texture| texture.resource.is_some());
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(has_resource));
            if has_resource {
                state.interpreter_words.insert(result_field_offset + 4, 0);
                state.interpreter_words.insert(result_field_offset + 8, 0);
                state.interpreter_words.insert(result_field_offset + 12, 1);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word,
                result_field_offset,
                aux_field_offset,
            },
            [],
        ) => {
            // `sub_47A0B0`/`sub_479D70`/`sub_479E30`/`sub_47A070`/`sub_479FB0`
            // publish `(scene[base] != 0, scene[base + 1] != 0)`.
            let first = state.scene_words.get(&base_word).copied().unwrap_or(0);
            let second = state
                .scene_words
                .get(&(base_word + 1))
                .copied()
                .unwrap_or(0);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(first != 0));
            state
                .interpreter_words
                .insert(aux_field_offset, u32::from(second != 0));
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word }, []) => {
            // `sub_4616E0`/`sub_4612B0`/`sub_461310`/`sub_4616C0`/
            // `sub_4613D0` clear the group's base and `base + 2` words and
            // leave `base + 1` untouched.
            state.scene_words.remove(&base_word);
            state.scene_words.remove(&(base_word + 2));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::TogglePresentationMode {
                first_field_offset,
                second_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => Ok(Some(CmvsPs2aVmAction::TogglePresentationMode {
            first_field_offset,
            second_field_offset,
            enabled: *enabled != 0,
        })),
        (CmvsPs2aCommandEffectKind::ResetPresentationBuffers, []) => {
            // `sub_45C5A0` clears presentation-manager counters only; the
            // headless presentation has no observable counterpart.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SelectPresentationSlot {
                slot_table_dword_index,
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _selector)],
        ) => {
            let _ = slot_table_dword_index;
            if *slot > max_slot {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            // The chosen slot becomes the presentation target (`sub_445E60`);
            // the headless compositor keeps it in the interpreter words.
            state.interpreter_words.insert(0x7421_0020, *slot);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SelectRenderViewport {
                slot_table_dword_index,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _viewport)],
        ) => {
            let key = slot_object_key(
                u16::try_from(slot_table_dword_index).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot table offset overflowed",
                    )
                })?,
                *slot,
            )?;
            if *slot >= 256 || !state.slot_objects.contains_key(&key) {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
            }
            // The renderer viewport selection has no observable headless
            // counterpart; the compositor renders the full stage.
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
