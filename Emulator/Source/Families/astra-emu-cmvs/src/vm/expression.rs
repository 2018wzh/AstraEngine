use super::*;

/// One entry of the evaluator-local stack retained by the recovered CMVS
/// 3.90 `0x200` evaluator. The tag is the original token opcode; the value
/// is the raw word pushed alongside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StackExpressionEntry {
    pub(super) tag: u16,
    pub(super) value: u32,
}

/// The process-indexed slot the original evaluator writes after every
/// completed `0x200` evaluation (`sub_48AC50(4096, result)`).
pub(super) const STACK_EXPRESSION_RESULT_INDEX: u32 = 4096;
/// The interpreter system-register field base behind tag `0x10F` reads and
/// writes; registers 0..=6 map to consecutive dwords.
pub(super) const SYSTEM_REGISTER_FIELD_BASE: u32 = 81220;

/// Executes one recovered `0x200` stack expression against the VM state.
///
/// The recovered subset covers plain `0x100`/`0x12a` literals, nested
/// `0x200` sub-expressions, the `0x101` indexed-prefix, the `0x170`
/// assignment postfix and the terminator write proven by `sub_46E740`.
/// Every other token, target tag or coercion stays fail-closed.
pub(super) fn run_stack_expression(
    state: &mut CmvsPs2aVmState,
    expression: &crate::CmvsPs2aStackExpression,
) -> Result<(), CoreError> {
    let mut stack: Vec<StackExpressionEntry> = Vec::new();
    eval_stack_expression_nodes(state, &mut stack, &expression.nodes)?;
    let [final_entry] = stack.as_slice() else {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
            "CMVS PS2A stack expression did not reduce to one value",
        ));
    };
    let result = coerce_stack_expression_entry(state, *final_entry)?;
    state.current_value = Some(result);
    state
        .process_indexed_words
        .insert(STACK_EXPRESSION_RESULT_INDEX, result);
    state.condition_flag = result != 0;
    state
        .interpreter_words
        .insert(81212, u32::from(result != 0));
    Ok(())
}

/// The bounded global string-slot bound proven by `sub_48AC90`/`sub_48AA40`.
pub(super) const MESSAGE_STRING_SLOT_MAX: u32 = 0x7f;
/// The interpreter field holding the final tagged word of a `0x201`
/// evaluation (`sub_46F180` writes it at byte offset 81216).
pub(super) const VALUE_EXPRESSION_RESULT_FIELD: u32 = 81216;

/// One payload-free segment of the recovered ephemeral message buffer.
/// String content itself stays owned by the active script pool or slot
/// table; only the ordered references are retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsMessageBufferSegment {
    /// The rendering of one bounded global string slot (`sub_48AA40`).
    StringSlot { index: u32 },
    /// One resolved tag-zero private string reference from the active
    /// script's pool.
    PoolString {
        reference: CmvsPs2aPrivateStringReference,
    },
}

