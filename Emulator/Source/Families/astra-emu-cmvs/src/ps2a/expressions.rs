use super::*;

pub(super) fn decode_stack_expression_at(
    script: &CmvsScript,
    start: usize,
    depth: usize,
    budget: &mut usize,
) -> Result<(CmvsPs2aStackExpression, usize), CoreError> {
    const STACK_EXPRESSION: u16 = 0x200;
    const EXPRESSION_END: u16 = 0x20f;
    const MAX_DEPTH: usize = 64;
    if depth == MAX_DEPTH {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION_DEPTH",
            "PS2A stack-expression recursion exceeds the depth budget",
        ));
    }
    if le_u16(&script.bytecode, start, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")? != STACK_EXPRESSION {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
            "PS2A stack-expression opcode is invalid",
        ));
    }
    let mut cursor = start.checked_add(2).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
            "PS2A stack-expression offset overflowed",
        )
    })?;
    let mut nodes = Vec::new();
    loop {
        if *budget == 0 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                "PS2A stack expression exceeds the token budget",
            ));
        }
        *budget -= 1;
        let opcode = le_u16(&script.bytecode, cursor, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?;
        if opcode == EXPRESSION_END {
            let end = cursor.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                    "PS2A stack-expression span overflowed",
                )
            })?;
            return Ok((
                CmvsPs2aStackExpression {
                    span: source_span(start, end, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    nodes,
                },
                end,
            ));
        }
        match opcode {
            0x200 => {
                let (expression, next) =
                    decode_stack_expression_at(script, cursor, depth + 1, budget)?;
                nodes.push(CmvsPs2aStackExpressionNode::Nested {
                    expression: Box::new(expression),
                });
                cursor = next;
            }
            0x101 | 0x102 | 0x12b => {
                let (operand, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(2).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                nodes.push(CmvsPs2aStackExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: None,
                    index: None,
                    operands: vec![operand],
                });
                cursor = next;
            }
            0x105 | 0x109 | 0x110 | 0x112 | 0x12d | 0x12f => {
                let payload = le_u32_at(&script.bytecode, cursor + 2)?;
                let (operand, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(6).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                nodes.push(CmvsPs2aStackExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: Some(payload),
                    index: None,
                    operands: vec![operand],
                });
                cursor = next;
            }
            0x107 | 0x10b => {
                let payload = le_u32_at(&script.bytecode, cursor + 2)?;
                let index = le_u16(
                    &script.bytecode,
                    cursor + 6,
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                )?;
                let (operand, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(8).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A indexed prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                nodes.push(CmvsPs2aStackExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: Some(payload),
                    index: Some(index),
                    operands: vec![operand],
                });
                cursor = next;
            }
            0x111 | 0x113 => {
                let payload = le_u32_at(&script.bytecode, cursor + 2)?;
                let index = le_u16(
                    &script.bytecode,
                    cursor + 6,
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                )?;
                let (first, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(8).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A indexed prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                let (second, next) = decode_stack_expression_at(script, next, depth + 1, budget)?;
                nodes.push(CmvsPs2aStackExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: Some(payload),
                    index: Some(index),
                    operands: vec![first, second],
                });
                cursor = next;
            }
            0x160..=0x17e => {
                let next = cursor.checked_add(2).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                        "PS2A operator span overflowed",
                    )
                })?;
                nodes.push(CmvsPs2aStackExpressionNode::Operator {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                });
                cursor = next;
            }
            _ => {
                let next = cursor.checked_add(6).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                        "PS2A value span overflowed",
                    )
                })?;
                nodes.push(CmvsPs2aStackExpressionNode::Value {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: le_u32_at(&script.bytecode, cursor + 2)?,
                });
                cursor = next;
            }
        }
    }
}

