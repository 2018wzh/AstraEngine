use super::*;

/// One entry of the `0x202` numeric evaluator's operand stack, mirroring the
/// interpreter's parallel tag/word/float arrays. `aux` is the float slot the
/// original keeps beside each entry; kind-298 resolution returns it verbatim.
#[derive(Debug, Clone, Copy)]
pub(super) struct NumericExpressionEntry {
    tag: u16,
    word: u32,
    aux: f32,
}

/// Executes the recovered `0x202` numeric-expression subset proven by
/// `sub_46F470`. Operand tokens push tag/word entries; the `0x170` postfix
/// assignment resolves the most recent entry as the value and stores it into
/// the preceding entry's target. The terminator resolves the single remaining
/// entry through the float resolver (`sub_46D630`), writes the interpreter
/// float register at byte offset 81216, and derives the branch flag from an
/// IEEE-754 zero comparison. Operator and target forms outside this recovered
/// subset stay blocking.
pub(super) fn run_numeric_expression(
    state: &mut CmvsPs2aVmState,
    expression: &crate::CmvsPs2aNumericExpression,
) -> Result<(), CoreError> {
    let mut entries: Vec<NumericExpressionEntry> = Vec::new();
    for node in &expression.nodes {
        match node {
            crate::CmvsPs2aNumericExpressionNode::StackExpression { expression } => {
                // Raw `0x200` token: the nested stack expression evaluates
                // into the current-value register and the entry is pushed as
                // a kind-256 literal of that word, matching `sub_46F470`.
                run_stack_expression(state, expression)?;
                let word = state.current_value.unwrap_or(0);
                entries.push(NumericExpressionEntry {
                    tag: 0x100,
                    word,
                    aux: f32::from_bits(word),
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Prefix {
                opcode: 0x12b,
                payload: None,
                index: None,
                operands,
                ..
            } => {
                // The nested expression computes the float-table index; the
                // entry keeps the raw tag so reads and assignments dispatch
                // through the float variable store.
                let [operand] = operands.as_slice() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
                        "CMVS PS2A numeric prefix carries an unrecovered operand shape",
                    ));
                };
                run_stack_expression(state, operand)?;
                entries.push(NumericExpressionEntry {
                    tag: 0x12b,
                    word: state.current_value.unwrap_or(0),
                    aux: 0.0,
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Value {
                opcode: 0x12a,
                payload,
                ..
            } => {
                entries.push(NumericExpressionEntry {
                    tag: 0x12a,
                    word: *payload,
                    aux: f32::from_bits(*payload),
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Value {
                opcode: 0x100,
                payload,
                ..
            } => {
                entries.push(NumericExpressionEntry {
                    tag: 0x100,
                    word: *payload,
                    aux: f32::from_bits(*payload),
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Value {
                opcode: opcode @ (0x12c | 0x132 | 0x133),
                payload,
                ..
            } => {
                // Stack-frame float read (`sub_46E440` cases 0x12c/0x132/0x133):
                // the payload offsets into the active call frame on the value
                // stack.
                let word = read_stack_frame_word(state, *payload)?;
                entries.push(NumericExpressionEntry {
                    tag: *opcode,
                    word: *payload,
                    aux: f32::from_bits(word),
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Value {
                opcode: opcode @ 0x129,
                payload,
                ..
            } => {
                // Stack-frame float at a negative offset from the frame base
                // (`sub_46E440` case 0x129): call arguments live below.
                let word = read_stack_frame_word_neg(state, *payload)?;
                entries.push(NumericExpressionEntry {
                    tag: *opcode,
                    word: *payload,
                    aux: f32::from_bits(word),
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Operator {
                opcode: opcode @ (0x160..=0x16f | 0x171 | 0x172),
                ..
            } => {
                // Binary arithmetic/comparison (`sub_46F470` -> `sub_46E740`
                // table): 0x160 mul, 0x161 div, 0x162 rem, 0x163 add,
                // 0x164 sub, 0x165 and, 0x166 or, 0x167 xor, 0x168 shl,
                // 0x169 shr, 0x16a-0x16f/0x171/0x172 comparisons.
                let Some(value_entry) = entries.pop() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
                        "CMVS PS2A numeric operator is missing its right operand",
                    ));
                };
                let Some(target_entry) = entries.pop() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
                        "CMVS PS2A numeric operator is missing its left operand",
                    ));
                };
                let right = resolve_numeric_entry(state, value_entry)? as i32;
                let left = resolve_numeric_entry(state, target_entry)? as i32;
                let result = match opcode {
                    0x160 => left.wrapping_mul(right),
                    0x161 if right != 0 => left.wrapping_div(right),
                    0x162 if right != 0 => left.wrapping_rem(right),
                    0x163 => left.wrapping_add(right),
                    0x164 => left.wrapping_sub(right),
                    0x165 => left & right,
                    0x166 => left | right,
                    0x167 => left ^ right,
                    0x168 => left.wrapping_shl(right as u32),
                    0x169 => left.wrapping_shr(right as u32),
                    0x16a => (left > right) as i32,
                    0x16b => (left <= right) as i32,
                    0x16c => (left < right) as i32,
                    0x16d => (left >= right) as i32,
                    0x16e => (left != 0 && right != 0) as i32,
                    0x16f => (left != 0 || right != 0) as i32,
                    0x171 => (left == right) as i32,
                    _ => (left != right) as i32,
                } as u32;
                entries.push(NumericExpressionEntry {
                    tag: 0x100,
                    word: result,
                    aux: f32::from_bits(result),
                });
            }
            crate::CmvsPs2aNumericExpressionNode::Operator { opcode: 0x170, .. } => {
                // Postfix assignment: the second-most-recent entry names the
                // target, the most recent entry resolves to the stored value.
                let Some(value_entry) = entries.pop() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
                        "CMVS PS2A numeric assignment is missing its value operand",
                    ));
                };
                let Some(target) = entries.pop() else {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
                        "CMVS PS2A numeric assignment is missing its target operand",
                    ));
                };
                let value = resolve_numeric_entry(state, value_entry)?;
                write_numeric_target(state, target, value)?;
                entries.push(NumericExpressionEntry {
                    tag: 0x12a,
                    word: value.to_bits(),
                    aux: value,
                });
            }
            _ => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(
                        event = "astra.emu.cmvs.vm.num_unsupported",
                        pc = state.program_counter,
                        frame = state.current_frame
                    );
                }
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
                    "CMVS PS2A numeric expression token is not executable in the recovered subset",
                ));
            }
        }
    }
    let [final_entry] = entries.as_slice() else {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS PS2A numeric expression did not reduce to one value",
        ));
    };
    let result = resolve_numeric_entry(state, *final_entry)?;
    state.current_value = Some(result.to_bits());
    state.condition_flag = result != 0.0;
    Ok(())
}