/// Executes one recovered `0x201` value expression against the VM state.
///
/// The recovered subset follows `sub_46F180`: the assembly buffer is reset,
/// nested `0x200` expressions and plain value tokens push tagged entries,
/// the non-assignment postfix operators materialize their operands into the
/// buffer, the `0x170` postfix materializes and stores through the target
/// tag, and a remaining leading entry installs the final tagged word.
/// Every other token or store target stays fail-closed.
pub(super) fn run_value_expression(
    state: &mut CmvsPs2aVmState,
    expression: &crate::CmvsPs2aValueExpression,
) -> Result<(), CoreError> {
    // The original copies an empty default string into the assembly buffer
    // before reading any token.
    state.message_buffer_segments.clear();
    let mut entries: Vec<(u16, u32)> = Vec::new();
    for token in &expression.tokens {
        match token {
            crate::CmvsPs2aValueExpressionToken::NestedStackExpression { expression, .. } => {
                run_stack_expression(state, expression)?;
                let value = state.current_value.ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS nested value expression left no current value",
                    )
                })?;
                entries.push((0x121, value | 0x8000_0000));
            }
            crate::CmvsPs2aValueExpressionToken::Value {
                opcode, payload, ..
            } => {
                entries.push((*opcode, *payload));
            }
            crate::CmvsPs2aValueExpressionToken::Operator {
                opcode: opcode @ 0x160..=0x172,
                ..
            } if *opcode != 0x170 => {
                let (top_tag, top_value) = entries.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS value-expression operator has no top operand",
                    )
                })?;
                let (second_tag, second_value) = entries.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS value-expression operator has no second operand",
                    )
                })?;
                append_value_entry_to_buffer(state, top_tag, top_value)?;
                append_value_entry_to_buffer(state, second_tag, second_value)?;
            }
            crate::CmvsPs2aValueExpressionToken::Operator { opcode: 0x170, .. } => {
                let (_, value_word) = entries.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS value-expression assignment has no value operand",
                    )
                })?;
                let (target_tag, target_payload) = entries.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS value-expression assignment has no target operand",
                    )
                })?;
                // `sub_46D270(target_tag, value_word)` materializes the
                // value into the assembly buffer; `sub_46E250` then stores
                // the buffer through the target tag.
                append_value_entry_to_buffer(state, target_tag, value_word)?;
                materialize_message_buffer(state, target_tag, target_payload)?;
            }
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                    "CMVS PS2A value expression token is not executable in the recovered subset",
                ));
            }
        }
    }
    // The final switch inspects the leading entry; numeric and unrecovered
    // tags leave the current value untouched, matching the default return.
    if let Some((tag, payload)) = entries.first().copied() {
        let word = match tag {
            0x120 => Some(payload),
            0x121 => Some(payload | 0x8000_0000),
            0x125 => Some(payload | 0x4000_0000),
            0x127 => Some(payload | 0xC000_0000),
            0x122 => {
                let offset = u8::try_from(payload).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS value-expression stack read exceeds the word bound",
                    )
                })?;
                Some(read_stack_word_from_top(state, offset)?)
            }
            _ => None,
        };
        if let Some(word) = word {
            state
                .interpreter_words
                .insert(VALUE_EXPRESSION_RESULT_FIELD, word);
            state.current_value = Some(word);
        }
    }
    Ok(())
}