pub(super) fn decode_numeric_expression_at(
    script: &CmvsScript,
    start: usize,
    depth: usize,
    budget: &mut usize,
) -> Result<(CmvsPs2aNumericExpression, usize), CoreError> {
    const NUMERIC_EXPRESSION: u16 = 0x202;
    const EXPRESSION_END: u16 = 0x20f;
    const MAX_DEPTH: usize = 64;
    if depth == MAX_DEPTH {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION_DEPTH",
            "PS2A numeric-expression recursion exceeds the depth budget",
        ));
    }
    if le_u16(&script.bytecode, start, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")? != NUMERIC_EXPRESSION {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
            "PS2A numeric-expression opcode is invalid",
        ));
    }
    let mut cursor = start.checked_add(2).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
            "PS2A numeric-expression offset overflowed",
        )
    })?;
    let mut nodes = Vec::new();
    loop {
        if *budget == 0 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                "PS2A numeric expression exceeds the token budget",
            ));
        }
        *budget -= 1;
        let opcode = le_u16(&script.bytecode, cursor, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?;
        if opcode == EXPRESSION_END {
            let end = cursor.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                    "PS2A numeric-expression span overflowed",
                )
            })?;
            return Ok((
                CmvsPs2aNumericExpression {
                    span: source_span(start, end, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    nodes,
                },
                end,
            ));
        }
        match opcode {
            0x200 => {
                let (expression, next) =
                    decode_stack_expression_at(script, cursor, depth + 1, budget)?;
                nodes.push(CmvsPs2aNumericExpressionNode::StackExpression {
                    expression: Box::new(expression),
                });
                cursor = next;
            }
            0x101 | 0x102 | 0x12b => {
                let (operand, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(2).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A numeric prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                nodes.push(CmvsPs2aNumericExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: None,
                    index: None,
                    operands: vec![operand],
                });
                cursor = next;
            }
            0x105 | 0x109 | 0x110 | 0x112 | 0x12d | 0x12f | 0x130 | 0x134 | 0x136 => {
                let payload = le_u32_at(&script.bytecode, cursor + 2)?;
                let (operand, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(6).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A numeric prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                nodes.push(CmvsPs2aNumericExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: Some(payload),
                    index: None,
                    operands: vec![operand],
                });
                cursor = next;
            }
            0x107 | 0x10b | 0x131 | 0x133 => {
                let payload = le_u32_at(&script.bytecode, cursor + 2)?;
                let index = le_u16(
                    &script.bytecode,
                    cursor + 6,
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                )?;
                let (operand, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(8).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A numeric indexed-prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                nodes.push(CmvsPs2aNumericExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: Some(payload),
                    index: Some(index),
                    operands: vec![operand],
                });
                cursor = next;
            }
            0x111 | 0x113 | 0x135 | 0x137 => {
                let payload = le_u32_at(&script.bytecode, cursor + 2)?;
                let index = le_u16(
                    &script.bytecode,
                    cursor + 6,
                    "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                )?;
                let (first, next) = decode_stack_expression_at(
                    script,
                    cursor.checked_add(8).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                            "PS2A numeric indexed-prefix offset overflowed",
                        )
                    })?,
                    depth + 1,
                    budget,
                )?;
                let (second, next) = decode_stack_expression_at(script, next, depth + 1, budget)?;
                nodes.push(CmvsPs2aNumericExpressionNode::Prefix {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: Some(payload),
                    index: Some(index),
                    operands: vec![first, second],
                });
                cursor = next;
            }
            0x160..=0x17e => {
                let next = cursor.checked_add(2).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                        "PS2A numeric operator span overflowed",
                    )
                })?;
                nodes.push(CmvsPs2aNumericExpressionNode::Operator {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                });
                cursor = next;
            }
            _ => {
                let next = cursor.checked_add(6).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PS2A_EXPRESSION",
                        "PS2A numeric value span overflowed",
                    )
                })?;
                nodes.push(CmvsPs2aNumericExpressionNode::Value {
                    span: source_span(cursor, next, "ASTRA_EMU_CMVS_PS2A_EXPRESSION")?,
                    opcode,
                    payload: le_u32_at(&script.bytecode, cursor + 2)?,
                });
                cursor = next;
            }
        }
    }
}
