use super::*;

/// Decodes one statically-proven PS2A control instruction at `offset`.
///
/// This is intentionally an exact, single-instruction decoder rather than a
/// speculative linear disassembler: expression and command instructions have
/// handler-owned stack contracts that are not yet proven for this engine
/// fingerprint.  Callers must stop on `ASTRA_EMU_CMVS_PS2A_OPCODE` instead of
/// skipping an unrecognised instruction.
pub fn decode_ps2a_control_instruction(
    script: &CmvsScript,
    offset: u32,
) -> Result<CmvsPs2aControlInstruction, CoreError> {
    let offset = usize::try_from(offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A control offset exceeds platform bounds",
        )
    })?;
    let opcode = le_u16(&script.bytecode, offset, "ASTRA_EMU_CMVS_PS2A_CONTROL")?;
    let span = |byte_length| {
        let end = offset
            .checked_add(usize::from(byte_length))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_CONTROL",
                    "PS2A control span overflowed",
                )
            })?;
        if end > script.bytecode.len() {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_CONTROL",
                "PS2A control instruction is truncated",
            ));
        }
        Ok(CmvsPs2aSourceSpan {
            offset: u32::try_from(offset).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_OFFSET",
                    "PS2A control offset is too large",
                )
            })?,
            byte_length,
        })
    };
    let validate_target = |target: u32| validate_program_counter(script, target);
    match opcode {
        0x400 => Ok(CmvsPs2aControlInstruction::AbsoluteJump {
            span: span(6)?,
            target: validate_target(le_u32_at(&script.bytecode, offset + 2)?)?,
        }),
        0x401 => Ok(CmvsPs2aControlInstruction::JumpWhenFlagClear {
            span: span(6)?,
            target: validate_target(le_u32_at(&script.bytecode, offset + 2)?)?,
        }),
        0x402 => Ok(CmvsPs2aControlInstruction::JumpWhenFlagSet {
            span: span(6)?,
            target: validate_target(le_u32_at(&script.bytecode, offset + 2)?)?,
        }),
        0x403 => Ok(CmvsPs2aControlInstruction::JumpWhenValueEquals {
            span: span(10)?,
            expected_value: le_u32_at(&script.bytecode, offset + 2)?,
            target: validate_target(le_u32_at(&script.bytecode, offset + 6)?)?,
        }),
        0x407 => Ok(CmvsPs2aControlInstruction::JumpWithOpaqueField {
            span: span(10)?,
            opaque_field: le_u32_at(&script.bytecode, offset + 2)?,
            target: validate_target(le_u32_at(&script.bytecode, offset + 6)?)?,
        }),
        0x405 | 0x410 => {
            let span = span(4)?;
            let name_index = le_u16(&script.bytecode, offset + 2, "ASTRA_EMU_CMVS_PS2A_CONTROL")?;
            let target = *script
                .name_index
                .get(usize::from(name_index))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_NAME_INDEX",
                        "PS2A control instruction references an unknown name index",
                    )
                })?;
            let target = validate_target(target)?;
            if opcode == 0x405 {
                Ok(CmvsPs2aControlInstruction::NameIndexJump {
                    span,
                    name_index,
                    target,
                })
            } else {
                Ok(CmvsPs2aControlInstruction::NameIndexCall {
                    span,
                    name_index,
                    target,
                })
            }
        }
        0x411 => Ok(CmvsPs2aControlInstruction::StackReturn {
            span: span(4)?,
            stack_adjust: le_u16(&script.bytecode, offset + 2, "ASTRA_EMU_CMVS_PS2A_CONTROL")?,
        }),
        0x412 => Ok(CmvsPs2aControlInstruction::StackDrop {
            span: span(4)?,
            stack_bytes: le_u16(&script.bytecode, offset + 2, "ASTRA_EMU_CMVS_PS2A_CONTROL")?,
        }),
        0x413 => Ok(CmvsPs2aControlInstruction::FrameReturn { span: span(2)? }),
        0x414 => Ok(CmvsPs2aControlInstruction::ScriptReturn { span: span(2)? }),
        0x416 => Ok(CmvsPs2aControlInstruction::StateTargetCall { span: span(2)? }),
        0x430 => Ok(CmvsPs2aControlInstruction::PushCurrentCondition { span: span(2)? }),
        0x440 | 0x442 => Ok(CmvsPs2aControlInstruction::PushImmediate {
            span: span(8)?,
            stack_bytes: le_u16(&script.bytecode, offset + 2, "ASTRA_EMU_CMVS_PS2A_CONTROL")?,
            value: le_u32_at(&script.bytecode, offset + 4)?,
        }),
        _ => {
            let start = offset.saturating_sub(8);
            let context = script
                .bytecode
                .get(start..(offset + 8).min(script.bytecode.len()))
                .map(|bytes| {
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            Err(CoreError::invalid(
                "ASTRA_EMU_CMVS_PS2A_OPCODE",
                format!("PS2A opcode {opcode:#06x} is not a proven control instruction (offset {offset}, context [start {start}] {context})"),
            ))
        }
    }
}