/// The recovered subset of the float resolver `sub_46D630`: kind 256 returns
/// the word reinterpreted as f32, kind 298 (`0x12a`) returns the retained
/// float slot, and kind 299 (`0x12b`) reads the process float table, which is
/// zero-initialized in the original binary.
/// Absolute stack address of a stack-frame float (`stack base + active
/// frame base + payload`), mirroring `sub_46E440` cases 0x12c/0x132/0x133.
fn stack_frame_float_addr(state: &CmvsPs2aVmState, payload: u32) -> Result<usize, CoreError> {
    let base = state.call_frame_bases.last().copied().unwrap_or_default() as usize;
    let offset = usize::try_from(payload).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float offset overflowed",
        )
    })?;
    let addr = base.checked_add(offset).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float address overflowed",
        )
    })?;
    if addr + 4 > state.stack_bytes.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float address is outside the value stack",
        ));
    }
    Ok(addr)
}

fn read_stack_frame_word(state: &CmvsPs2aVmState, payload: u32) -> Result<u32, CoreError> {
    let addr = stack_frame_float_addr(state, payload)?;
    Ok(u32::from_le_bytes(
        state.stack_bytes[addr..addr + 4].try_into().unwrap(),
    ))
}

fn write_stack_frame_word(
    state: &mut CmvsPs2aVmState,
    payload: u32,
    word: u32,
) -> Result<(), CoreError> {
    let addr = stack_frame_float_addr(state, payload)?;
    state.stack_bytes[addr..addr + 4].copy_from_slice(&word.to_le_bytes());
    Ok(())
}

