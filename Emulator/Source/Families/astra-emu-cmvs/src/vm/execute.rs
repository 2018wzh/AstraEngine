use super::*;

/// Whether an effect channel index below 8 holds a live channel object, the
/// recovered guard every channel-scoped handler applies through
/// `this[channel + 742]`.
pub(super) fn effect_channel_is_occupied(state: &CmvsPs2aVmState, channel: u32) -> bool {
    channel <= 7 && state.slot_objects.contains_key(&(742_u32 << 8 | channel))
}

pub(super) fn execute_command(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    stack_pop_bytes: u8,
    stack_words: &[crate::CmvsPs2aCommandStackWord],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    // The pop width and the read set are independent in the original:
    // handlers such as `sub_483B10` inspect stack words without popping
    // them. Each read is bounds-checked individually below.
    if effect_kind == CmvsPs2aCommandEffectKind::StopDispatch {
        state.dispatch_stopped = true;
        return Ok(None);
    }
    let mut values = Vec::with_capacity(stack_words.len());
    for word in stack_words {
        let value = read_stack_word_from_top(state, word.offset_from_top_bytes)?;
        values.push((word.kind, value));
    }
    if effect_trace_enabled() && is_effect_trace_target(effect_kind) {
        tracing::trace!(
            event = "astra.emu.cmvs.vm.effect",
            pc = state.program_counter,
            frame = state.current_frame
        );
    }
    drop_stack_bytes(state, u16::from(stack_pop_bytes))?;
    match (effect_kind, values.as_slice()) {
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
            let reference = tag_zero_reference(*value)?;
            state.interpreter_string_buffers.insert(
                buffer_offset,
                vec![
                    CmvsStringSegment::InterpreterPrefixField {
                        field_offset: prefix_field_offset,
                    },
                    CmvsStringSegment::PrivateString(reference),
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
            CmvsPs2aCommandEffectKind::RequestWindowCaption {
                prefix_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let reference = tag_zero_reference(*value)?;
            let mut segments = Vec::with_capacity(2);
            if let Some(field_offset) = prefix_field_offset {
                segments.push(CmvsStringSegment::InterpreterPrefixField { field_offset });
            }
            segments.push(CmvsStringSegment::PrivateString(reference));
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
            // The length bound depends on the resolved text, which only the
            // host can read; the host applies either the buffer write or the
            // error-flag mask exactly as `sub_48A670` does.
            let reference = tag_zero_reference(*value)?;
            Ok(Some(CmvsPs2aVmAction::StoreBoundedString {
                buffer_offset,
                max_text_bytes,
                error_flag_field_offset,
                error_flag_mask,
                reference,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::BuildPrefixedWindowCaption {
                prefix_field_offset,
                buffer_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let reference = tag_zero_reference(*value)?;
            // `sub_48A710` stores the resolved reference into the caption
            // buffer, then sets the window text from the assembled caption.
            state.interpreter_string_buffers.insert(
                buffer_offset,
                vec![CmvsStringSegment::PrivateString(reference)],
            );
            // The original selects the gap between an unrecovered exe
            // literal and two spaces through `sub_419DB0`; the recovered
            // model retains the proven two-space literal.
            let segments = vec![
                CmvsStringSegment::InterpreterPrefixField {
                    field_offset: prefix_field_offset,
                },
                CmvsStringSegment::LiteralSpaces { count: 2 },
                CmvsStringSegment::PrivateString(reference),
            ];
            Ok(Some(CmvsPs2aVmAction::SetWindowCaption { segments }))
        }
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
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad), (CmvsPs2aCommandStackWordKind::OpaqueU32, sub), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth), (CmvsPs2aCommandStackWordKind::OpaqueU32, sixth)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466CB0` truncates every value to `__int16` and stores the
            // six-word geometry block at `38*quad + 6*sub + 22`.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .write_quad_geometry(
                    *quad,
                    *sub,
                    [
                        *first as u16 as i16,
                        *second as u16 as i16,
                        *third as u16 as i16,
                        *fourth as u16 as i16,
                        *fifth as u16 as i16,
                        *sixth as u16 as i16,
                    ],
                );
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D10` stores four `__int16` hit-rectangle words at
            // `38*quad + 52`.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .write_quad_rect(
                    *quad,
                    [
                        *first as u16 as i16,
                        *second as u16 as i16,
                        *third as u16 as i16,
                        *fourth as u16 as i16,
                    ],
                );
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectQuad {
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
        (CmvsPs2aCommandEffectKind::NoOpCommand, []) => Ok(None),
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
            CmvsPs2aCommandEffectKind::SelectEffectQuad { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D50`: the quad record's visibility flag becomes 1 and
            // the bound element is shown.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_quad_visible(*quad, true);
            Ok(Some(CmvsPs2aVmAction::SelectEffectQuad {
                channel: *channel,
                quad: *quad,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::DeselectEffectQuad { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D90`: the quad record's visibility flag becomes 0 and
            // the bound element is hidden.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_quad_visible(*quad, false);
            Ok(Some(CmvsPs2aVmAction::DeselectEffectQuad {
                channel: *channel,
                quad: *quad,
            }))
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
            CmvsPs2aCommandEffectKind::QueryTextureManagerState {
                state_field_offset,
                aux_field_offset,
            },
            [],
        ) => {
            // The headless texture manager never has a pending load or a
            // failed operation, so the recovered manager status pair reads
            // as idle/zero (`sub_479C20` -> `sub_45CCB0`).
            state.interpreter_words.insert(state_field_offset, 0);
            state.interpreter_words.insert(aux_field_offset, 0);
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
            CmvsPs2aCommandEffectKind::ActivateEffectQuad { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (742_u32 << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_47F330` stores the selected animation frame at word
            // `38*quad + 38` and, when the frame actually changes, copies the
            // frame's geometry sub-record onto the bound element through
            // `sub_446B30`. The element surface is not modelled yet, so only
            // the frame word is persisted.
            let frame = *value as u16;
            let channel_object = state.effect_channels.entry(*channel as u8).or_default();
            if channel_object.quad_frame(*quad) != frame {
                channel_object.set_quad_frame(*quad, frame);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryNewestSaveSlot {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _start), (CmvsPs2aCommandStackWordKind::OpaqueU32, _count)],
        ) => {
            // No writable save files exist in the headless session, so the
            // newest-save scan returns the canonical "none" slot.
            state
                .interpreter_words
                .insert(result_field_offset, u32::MAX);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectQuadState {
                effect_table_dword_index,
                empty_error_mask,
                result0_field_offset,
                result1_field_offset,
                result2_field_offset,
                result3_field_offset,
                result8_field_offset,
                result9_field_offset,
                ..
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _index)],
        ) => {
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, empty_error_mask)?;
                return Ok(None);
            }
            // The headless effect graph holds no animated quad state, so the
            // selected sub-object reports zeros for all six fields.
            for offset in [
                result0_field_offset,
                result1_field_offset,
                result2_field_offset,
                result3_field_offset,
                result8_field_offset,
                result9_field_offset,
            ] {
                state.interpreter_words.insert(offset, 0);
            }
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
            CmvsPs2aCommandEffectKind::QueryEffectQuadActive {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, quad)],
        ) => {
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0002_0000)?;
                return Ok(None);
            }
            // `sub_47F470` reads the quad record's byte at offset
            // `76*quad + 40` and stores `(byte == 0)`: 1 while the quad is
            // hidden, 0 once case 402 shows it. The script gates the
            // quad-update path on this flag, so it must follow the
            // recovered visibility transitions rather than a constant.
            let hidden = state
                .effect_channels
                .get(&(*channel as u8))
                .is_none_or(|object| !object.quad_visible(*quad));
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hidden));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectPosition {
                result_x_field_offset,
                result_y_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            // The headless engine has no animated effect position.
            state.interpreter_words.insert(result_x_field_offset, 0);
            state.interpreter_words.insert(result_y_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectPointerHit {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_x), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_y), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_w), (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_h)],
        ) => {
            // `sub_47F2C0` requires an occupied channel and forwards the
            // rectangle to the hit test (`sub_466A50`). The stage scale and
            // offset are identity in headless, so the test compares the
            // streamed pointer position words against the rectangle.
            let effect_key = (742_u32 << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            let pointer_x = state.pointer_x.max(0) as u32;
            let pointer_y = state.pointer_y.max(0) as u32;
            let x = *rect_x;
            let y = *rect_y;
            let hit = x <= pointer_x
                && y <= pointer_y
                && x.saturating_add(*rect_w) > pointer_x
                && y.saturating_add(*rect_h) > pointer_y;
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hit));
            tracing::trace!(
                event = "astra.emu.cmvs.vm.hit_trace",
                pc = state.program_counter,
                frame = state.current_frame
            );
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
            CmvsPs2aCommandEffectKind::SelectSystemCursor {
                manager_field_offset,
                current_index_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            let key = component_field_key(manager_field_offset, current_index_field_offset);
            if state.component_words.get(&key) == Some(index) {
                return Ok(None);
            }
            state.component_words.insert(key, *index);
            Ok(Some(CmvsPs2aVmAction::SelectSystemCursor { index: *index }))
        }
        (CmvsPs2aCommandEffectKind::RequestSystemStateLoad { path_buffer_offset }, []) => {
            let request = CmvsPs2aStorageRequest::LoadSystemState { path_buffer_offset };
            state.storage_await = Some(request.clone());
            Ok(Some(CmvsPs2aVmAction::StorageRequest(request)))
        }
        (
            CmvsPs2aCommandEffectKind::CallScript,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            if *slot >= crate::CMVS_MAX_SCRIPT_CALL_INDEX {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS nested script frame index exceeds the recovered bound",
                ));
            }
            let frame = u16::try_from(*slot + 1).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS nested script frame index overflowed",
                )
            })?;
            let name = script_name_reference(state, *name)?;
            // The loader pops both operands through the contract's
            // `stack_pop_bytes` before this branch runs; the dispatcher
            // result 0 adds no further pop.  It then pushes the frame
            // counter, the advanced PC and the previous frame index before
            // the host transfers dispatch to the loaded script.
            let frame_counter = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or(0);
            let return_pc = state.program_counter.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_PC",
                    "CMVS PS2A program counter overflowed",
                )
            })?;
            push_word(state, frame_counter)?;
            push_word(state, return_pc)?;
            push_word(state, u32::from(state.current_frame))?;
            if vm_trace_enabled() {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.stack_trace",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            state.current_frame = frame;
            Ok(Some(CmvsPs2aVmAction::CallScript { frame, name }))
        }
        (
            CmvsPs2aCommandEffectKind::ReloadRootScript,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            let name = script_name_reference(state, *name)?;
            // `sub_4781B0` resets the data stack, the frame stack-table
            // words and the current frame before the new root script takes
            // over; the expression-result field is zeroed with its flag bit.
            state.stack_bytes.clear();
            state.stack_initialized.clear();
            state.stack_cursor_bytes = 0;
            state.call_frame_bases.clear();
            state.call_frame_bases.push(0);
            state.current_frame = 0;
            state.current_value = None;
            state.condition_flag = false;
            state.interpreter_words.insert(80952, 0);
            state.interpreter_words.insert(81208, 0);
            state.interpreter_words.insert(81212, 0);
            state.interpreter_words.insert(81216, 0);
            // The same loader invalidates the coroutine table lazily: a
            // record whose frame id is still the constructor default keeps
            // only its previous-slot flag cleared, with the PC word set to
            // the all-ones unregistered value (`sub_4781B0`).
            for label in 0..COROUTINE_RECORD_COUNT {
                let base = COROUTINE_RECORD_TABLE_BASE + COROUTINE_RECORD_STRIDE * label;
                let frame_id = state
                    .interpreter_words
                    .get(&(base + 4))
                    .copied()
                    .unwrap_or(0);
                if frame_id == 0 {
                    state.interpreter_words.insert(base + 8, u32::MAX);
                    state.interpreter_words.insert(base, 0);
                }
            }
            Ok(Some(CmvsPs2aVmAction::ReloadRootScript { name }))
        }
        (
            CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord {
                record_table_base_offset,
                record_stride_bytes,
                max_index_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, label)],
        ) => {
            // `sub_47CB00` masks the label like the record writer does and
            // indexes the same 28-byte table.
            let label = *label & u32::from(max_index_mask);
            let base = u32::from(record_table_base_offset)
                .checked_add(u32::from(record_stride_bytes).wrapping_mul(label))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FIELD",
                        "CMVS coroutine record offset overflowed",
                    )
                })?;
            let (recorded_frame, recorded_pc) = match (
                state.interpreter_words.get(&(base + 4)),
                state.interpreter_words.get(&(base + 8)),
            ) {
                (Some(frame), Some(pc)) => (*frame, *pc),
                // The original would jump to an unregistered address; the
                // recovered subset blocks instead of emulating that fault.
                _ => {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_COROUTINE",
                        "CMVS coroutine label has no registered continuation record",
                    ))
                }
            };
            if recorded_pc == u32::MAX {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_COROUTINE",
                    "CMVS coroutine label was invalidated before the resume",
                ));
            }
            let frame = u16::try_from(recorded_frame).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_COROUTINE",
                    "CMVS coroutine record frame index overflowed",
                )
            })?;
            // The original advances the PC past the opcode before pushing
            // the return record (`sub_47CB00`); the frame-counter value is
            // read after that advance, matching the original field order.
            let return_pc = state.program_counter.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_PC",
                    "CMVS PS2A program counter overflowed",
                )
            })?;
            let frame_counter = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or(0);
            push_word(state, frame_counter)?;
            push_word(state, return_pc)?;
            push_word(state, u32::from(state.current_frame))?;
            if vm_trace_enabled() {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.stack_trace",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            state
                .interpreter_words
                .insert(SCRIPT_FRAME_COUNTER_FIELD, 0);
            state.current_frame = frame;
            state.program_counter = recorded_pc;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RecreateSaveImageOwner {
                handle_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            // `sub_47AFF0` frees the previous owner, then `sub_459BE0`
            // zeroes the flag and progress fields of the fresh owner. The
            // published handle word keeps the version-pinned field updated
            // for snapshot parity.
            state.save_image_owner = Some(CmvsSaveImageOwnerState {
                construction_words: [*first, *second, *third, *fourth],
                enabled: false,
                progress_delta: 0,
            });
            state.interpreter_words.insert(
                u32::from(handle_field_offset),
                state.save_image_owner.as_ref().map(|_| 1_u32).unwrap_or(0),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySaveImageProgress {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_459CC0` returns the fourth construction word unchanged
            // when it is all-ones, otherwise reduced by the progress delta
            // the unrecovered render path updates.
            let value = match &state.save_image_owner {
                Some(owner) if owner.construction_words[3] == u32::MAX => u32::MAX,
                Some(owner) => owner.construction_words[3].wrapping_sub(owner.progress_delta),
                None => u32::MAX,
            };
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSaveImageEnabled,
            [(CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            // `sub_47B0F0` only touches the flag of a present owner; the
            // pop happens regardless of the owner's existence.
            if let Some(owner) = state.save_image_owner.as_mut() {
                owner.enabled = *enabled != 0;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SaveImageOwnerWaitOrDestroy {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, wait)],
        ) => match (&state.save_image_owner, *wait != 0) {
            // `sub_47B160` writes one for a missing owner and returns
            // 0x4004.
            (None, _) => {
                state.interpreter_words.insert(result_field_offset, 1);
                Ok(None)
            }
            // The set-boolean branch waits for the screenshot render to
            // complete (dispatch result 0xA000); that path is not
            // recovered and blocks instead of spinning.
            (Some(_), true) => {
                state.interpreter_words.insert(result_field_offset, 0);
                Err(invalid(
                    "ASTRA_EMU_CMVS_VM_SAVE_IMAGE",
                    "CMVS screenshot completion wait is not recovered",
                ))
            }
            // The clear-boolean branch destroys the owner and reports
            // success (dispatch result 0xC004).
            (Some(_), false) => {
                state.save_image_owner = None;
                state.interpreter_words.insert(result_field_offset, 1);
                Ok(None)
            }
        },
        (CmvsPs2aCommandEffectKind::DestroySaveImageOwner, []) => {
            // `sub_47B120` frees a present owner without further effects.
            state.save_image_owner = None;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySaveImageEnabled {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_47B0B0` combines the owner existence with its field-5
            // flag through `sub_432F60`.
            let value = u32::from(
                state
                    .save_image_owner
                    .as_ref()
                    .is_some_and(|owner| owner.enabled),
            );
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::QueryRendererDisplayMode { .. }, []) => {
            // `sub_48A570` reads the inner display-mode fields of the
            // renderer object; that object model is not recovered and the
            // register values would branch the script, so block instead.
            Err(invalid(
                "ASTRA_EMU_CMVS_VM_DISPLAY_MODE",
                "CMVS renderer display-mode registers are not recovered",
            ))
        }
        (
            CmvsPs2aCommandEffectKind::QueryPointerPosition {
                x_field_offset,
                y_field_offset,
            },
            [],
        ) => {
            // `sub_480280` publishes the persistent pointer into system
            // registers 1 and 2 through `sub_45CF40`.
            state
                .interpreter_words
                .insert(x_field_offset, state.pointer_x as u32);
            state
                .interpreter_words
                .insert(y_field_offset, state.pointer_y as u32);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRect {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, x), (CmvsPs2aCommandStackWordKind::OpaqueU32, y), (CmvsPs2aCommandStackWordKind::OpaqueU32, width), (CmvsPs2aCommandStackWordKind::OpaqueU32, height)],
        ) => {
            // `sub_480060` rejects only when the pointer is strictly outside
            // the rectangle, so both bounds are inclusive.
            let x = *x as i32;
            let y = *y as i32;
            let hit = state.pointer_x >= x
                && state.pointer_x <= x.wrapping_add(*width as i32)
                && state.pointer_y >= y
                && state.pointer_y <= y.wrapping_add(*height as i32);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hit));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRegion {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, shape), (CmvsPs2aCommandStackWordKind::OpaqueU32, left), (CmvsPs2aCommandStackWordKind::OpaqueU32, top), (CmvsPs2aCommandStackWordKind::OpaqueU32, right), (CmvsPs2aCommandStackWordKind::OpaqueU32, bottom), (CmvsPs2aCommandStackWordKind::OpaqueU32, _width), (CmvsPs2aCommandStackWordKind::OpaqueU32, _height)],
        ) => match *shape {
            // `SetRect` + `CreateRectRgnIndirect`: a GDI rectangle region is
            // half-open, covering left/top and excluding right/bottom.
            1 => {
                let hit = state.pointer_x >= *left as i32
                    && state.pointer_x < *right as i32
                    && state.pointer_y >= *top as i32
                    && state.pointer_y < *bottom as i32;
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(hit));
                Ok(None)
            }
            _ => Err(invalid(
                "ASTRA_EMU_CMVS_VM_POINTER_REGION",
                "CMVS pointer region shape rasterization is not recovered",
            )),
        },
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
        (CmvsPs2aCommandEffectKind::ResetPresentationBuffers, []) => {
            // `sub_45C5A0` clears presentation-manager counters only; the
            // headless presentation has no observable counterpart.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy {
                slot_table_dword_index,
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _selector)],
        ) => {
            let _ = slot_table_dword_index;
            if *slot > max_slot {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let key = slot_object_key(
                u16::try_from(slot_table_dword_index).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot table offset overflowed",
                    )
                })?,
                *slot,
            )?;
            let occupied = state.slot_objects.contains_key(&key);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(occupied));
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
            store_process_string_range(state, *start, *count, tag_zero_reference(*value)?)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearMessagePanel,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _voice_control)],
        ) => Ok(Some(CmvsPs2aVmAction::ClearMessagePanel)),
        (CmvsPs2aCommandEffectKind::ResetMessagePanel, []) => {
            Ok(Some(CmvsPs2aVmAction::ResetMessagePanel))
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
        (
            CmvsPs2aCommandEffectKind::MessageBody,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, body), (CmvsPs2aCommandStackWordKind::OpaqueU32, opaque_value), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => Ok(Some(CmvsPs2aVmAction::Message {
            speaker: None,
            body: tag_zero_reference(*body)?,
            opaque_value: *opaque_value,
            enabled: *enabled != 0,
        })),
        (
            CmvsPs2aCommandEffectKind::MessageSpeakerBody,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, body), (CmvsPs2aCommandStackWordKind::TaggedStringReference, speaker), (CmvsPs2aCommandStackWordKind::OpaqueU32, opaque_value), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => Ok(Some(CmvsPs2aVmAction::Message {
            speaker: Some(tag_zero_reference(*speaker)?),
            body: tag_zero_reference(*body)?,
            opaque_value: *opaque_value,
            enabled: *enabled != 0,
        })),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