/// The recovered buffer-append side effects of `sub_46D270` for the `0x201`
/// evaluator's string tags.  Numeric and unrecovered tags resolve without a
/// buffer side effect; stack-string copies stay fail-closed.
pub(super) fn append_value_entry_to_buffer(
    state: &mut CmvsPs2aVmState,
    tag: u16,
    payload: u32,
) -> Result<(), CoreError> {
    match tag {
        0x120 | 0x125 | 0x127 => {
            let reference = tag_zero_reference(payload)?;
            state
                .message_buffer_segments
                .push(CmvsMessageBufferSegment::PoolString { reference });
        }
        0x121 => {
            let index = payload & 0x3fff_ffff;
            if index <= MESSAGE_STRING_SLOT_MAX {
                state
                    .message_buffer_segments
                    .push(CmvsMessageBufferSegment::StringSlot { index });
            }
        }
        0x122 => {
            let offset = u8::try_from(payload).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                    "CMVS value-expression stack read exceeds the word bound",
                )
            })?;
            let word = read_stack_word_from_top(state, offset)?;
            match word & 0xc000_0000 {
                0 => {
                    let reference = tag_zero_reference(word)?;
                    state
                        .message_buffer_segments
                        .push(CmvsMessageBufferSegment::PoolString { reference });
                }
                0x4000_0000 => {
                    let reference = tag_zero_reference(word & 0x3fff_ffff)?;
                    state
                        .message_buffer_segments
                        .push(CmvsMessageBufferSegment::PoolString { reference });
                }
                0x8000_0000 => {
                    let index = word & 0x3fff_ffff;
                    if index <= MESSAGE_STRING_SLOT_MAX {
                        state
                            .message_buffer_segments
                            .push(CmvsMessageBufferSegment::StringSlot { index });
                    }
                }
                _ => {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
                        "CMVS stack-string buffer copy is not recovered",
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// The recovered `sub_46E250` store target behind the `0x201` assignment:
/// tag `0x121` copies the assembly buffer into one bounded global string
/// slot. Other targets stay fail-closed until their stores are recovered.
pub(super) fn materialize_message_buffer(
    state: &mut CmvsPs2aVmState,
    target_tag: u16,
    target_payload: u32,
) -> Result<(), CoreError> {
    match target_tag {
        0x121 => {
            let index = target_payload & 0x3fff_ffff;
            if index <= MESSAGE_STRING_SLOT_MAX {
                state
                    .message_string_slots
                    .insert(index, state.message_buffer_segments.clone());
            }
            Ok(())
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_VALUE_EXPRESSION",
            "CMVS message-buffer store target is not recovered",
        )),
    }
}

pub(super) fn eval_stack_expression_nodes(
    state: &mut CmvsPs2aVmState,
    stack: &mut Vec<StackExpressionEntry>,
    nodes: &[crate::CmvsPs2aStackExpressionNode],
) -> Result<(), CoreError> {
    for node in nodes {
        match node {
            crate::CmvsPs2aStackExpressionNode::Value {
                opcode:
                    opcode @ (0x100 | 0x12a | 0x10e | 0x108 | 0x10f | 0x103 | 0x104 | 0x106 | 0x107
                    | 0x10a | 0x10b),
                payload,
                ..
            } => stack.push(StackExpressionEntry {
                tag: *opcode,
                value: *payload,
            }),
            crate::CmvsPs2aStackExpressionNode::Nested { expression } => {
                // The original evaluates the nested form recursively, which
                // updates the current value, result slot and condition flag,
                // and then pushes the new current value as a plain literal.
                run_stack_expression(state, expression)?;
                let value = state.current_value.ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS nested stack expression left no current value",
                    )
                })?;
                stack.push(StackExpressionEntry { tag: 0x100, value });
            }
            crate::CmvsPs2aStackExpressionNode::Prefix {
                opcode: opcode @ (0x101 | 0x102),
                operands,
                ..
            } => {
                let [operand] = operands.as_slice() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS indexed prefix must carry one operand expression",
                    ));
                };
                run_stack_expression(state, operand)?;
                let value = state.current_value.ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS indexed prefix left no current value",
                    )
                })?;
                stack.push(StackExpressionEntry {
                    tag: *opcode,
                    value,
                });
            }
            crate::CmvsPs2aStackExpressionNode::Prefix {
                opcode: 0x12b,
                payload: None,
                index: None,
                operands,
                ..
            } => {
                // The original pushes the raw tag with the nested result as
                // the word; resolution later reads the process float table
                // at that index (`sub_46D270` case 0x12B via `sub_48A9F0`).
                let [operand] = operands.as_slice() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS float-variable prefix must carry one operand expression",
                    ));
                };
                run_stack_expression(state, operand)?;
                let value = state.current_value.ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS float-variable prefix left no current value",
                    )
                })?;
                stack.push(StackExpressionEntry { tag: 0x12b, value });
            }
            crate::CmvsPs2aStackExpressionNode::Prefix {
                opcode: opcode @ (0x105 | 0x109 | 0x110 | 0x112),
                payload: Some(payload),
                index: None,
                operands,
                ..
            } => {
                // `sub_46E740` rewrites the computed-offset prefixes to the
                // base accessor family with the offset `payload + 4 *
                // operand`: `0x105`->0x104 and `0x110`->0x106 (script data
                // segment), `0x109`->0x108 and `0x112`->0x10A (frame-local
                // stack).
                let [operand] = operands.as_slice() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS computed frame-local prefix must carry one operand expression",
                    ));
                };
                run_stack_expression(state, operand)?;
                let index_value = state.current_value.ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS computed frame-local prefix left no current value",
                    )
                })?;
                let tag = match opcode {
                    0x105 => 0x104,
                    0x109 => 0x108,
                    0x110 => 0x106,
                    _ => 0x10A,
                };
                let offset = payload.wrapping_add(index_value.wrapping_mul(4));
                stack.push(StackExpressionEntry { tag, value: offset });
            }
            crate::CmvsPs2aStackExpressionNode::Operator { opcode: 0x170, .. } => {
                let value_entry = stack.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS assignment postfix has no value operand",
                    )
                })?;
                let target_entry = stack.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS assignment postfix has no target operand",
                    )
                })?;
                let value = coerce_stack_expression_entry(state, value_entry)?;
                store_stack_expression_target(state, target_entry, value)?;
                stack.push(StackExpressionEntry { tag: 0x100, value });
            }
            crate::CmvsPs2aStackExpressionNode::Operator {
                opcode: opcode @ (0x173 | 0x174),
                ..
            } => {
                // Postfix increment/decrement: the top entry resolves, the
                // adjusted value is written back through the store dispatch,
                // and the entry itself becomes a literal of the adjusted
                // value, matching the evaluator's in-place rewrite.
                let entry = stack.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS increment postfix has no operand",
                    )
                })?;
                let value = coerce_stack_expression_entry(state, entry)?;
                let adjusted = if *opcode == 0x173 {
                    value.wrapping_add(1)
                } else {
                    value.wrapping_sub(1)
                };
                store_stack_expression_target(state, entry, adjusted)?;
                stack.push(StackExpressionEntry {
                    tag: 0x100,
                    value: adjusted,
                });
            }
            crate::CmvsPs2aStackExpressionNode::Operator {
                opcode: opcode @ 0x160..=0x172,
                ..
            } if *opcode != 0x170 => {
                // The original coerces the first-pushed entry as the LEFT
                // operand and the second-pushed (top) entry as the RIGHT
                // operand, then computes LEFT op RIGHT (proven by the
                // evaluator's cmp/sub instruction order).
                let right_entry = stack.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS binary postfix has no right operand",
                    )
                })?;
                let left_entry = stack.pop().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                        "CMVS binary postfix has no left operand",
                    )
                })?;
                let left = coerce_stack_expression_entry(state, left_entry)?;
                let right = coerce_stack_expression_entry(state, right_entry)?;
                let result = apply_stack_expression_binary(*opcode, left, right);
                stack.push(StackExpressionEntry {
                    tag: 0x100,
                    value: result,
                });
            }
            _ => {
                return Err(CoreError::invalid(
                    "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
                    stack_expression_blocker_message(node),
                ));
            }
        }
    }
    Ok(())
}