/// Decodes the proven linear `0x201` PS2A value-expression form.
///
/// The CMVS 3.90 evaluator stops at `0x20f`; ordinary tokens consume six
/// bytes and postfix operators consume two. The `0x121` form delegates to the
/// separately bounded `0x200` stack-expression grammar. This preserves
/// program-counter safety without pretending to evaluate either form.
pub fn decode_ps2a_value_expression(
    script: &CmvsScript,
    offset: u32,
) -> Result<CmvsPs2aValueExpression, CoreError> {
    const VALUE_EXPRESSION: u16 = 0x201;
    const EXPRESSION_END: u16 = 0x20f;
    const NESTED_EXPRESSION: u16 = 0x121;
    const MAX_TOKENS: usize = 16 * 1024;

    let offset = usize::try_from(offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A expression offset exceeds platform bounds",
        )
    })?;
    if le_u16(&script.bytecode, offset, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")? != VALUE_EXPRESSION {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
            "PS2A value-expression opcode is invalid",
        ));
    }
    let mut cursor = offset.checked_add(2).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
            "PS2A expression offset overflowed",
        )
    })?;
    let mut tokens = Vec::new();
    loop {
        if tokens.len() == MAX_TOKENS {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                "PS2A value expression exceeds the token budget",
            ));
        }
        let opcode = le_u16(&script.bytecode, cursor, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?;
        if opcode == EXPRESSION_END {
            let end = cursor.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                    "PS2A expression span overflowed",
                )
            })?;
            return Ok(CmvsPs2aValueExpression {
                span: source_span(offset, end, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                tokens,
            });
        }
        if opcode == NESTED_EXPRESSION {
            let nested_start = cursor.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                    "PS2A nested expression offset overflowed",
                )
            })?;
            let mut nested_budget = MAX_TOKENS;
            let (expression, end) =
                decode_stack_expression_at(script, nested_start, 0, &mut nested_budget)?;
            tokens.push(CmvsPs2aValueExpressionToken::NestedStackExpression {
                span: source_span(cursor, end, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                expression: Box::new(expression),
            });
            cursor = end;
            continue;
        }
        if (0x160..=0x172).contains(&opcode) {
            tokens.push(CmvsPs2aValueExpressionToken::Operator {
                span: source_span(
                    cursor,
                    cursor.checked_add(2).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A operator span overflowed",
                        )
                    })?,
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                )?,
                opcode,
            });
            cursor += 2;
        } else {
            let end = cursor.checked_add(6).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                    "PS2A value span overflowed",
                )
            })?;
            tokens.push(CmvsPs2aValueExpressionToken::Value {
                span: source_span(cursor, end, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                opcode,
                payload: le_u32_at(&script.bytecode, cursor + 2)?,
            });
            cursor = end;
        }
    }
}

/// Converts the single-token `0x120` value expression proven by the CMVS
/// 3.90 evaluator into a tag-zero private string reference. Other expression
/// shapes may evaluate to a different tag domain or depend on VM state, so
/// they remain blocking rather than being treated as raw pool offsets.
pub fn private_string_reference_from_value_expression(
    expression: &CmvsPs2aValueExpression,
) -> Result<CmvsPs2aPrivateStringReference, CoreError> {
    match expression.tokens.as_slice() {
        [CmvsPs2aValueExpressionToken::Value {
            opcode: 0x120,
            payload,
            ..
        }] => Ok(CmvsPs2aPrivateStringReference {
            relative_offset: *payload,
        }),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_STRING_EXPRESSION",
            "PS2A value expression is not a proven tag-zero string reference",
        )),
    }
}

/// Decodes the CMVS 3.90 `0x200` stack-expression grammar without evaluating
/// it.  The grammar and each recursive entry point are taken from the static
/// evaluator.  Any unknown prefix form, unterminated expression, excessive
/// recursion or token budget is a blocking diagnostic rather than a skipped
/// byte range.
pub fn decode_ps2a_stack_expression(
    script: &CmvsScript,
    offset: u32,
) -> Result<CmvsPs2aStackExpression, CoreError> {
    let offset = usize::try_from(offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A stack-expression offset exceeds platform bounds",
        )
    })?;
    let mut budget = 16 * 1024usize;
    let (expression, _) = decode_stack_expression_at(script, offset, 0, &mut budget)?;
    Ok(expression)
}

