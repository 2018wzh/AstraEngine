use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StoreSettingBooleanPlain {
                setting_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            // No reset preamble: `sub_4164F0` only writes the field.
            state
                .settings_words
                .insert(setting_field_offset, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            reset_settings_object(state);
            state
                .settings_words
                .insert(setting_field_offset, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord {
                settings_field_offset,
                target_field_offset,
            },
            [],
        ) => {
            let value = state
                .settings_words
                .get(&settings_field_offset)
                .copied()
                .unwrap_or(0);
            state.interpreter_words.insert(target_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreIndexedSettingBoolean {
                target_field_offset,
                max_index,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // `sub_45C300` selects settings field 4*index for indices
            // 0..=5; negative indices fall back to field 0 and higher
            // indices resolve to zero.
            let value = if *index <= u32::from(max_index) {
                let field_offset = u16::try_from(4 * *index).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FIELD",
                        "CMVS settings field offset overflowed",
                    )
                })?;
                state
                    .settings_words
                    .get(&field_offset)
                    .copied()
                    .unwrap_or(0)
            } else if (*index as i32) < 0 {
                state.settings_words.get(&0).copied().unwrap_or(0)
            } else {
                0
            };
            state
                .interpreter_words
                .insert(target_field_offset, u32::from(value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreDerivedSettingsPair {
                first_selector_field_offset,
                second_selector_field_offset,
                result_field_offset,
                result_target_field_offset,
                flag_target_field_offset,
            },
            [],
        ) => {
            // `sub_45C390` zeroes the result field when both selectors
            // agree on zero, or when both are non-zero, then returns the
            // result field; the output flag mirrors the second selector.
            let first = state
                .settings_words
                .get(&first_selector_field_offset)
                .copied()
                .unwrap_or(0);
            let second = state
                .settings_words
                .get(&second_selector_field_offset)
                .copied()
                .unwrap_or(0);
            if (first == 0) == (second == 0) {
                state.settings_words.insert(result_field_offset, 0);
            }
            let result = state
                .settings_words
                .get(&result_field_offset)
                .copied()
                .unwrap_or(0);
            state
                .interpreter_words
                .insert(result_target_field_offset, result);
            state
                .interpreter_words
                .insert(flag_target_field_offset, u32::from(second != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSettingsWordReset {
                setting_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            reset_settings_object(state);
            state.settings_words.insert(setting_field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSettingsPredicate {
                settings_field_offset,
                target_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_45C370` requires the stack word to equal one and the
            // signed settings field to stay non-negative.
            let field = state
                .settings_words
                .get(&settings_field_offset)
                .copied()
                .unwrap_or(0);
            let result = *value == 1 && (field as i32) >= 0;
            state
                .interpreter_words
                .insert(target_field_offset, u32::from(result));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DeriveSettingsSnapshot {
                gate_field_offset,
                copy_source_field_offsets,
                copy_target_field_offsets,
                always_source_field_offset,
                always_target_field_offset,
                gate_target_field_offset,
            },
            [],
        ) => {
            // `sub_45C3E0` copies four settings fields only while the gate is
            // non-zero, always copies the fifth settings field, and returns
            // the gate through the result word.
            let gate = state
                .settings_words
                .get(&gate_field_offset)
                .copied()
                .unwrap_or(0);
            if gate != 0 {
                for (source, target) in copy_source_field_offsets
                    .iter()
                    .zip(copy_target_field_offsets.iter())
                {
                    let value = state.settings_words.get(source).copied().unwrap_or(0);
                    state.interpreter_words.insert(*target, value);
                }
            }
            let always_value = state
                .settings_words
                .get(&always_source_field_offset)
                .copied()
                .unwrap_or(0);
            state
                .interpreter_words
                .insert(always_target_field_offset, always_value);
            state
                .interpreter_words
                .insert(gate_target_field_offset, u32::from(gate != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSettingsRecord {
                record_base_field_offset,
                record_stride_bytes,
                max_index,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::BooleanU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth), (CmvsPs2aCommandStackWordKind::OpaqueU32, sixth)],
        ) => {
            // `sub_45B260` skips the write for indices at or above six.
            if *index <= u32::from(max_index) {
                let base = u32::from(record_base_field_offset)
                    .checked_add(u32::from(record_stride_bytes).wrapping_mul(*index))
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_FIELD",
                            "CMVS settings record offset overflowed",
                        )
                    })?;
                let base = u16::try_from(base).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FIELD",
                        "CMVS settings record offset exceeds the field bound",
                    )
                })?;
                let words = [u32::from(*second != 0), *third, *fourth, *fifth, *sixth];
                for (slot, word) in words.iter().enumerate() {
                    let offset = base.checked_add(4 * slot as u16).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_FIELD",
                            "CMVS settings record slot overflowed",
                        )
                    })?;
                    state.settings_words.insert(offset, *word);
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSystemSettingBooleanPair {
                first_field_offset,
                second_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, first), (CmvsPs2aCommandStackWordKind::BooleanU32, second)],
        ) => {
            reset_settings_object(state);
            state
                .settings_words
                .insert(first_field_offset, u32::from(*first != 0));
            state
                .settings_words
                .insert(second_field_offset, u32::from(*second != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoundedWord {
                setting_field_offset,
                minimum,
                maximum,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            if *value >= minimum && *value <= maximum {
                state.settings_words.insert(setting_field_offset, *value);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSystemSettingNonNegativeWord {
                setting_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            if i32::from_ne_bytes(value.to_ne_bytes()) >= 0 {
                state.settings_words.insert(setting_field_offset, *value);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSystemConfigBoolean {
                config_field_offset,
                skip_flag_field_offset,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, key), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_478EC0`: a non-zero key writes the config boolean;
            // a zero key reads the skip flag into the result word.
            if key != &0 {
                state
                    .interpreter_words
                    .insert(config_field_offset, u32::from(value != &0));
            } else {
                let skip = state
                    .interpreter_words
                    .get(&skip_flag_field_offset)
                    .copied()
                    .unwrap_or_default();
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(skip != 0));
            }
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