/// Payload-free static blocker message discriminating the stack-expression
/// node shape that stayed unrecovered. The failing token opcode is
/// control-flow data already exposed by the census tooling, so it is
/// included to keep real-session diagnosis off the trace-script detour.
pub(super) fn stack_expression_blocker_message(
    node: &crate::CmvsPs2aStackExpressionNode,
) -> String {
    match node {
        crate::CmvsPs2aStackExpressionNode::Value { opcode, .. } => format!(
            "CMVS PS2A stack expression value token {opcode:#06x} is not executable in the recovered subset"
        ),
        crate::CmvsPs2aStackExpressionNode::Operator { opcode, .. } => format!(
            "CMVS PS2A stack expression operator token {opcode:#06x} is not executable in the recovered subset"
        ),
        crate::CmvsPs2aStackExpressionNode::Nested { .. } => {
            "CMVS PS2A nested stack expression is not executable in the recovered subset".to_owned()
        }
        crate::CmvsPs2aStackExpressionNode::Prefix { opcode, .. } => format!(
            "CMVS PS2A stack expression prefix token {opcode:#06x} is not executable in the recovered subset"
        ),
    }
}

/// The recovered binary postfix table of the `0x200` evaluator, preserving
/// the original's signed 32-bit, x86-divide and x86-shift semantics.  A zero
/// divisor yields zero, matching the original's guarded division path.
pub(super) fn apply_stack_expression_binary(opcode: u16, left: u32, right: u32) -> u32 {
    let l = i32::from_ne_bytes(left.to_ne_bytes());
    let r = i32::from_ne_bytes(right.to_ne_bytes());
    let value: i32 = match opcode {
        0x160 => l.wrapping_mul(r),
        0x161 => {
            if r == 0 {
                0
            } else {
                l.wrapping_div(r)
            }
        }
        0x162 => {
            if r == 0 {
                0
            } else {
                l.wrapping_rem(r)
            }
        }
        0x163 => l.wrapping_add(r),
        0x164 => l.wrapping_sub(r),
        0x165 => l & r,
        0x166 => l | r,
        0x167 => l ^ r,
        0x168 => l.wrapping_shl(right & 31),
        0x169 => l.wrapping_shr(right & 31),
        0x16a => i32::from(l > r),
        0x16b => i32::from(l >= r),
        0x16c => i32::from(l < r),
        0x16d => i32::from(l <= r),
        0x16e => i32::from(l != 0 && r != 0),
        0x16f => i32::from(l != 0 || r != 0),
        0x171 => i32::from(l == r),
        0x172 => i32::from(l != r),
        _ => 0,
    };
    u32::from_ne_bytes(value.to_ne_bytes())
}