/// Decodes the CMVS 3.90 `0x202` numeric-expression grammar without
/// evaluating it.  This evaluator has its own prefix forms, but every child
/// expression is the independently bounded `0x200` stack-expression form.
/// Unknown framing, truncation, recursion and budget exhaustion are blocking.
pub fn decode_ps2a_numeric_expression(
    script: &CmvsScript,
    offset: u32,
) -> Result<CmvsPs2aNumericExpression, CoreError> {
    let offset = usize::try_from(offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A numeric-expression offset exceeds platform bounds",
        )
    })?;
    let mut budget = 16 * 1024usize;
    let (expression, _) = decode_numeric_expression_at(script, offset, 0, &mut budget)?;
    Ok(expression)
}

/// Frames one instruction at an exact program counter.  Command handlers are
/// not executed here: the returned id is only the interpreter's bounded
/// `0x2000..=0x27ff` dispatch index.  Any other opcode is rejected so a census
/// cannot silently desynchronize after an unrecovered expression form.
pub fn frame_ps2a_instruction(
    script: &CmvsScript,
    offset: u32,
) -> Result<CmvsPs2aInstructionFrame, CoreError> {
    let offset_usize = usize::try_from(offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A instruction offset exceeds platform bounds",
        )
    })?;
    let opcode = le_u16(&script.bytecode, offset_usize, "ASTRA_EMU_CMVS_PS2A_FRAME")?;
    match opcode {
        0x200 => Ok(CmvsPs2aInstructionFrame::StackExpression(
            decode_ps2a_stack_expression(script, offset)?,
        )),
        0x201 => Ok(CmvsPs2aInstructionFrame::ValueExpression(
            decode_ps2a_value_expression(script, offset)?,
        )),
        0x202 => Ok(CmvsPs2aInstructionFrame::NumericExpression(
            decode_ps2a_numeric_expression(script, offset)?,
        )),
        0x2000..=0x27ff => Ok(CmvsPs2aInstructionFrame::Command {
            span: source_span(
                offset_usize,
                offset_usize.checked_add(2).ok_or_else(|| {
                    invalid("ASTRA_EMU_CMVS_PS2A_FRAME", "PS2A command span overflowed")
                })?,
                "ASTRA_EMU_CMVS_PS2A_FRAME",
            )?,
            command_id: opcode & 0x7ff,
        }),
        _ => decode_ps2a_control_instruction(script, offset).map(CmvsPs2aInstructionFrame::Control),
    }
}

/// Counts the documented `0x01200201` PS2A string-reference pattern without
/// decoding or returning script strings.  A candidate is valid only when its
/// relative operand resolves to the beginning of a bounded, validated string
/// pool entry; references into the middle of a string are retained as invalid
/// evidence rather than silently accepted.
pub fn census_ps2a_string_references(
    script: &CmvsScript,
) -> Result<CmvsPs2aStringReferenceCensus, CoreError> {
    const PUSH_STRING_PATTERN: [u8; 4] = 0x0120_0201u32.to_le_bytes();
    let string_pool_start = usize::try_from(script.string_pool_start_u64()).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A string pool offset is too large",
        )
    })?;
    let string_starts = script
        .strings
        .iter()
        .map(|entry| usize::try_from(entry.offset))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_OFFSET",
                "PS2A string offset is too large",
            )
        })?;
    let mut census = CmvsPs2aStringReferenceCensus {
        candidate_count: 0,
        valid_string_start_count: 0,
        invalid_offset_count: 0,
    };
    for position in 0..script.bytecode.len().saturating_sub(7) {
        if script.bytecode[position..position + 4] != PUSH_STRING_PATTERN {
            continue;
        }
        census.candidate_count = census.candidate_count.checked_add(1).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_COUNT",
                "PS2A reference count overflowed",
            )
        })?;
        let relative = u32::from_le_bytes(
            script.bytecode[position + 4..position + 8]
                .try_into()
                .map_err(|_| invalid("ASTRA_EMU_CMVS_PS2A_LAYOUT", "PS2A operand is truncated"))?,
        );
        let target = string_pool_start.checked_add(usize::try_from(relative).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_OFFSET",
                "PS2A string reference is too large",
            )
        })?);
        if target.is_some_and(|offset| string_starts.contains(&offset)) {
            census.valid_string_start_count = census
                .valid_string_start_count
                .checked_add(1)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_COUNT",
                        "PS2A reference count overflowed",
                    )
                })?;
        } else {
            census.invalid_offset_count =
                census.invalid_offset_count.checked_add(1).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_COUNT",
                        "PS2A reference count overflowed",
                    )
                })?;
        }
    }
    Ok(census)
}

pub(crate) fn decode_ps2a(source: &[u8], header: Ps2aHeader) -> Result<Vec<u8>, CoreError> {
    decode_payload(source, header)
}