/// Negative-direction stack-frame float address (`sub_46E440` case 0x129).
fn read_stack_frame_word_neg(state: &CmvsPs2aVmState, payload: u32) -> Result<u32, CoreError> {
    let base = state.call_frame_bases.last().copied().unwrap_or_default() as usize;
    let offset = usize::try_from(payload).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float offset overflowed",
        )
    })?;
    let addr = base.checked_sub(offset).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float address underflowed",
        )
    })?;
    if addr + 4 > state.stack_bytes.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float address is outside the value stack",
        ));
    }
    Ok(u32::from_le_bytes(
        state.stack_bytes[addr..addr + 4].try_into().unwrap(),
    ))
}

fn write_stack_frame_word_neg(
    state: &mut CmvsPs2aVmState,
    payload: u32,
    word: u32,
) -> Result<(), CoreError> {
    let base = state.call_frame_bases.last().copied().unwrap_or_default() as usize;
    let offset = usize::try_from(payload).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float offset overflowed",
        )
    })?;
    let addr = base.checked_sub(offset).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float address underflowed",
        )
    })?;
    if addr + 4 > state.stack_bytes.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS stack-frame float address is outside the value stack",
        ));
    }
    state.stack_bytes[addr..addr + 4].copy_from_slice(&word.to_le_bytes());
    Ok(())
}

pub(super) fn resolve_numeric_entry(
    state: &CmvsPs2aVmState,
    entry: NumericExpressionEntry,
) -> Result<f32, CoreError> {
    match entry.tag {
        0x100 => Ok(f32::from_bits(entry.word)),
        0x12a => Ok(entry.aux),
        0x12b => {
            if entry.word >= MAX_PROCESS_FLOAT_WORDS {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_PROCESS_FLOAT",
                    "CMVS float variable index exceeds the recovered table budget",
                ));
            }
            Ok(f32::from_bits(
                state
                    .process_float_words
                    .get(&entry.word)
                    .copied()
                    .unwrap_or(0),
            ))
        }
        0x12c | 0x132 | 0x133 => Ok(f32::from_bits(read_stack_frame_word(state, entry.word)?)),
        0x129 => Ok(f32::from_bits(read_stack_frame_word_neg(
            state, entry.word,
        )?)),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS PS2A numeric expression entry tag is not resolvable in the recovered subset",
        )),
    }
}

/// The recovered subset of the float write dispatcher `sub_46E440`: only the
/// `0x12b` process float-table target is backed by serializable state today.
pub(super) fn write_numeric_target(
    state: &mut CmvsPs2aVmState,
    target: NumericExpressionEntry,
    value: f32,
) -> Result<(), CoreError> {
    match target.tag {
        0x12b => {
            if target.word >= MAX_PROCESS_FLOAT_WORDS {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_PROCESS_FLOAT",
                    "CMVS float variable index exceeds the recovered table budget",
                ));
            }
            state
                .process_float_words
                .insert(target.word, value.to_bits());
            Ok(())
        }
        0x12c | 0x132 | 0x133 => write_stack_frame_word(state, target.word, value.to_bits()),
        0x129 => write_stack_frame_word_neg(state, target.word, value.to_bits()),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION",
            "CMVS PS2A numeric assignment target is not writable in the recovered subset",
        )),
    }
}