/// The recovered read coercion for evaluator-local entries. Literal tags
/// pass through; the `0x101` process-indexed read returns the retained word,
/// which defaults to zero because the original table is a zero-initialized
/// global.
pub(super) fn coerce_stack_expression_entry(
    state: &CmvsPs2aVmState,
    entry: StackExpressionEntry,
) -> Result<u32, CoreError> {
    coerce_stack_expression_entry_inner(state, entry)
}

pub(super) fn coerce_stack_expression_entry_inner(
    state: &CmvsPs2aVmState,
    entry: StackExpressionEntry,
) -> Result<u32, CoreError> {
    match entry.tag {
        0x100 | 0x12a => Ok(entry.value),
        0x101 => {
            if tracing::enabled!(tracing::Level::TRACE) && matches!(entry.value, 0x4d | 0x6e) {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.proc_read",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            Ok(state
                .process_indexed_words
                .get(&entry.value)
                .copied()
                .unwrap_or(0))
        }
        0x102 => Ok(u32::from(state.process_flag_bits.contains(&entry.value))),
        0x108 | 0x10a | 0x10b => read_frame_local_word(state, signed_word(entry.value)?),
        0x103 => {
            // The original dereferences `variable_base + frame_offsets[frame]
            // - word`; call arguments remain immediately below the return PC
            // and are therefore visible through these negative offsets.
            let offset = signed_word(entry.value)?.wrapping_neg();
            read_frame_local_word(state, offset)
        }
        0x104 | 0x106 | 0x107 => {
            // `sub_46D270` groups these tags onto one read: the dword at
            // the current frame's runtime data segment plus the raw byte
            // offset (unaligned like the original x86 access).
            read_script_data_segment_word(state, entry.value)
        }
        0x10f => {
            // `sub_46D270` case 0x10F reads system registers 0..=6 as the
            // interpreter words at byte offset 81220+4i and registers
            // 7..=10 as the float registers at 81248+4(i-7) cast to int;
            // higher indices return zero in the original.
            match entry.value {
                0..=6 => Ok(state
                    .interpreter_words
                    .get(&(SYSTEM_REGISTER_FIELD_BASE + 4 * entry.value))
                    .copied()
                    .unwrap_or(0)),
                7..=10 => {
                    let field = 81248 + 4 * (entry.value - 7);
                    let bits = state.interpreter_words.get(&field).copied().unwrap_or(0);
                    Ok(f32::from_bits(bits) as i32 as u32)
                }
                _ => Ok(0),
            }
        }
        0x12b => {
            if entry.value >= MAX_PROCESS_FLOAT_WORDS {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_PROCESS_FLOAT",
                    "CMVS float variable index exceeds the recovered table budget",
                ));
            }
            let bits = state
                .process_float_words
                .get(&entry.value)
                .copied()
                .unwrap_or(0);
            Ok(f32::from_bits(bits) as i32 as u32)
        }
        0x10e => {
            // `sub_46D270` reads the word at `script_handle + 48 + 4*index`,
            // i.e. the current frame's PS2A name-index entry.  The original
            // read is unbounded; the recovered reader fails closed instead.
            let table = state
                .script_name_indices
                .get(&state.current_frame)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_SCRIPT_FRAME",
                        "CMVS name-index read has no loaded script frame",
                    )
                })?;
            let index = usize::try_from(entry.value).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_NAME_INDEX",
                    "CMVS name-index read exceeds platform bounds",
                )
            })?;
            table.get(index).copied().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_NAME_INDEX",
                    "CMVS name-index read is outside the loaded table",
                )
            })
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
            "CMVS PS2A stack expression coercion is not recovered for this tag",
        )),
    }
}
