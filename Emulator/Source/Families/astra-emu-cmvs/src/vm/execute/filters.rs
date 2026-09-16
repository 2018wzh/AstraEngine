use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::RunFilterGraphControl {
                table_byte_offset,
                slot,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, control)],
        ) => {
            // `sub_47A6B0` clears the flag word first, then dispatches on the
            // owner handle word. A missing handle returns 0x4004 (pop and
            // continue). Every other healthy state returns 0xA000 (wait, no
            // pop): zero control falls through the original branch chain to
            // the same wait, and a non-zero control only resolves immediately
            // through the recovered skip flags, which destroy the owner and
            // set the flag word to one (0xC004). The owner fault word and the
            // input-subsystem probes stay outside the recovered subset: the
            // constructor zeroes the fault word and no recovered command
            // writes it, so the health check passes.
            state.interpreter_words.insert(field_offset, 0);
            let handle = state
                .interpreter_words
                .get(&u32::from(table_byte_offset))
                .copied()
                .unwrap_or(0);
            if handle == 0 {
                // 0x4004: the contract pop already matches the original.
                return Ok(None);
            }
            let skip_wait = *control != 0
                && (state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0
                    || state.interpreter_words.get(&1464).copied().unwrap_or(0) != 0);
            if skip_wait {
                state.interpreter_words.insert(field_offset, 1);
                destroy_filter_graph_owner(state, table_byte_offset, slot);
                return Ok(None);
            }
            state.filter_graph_input_await = true;
            Ok(Some(CmvsPs2aVmAction::FilterGraphInputWait))
        }
        (
            CmvsPs2aCommandEffectKind::ReplaceFilterChainSlot {
                table_byte_offset,
                max_bank,
                max_channel_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *bank > max_bank || *channel > max_channel_id {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            // The previous occupant is freed and a fresh chain object takes
            // the slot; only occupancy and identity are observable.
            let key = slot_object_key(table_byte_offset, *bank)?;
            state.slot_object_seed_words.remove(&key);
            let bank_index = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            state.filter_chain_records.remove(&bank_index);
            state.filter_chain_record_order.remove(&bank_index);
            state.filter_chain_active_records.remove(&bank_index);
            state.filter_chain_channels.remove(&bank_index);
            let identity = state.next_slot_object_id;
            state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                    "CMVS slot object identity space overflowed",
                )
            })?;
            state.slot_objects.insert(key, identity);
            state.filter_chain_channels.insert(
                bank_index,
                u8::try_from(*channel).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                        "CMVS filter-chain channel exceeds the recovered bound",
                    )
                })?,
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyFilterChainSlot {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            if *bank > u32::from(max_bank) {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            let key = slot_object_key(table_byte_offset, u32::from(bank))?;
            state.slot_objects.remove(&key);
            state.slot_object_seed_words.remove(&key);
            state.filter_chain_records.remove(&bank);
            state.filter_chain_record_order.remove(&bank);
            state.filter_chain_active_records.remove(&bank);
            state.filter_chain_channels.remove(&bank);
            state.filter_chain_apply_revisions.remove(&bank);
            if state.filter_chain_selection_await == Some(bank) {
                state.filter_chain_selection_await = None;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryFilterChainSlot {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, record_key)],
        ) => {
            // `sub_468560` allocates a keyed queue node on a live bank. Host
            // allocation exhaustion is represented by the deterministic
            // record budget rather than ambient allocator behavior.
            let key = slot_object_key(table_byte_offset, *bank);
            let live =
                *bank <= max_bank && key.is_ok() && state.slot_objects.contains_key(&key.unwrap());
            if live {
                let bank = u8::try_from(*bank).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                        "CMVS filter-chain bank exceeds the recovered bound",
                    )
                })?;
                let records = state.filter_chain_records.entry(bank).or_default();
                if !records.contains_key(record_key)
                    && records.len() >= MAX_FILTER_CHAIN_RECORDS_PER_BANK
                {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_FILTER_CHAIN_BUDGET",
                        "CMVS filter-chain record budget is exhausted",
                    ));
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    records.entry(*record_key)
                {
                    entry.insert(CmvsFilterChainRecord::default());
                    state
                        .filter_chain_record_order
                        .entry(bank)
                        .or_default()
                        .push(*record_key);
                }
                state.interpreter_words.insert(result_field_offset, 1);
            } else {
                let flags = state
                    .interpreter_words
                    .get(&u32::from(error_flag_field_offset))
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(u32::from(error_flag_field_offset), flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::UpdateFilterChainRecord {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, record_key), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            if let Some(record) = state
                .filter_chain_records
                .get_mut(&bank)
                .and_then(|records| records.get_mut(record_key))
            {
                record.words = [*first, *second, *third, *fourth];
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::UpdateFilterChainParameterBlock {
                table_byte_offset,
                max_bank,
                backend_mode_address,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, record_key), (CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::OpaqueU32, word), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth), (CmvsPs2aCommandStackWordKind::OpaqueU32, sixth)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            let Some(record) = state
                .filter_chain_records
                .get_mut(&bank)
                .and_then(|records| records.get_mut(record_key))
            else {
                return Ok(None);
            };
            if *selector >= 4 {
                return Ok(None);
            }
            let block = CmvsFilterChainParameterBlock {
                word: *word,
                short_words: [
                    *first as u16,
                    *second as u16,
                    *third as u16,
                    *fourth as u16,
                    *fifth as u16,
                    *sixth as u16,
                ],
            };
            let alternate_backend = state
                .process_global_words
                .get(&backend_mode_address)
                .copied()
                .unwrap_or(0)
                != 0;
            if alternate_backend {
                match *selector {
                    0 => {
                        record.parameter_blocks.insert(0, block);
                    }
                    1 => {
                        record.parameter_blocks.insert(1, block);
                        record.parameter_blocks.insert(2, block);
                    }
                    2 => {
                        record.parameter_blocks.insert(3, block);
                    }
                    _ => {}
                }
            } else {
                record.parameter_blocks.insert(
                    u8::try_from(*selector).map_err(|_| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                            "CMVS filter-chain selector exceeds the recovered bound",
                        )
                    })?,
                    block,
                );
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyFilterChainRecords {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_BANK",
                    "CMVS filter-chain bank exceeded the recovered range",
                )
            })?;
            let revision = state
                .filter_chain_apply_revisions
                .get(&bank)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FILTER_REVISION",
                        "CMVS filter-chain apply revision overflowed",
                    )
                })?;
            state.filter_chain_apply_revisions.insert(bank, revision);
            // Applying the chain renders the registered entries; the active
            // selection survives the apply so a confirm key returns it.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PollFilterChainSelection {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            // The original poll (`sub_468740`) is synchronous: it reads the
            // live input latches and returns the active id, -1 while idle or
            // -10 when the confirm latch does not match. A control delivered
            // for the previous wait is consumed here so the script observes
            // the result in the same dispatch pass.
            if let Some(control) = state.filter_chain_pending_control.take() {
                let result = apply_filter_chain_control(state, bank, &control);
                // A confirm result survives later bank rebuilds so the
                // provider can resolve the scenario name when `command 128`
                // reads the emptied global slot.
                if result != u32::MAX && result != (-10_i32) as u32 {
                    state.interpreter_words.insert(0x7421_0010, result);
                }

                state.interpreter_words.insert(result_field_offset, result);
                state.filter_chain_selection_await = None;
                return Ok(None);
            }
            state
                .interpreter_words
                .insert(result_field_offset, u32::MAX);
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.sel_trace",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            state.filter_chain_selection_await = Some(bank);
            Ok(Some(CmvsPs2aVmAction::FilterChainSelectionWait { bank }))
        }
        (
            CmvsPs2aCommandEffectKind::ReadActiveFilterChainSelection {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            let result = state
                .filter_chain_active_records
                .get(&bank)
                .copied()
                .unwrap_or(u32::MAX);
            state.interpreter_words.insert(result_field_offset, result);
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
