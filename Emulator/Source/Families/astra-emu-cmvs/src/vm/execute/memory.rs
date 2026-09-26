use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StoreComponentConstant {
                component_offset,
                field_offset,
                value,
            },
            [],
        ) => {
            state
                .component_words
                .insert(component_field_key(component_offset, field_offset), value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset,
                value,
            },
            [],
        ) => {
            state.interpreter_words.insert(field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearInterpreterTableWord {
                table_base_offset,
                table_word_count,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // The original indexes a dword offset into the recovered word
            // family without a bounds check; anything outside the family
            // writes unrecovered interpreter state, so block it instead.
            if *index >= table_word_count {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_INTERPRETER_TABLE",
                    "CMVS interpreter table clear index is outside the recovered family",
                ));
            }
            let field_offset = table_base_offset
                + index.checked_mul(4).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_INTERPRETER_TABLE",
                        "CMVS interpreter table byte extent overflowed",
                    )
                })?;
            state.interpreter_words.insert(field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadInterpreterTableWord {
                table_base_offset,
                table_word_count,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // The recovered family is zero-initialized, so unwritten words
            // read back as zero; indices outside the family stay blocked.
            if *index >= table_word_count {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_INTERPRETER_TABLE",
                    "CMVS interpreter table load index is outside the recovered family",
                ));
            }
            let source_offset = table_base_offset
                + index.checked_mul(4).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_INTERPRETER_TABLE",
                        "CMVS interpreter table byte extent overflowed",
                    )
                })?;
            let word = state
                .interpreter_words
                .get(&source_offset)
                .copied()
                .unwrap_or(0);
            state.interpreter_words.insert(field_offset, word);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::MarkInterpreterRecordUnused {
                record_table_base_offset,
                record_count,
                record_stride_bytes,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // The original ignores out-of-range indices silently.
            if *index < record_count {
                let record_offset = record_table_base_offset
                    + index.checked_mul(record_stride_bytes).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_INTERPRETER_TABLE",
                            "CMVS interpreter record offset overflowed",
                        )
                    })?;
                state.interpreter_words.insert(record_offset, u32::MAX);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip {
                flag_field_offset,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            if *selector != 0 {
                state
                    .interpreter_words
                    .insert(flag_field_offset, u32::from(*value != 0));
            } else {
                let flag = state
                    .interpreter_words
                    .get(&flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(flag != 0));
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset,
                prefix_field_offset,
                ensure_directories,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let value = resolve_string_value(state, *value)?;
            crate::archive::native::validate_search_path(&value)?;
            state.interpreter_string_buffers.insert(
                buffer_offset,
                vec![
                    CmvsStringSegment::InterpreterPrefixField {
                        field_offset: prefix_field_offset,
                    },
                    CmvsStringSegment::OwnedString(value),
                ],
            );
            if ensure_directories {
                Ok(Some(CmvsPs2aVmAction::EnsurePathDirectories {
                    buffer_offset,
                }))
            } else {
                Ok(None)
            }
        }
        (
            CmvsPs2aCommandEffectKind::RequestWindowIcon {
                prefix_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let text = resolve_string_value(state, *value)?;
            Ok(Some(CmvsPs2aVmAction::SetWindowIcon {
                segments: vec![
                    CmvsStringSegment::InterpreterPrefixField {
                        field_offset: prefix_field_offset,
                    },
                    CmvsStringSegment::OwnedString(text),
                ],
            }))
        }
        (
            CmvsPs2aCommandEffectKind::RequestWindowCaption,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let text = resolve_string_value(state, *value)?;
            let segments = vec![CmvsStringSegment::OwnedString(text)];
            // 41B1C0 owns a copy at component+288 before SetWindowTextA.
            state
                .interpreter_string_buffers
                .insert(288, segments.clone());
            Ok(Some(CmvsPs2aVmAction::SetWindowCaption { segments }))
        }
        (
            CmvsPs2aCommandEffectKind::StoreComponentWord {
                component_field_offset,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.component_words.insert(
                component_field_key(component_field_offset, field_offset),
                *value,
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreBoundedInterpreterString {
                buffer_offset,
                max_text_bytes,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            // 48A670 measures native CP932 bytes, not Unicode characters.
            // 4712F0 reads this same buffer for the native save title/tag 256.
            let bytes = resolve_tagged_string_bytes(state, *value)?;
            if bytes.len() >= usize::from(max_text_bytes) {
                *state
                    .interpreter_words
                    .entry(u32::from(error_flag_field_offset))
                    .or_default() |= error_flag_mask;
            } else {
                let text = resolve_string_value(state, *value)?;
                state
                    .interpreter_string_buffers
                    .insert(buffer_offset, vec![CmvsStringSegment::OwnedString(text)]);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::BuildPrefixedWindowCaption {
                prefix_field_offset,
                buffer_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let suffix = resolve_string_value(state, *value)?;
            let suffix_bytes = resolve_tagged_string_bytes(state, *value)?.len();
            let Some(prefix) = state.interpreter_string_buffers.get(&prefix_field_offset) else {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_WINDOW_CAPTION",
                    "native caption prefix is not bound",
                ));
            };
            let [CmvsStringSegment::OwnedString(prefix)] = prefix.as_slice() else {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_WINDOW_CAPTION",
                    "native caption prefix is not owned",
                ));
            };
            let (prefix_bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(prefix);
            // 48A710 uses a 128-byte stack array with unbounded native copies.
            // Refuse that overflow before changing the owned suffix buffer.
            if malformed
                || prefix_bytes
                    .len()
                    .saturating_add(suffix_bytes)
                    .saturating_add(2)
                    >= 128
            {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_WINDOW_CAPTION",
                    "native caption exceeds its byte capacity",
                ));
            }
            let gap = match state.process_global_words.get(&NATIVE_FONT_SHIFT_JIS) {
                Some(0) => "  ",
                Some(_) => "\u{3000}",
                None => {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_WINDOW_CAPTION",
                        "native caption character set is not bound",
                    ))
                }
            };
            let segments = vec![CmvsStringSegment::OwnedString(format!(
                "{prefix}{gap}{suffix}"
            ))];
            state
                .interpreter_string_buffers
                .insert(buffer_offset, vec![CmvsStringSegment::OwnedString(suffix)]);
            Ok(Some(CmvsPs2aVmAction::SetWindowCaption { segments }))
        }
        (CmvsPs2aCommandEffectKind::NoOpCommand, []) => Ok(None),
        (
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset,
                result_field_offset,
            },
            [],
        ) => {
            // Dispatcher case 718 copies the pinned interpreter word into
            // the result field; unwritten words read zero.
            let value = state
                .interpreter_words
                .get(&source_field_offset)
                .copied()
                .unwrap_or(0);
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset,
                result_field_offset,
            },
            [],
        ) => {
            // Dispatcher case 750 canonicalizes the pinned interpreter word
            // to a boolean result.
            let value = state
                .interpreter_words
                .get(&source_field_offset)
                .copied()
                .unwrap_or(0);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::AppendPrivateStringToList {
                list_id,
                length_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let reference = tag_zero_reference(*value)?;
            let list = state.private_string_lists.entry(list_id).or_default();
            list.push(reference);
            let length = u32::try_from(list.len()).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_STRING_LIST",
                    "CMVS string list length overflowed",
                )
            })?;
            state.interpreter_words.insert(length_field_offset, length);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.interpreter_words.insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreRandomModulo {
                target_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bound)],
        ) => {
            // `sub_484F60` computes `rand() % bound`; the original's CRT
            // divide faults on a zero bound, so the recovered handler blocks
            // instead of inventing a result.
            if *bound == 0 {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_RANDOM",
                    "CMVS random draw has a zero modulo bound",
                ));
            }
            let next = state
                .prng_state
                .wrapping_mul(214_013)
                .wrapping_add(2_531_011);
            state.prng_state = next;
            let draw = (next >> 16) & 0x7fff;
            state
                .interpreter_words
                .insert(target_field_offset, draw % bound);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreStringLength {
                target_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, reference)],
        ) => {
            // `sub_47D220` resolves the tagged string through `sub_470D50`
            // and stores `lstrlenA`; the length comes from the frame table
            // injected at script load for pool tags, and from the assembled
            // segment lengths for the process-global string slots.
            let length = resolve_tagged_string_length(state, *reference)?;
            state.interpreter_words.insert(target_field_offset, length);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            state
                .interpreter_words
                .insert(field_offset, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterTimestampedRecord {
                table_base_offset,
                record_stride_bytes,
                max_index,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth)],
        ) => {
            // `sub_47CBE0` skips the write for indices above the bound and
            // records a host tick through `sub_41A000`. The runtime host
            // supplies the deterministic fixed-session clock before dispatch.
            if *index <= u32::from(max_index) {
                let base = u32::from(table_base_offset)
                    .checked_add(u32::from(record_stride_bytes).wrapping_mul(*index))
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_FIELD",
                            "CMVS record table offset overflowed",
                        )
                    })?;
                state.interpreter_words.insert(base, 0);
                state
                    .interpreter_words
                    .insert(base + 4, u32::from(state.current_frame));
                state.interpreter_words.insert(base + 8, *second);
                state.interpreter_words.insert(base + 12, *third);
                state.interpreter_words.insert(base + 16, *fourth);
                state.interpreter_words.insert(base + 20, *fifth);
                state
                    .interpreter_words
                    .insert(base + 24, state.session_clock_millis);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessGlobalWord { address },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.process_global_words.insert(address, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean { address },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            state
                .process_global_words
                .insert(address, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::MutateProcessFlagRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            mutate_process_flag_range(state, *start, *count, *enabled != 0)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessIndexedRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            store_process_indexed_range(state, *start, *count, *value)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessFloatRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            store_process_float_range(state, *start, *count, *value)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessStringRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            store_process_string_range(state, *start, *count, *value)?;
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
