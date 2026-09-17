use super::*;

pub(super) fn parse_line(
    logical: &[u8],
    span: SourceSpan,
    ordinal: u32,
    catalog: &ScOpcodeCatalog,
    encoding: ScriptEncoding,
) -> Result<(ScLineKind, Option<char>), ScParseError> {
    let mut language_guard = None;
    let start = logical
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'));
    let Some(start) = start else {
        return Ok((ScLineKind::Blank, None));
    };
    let mut cursor = start;
    // ef*-style per-line language guard: `[j]` or `[e]` immediately before
    // the command token. Invalid guards are rejected at the byte boundary.
    if logical[cursor] == b'[' {
        let Some(close) = logical[cursor..]
            .iter()
            .position(|byte| *byte == b']')
            .map(|relative| cursor + relative)
        else {
            return Err(ScParseError::OperandSchema(span.offset as usize));
        };
        let guard = match &logical[cursor + 1..close] {
            b"j" | b"J" => Some('j'),
            b"e" | b"E" => Some('e'),
            _ => return Err(ScParseError::OperandSchema(span.offset as usize)),
        };
        language_guard = guard;
        cursor = close + 1;
        while matches!(logical.get(cursor), Some(b' ') | Some(b'\t')) {
            cursor += 1;
        }
    }
    let trimmed = &logical[cursor..];
    if trimmed.starts_with(b";") || trimmed.starts_with(b"#") || trimmed.starts_with(b"//") {
        return Ok((ScLineKind::Comment, language_guard));
    }
    if !trimmed.starts_with(b".") {
        return Ok((ScLineKind::Unknown, language_guard));
    }
    let token_end = trimmed[1..]
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        .map_or(trimmed.len(), |relative| relative + 1);
    if token_end == 1 {
        return Ok((ScLineKind::Unknown, language_guard));
    }
    let opcode_bytes = &trimmed[1..token_end];
    if !opcode_bytes[0].is_ascii_alphabetic() && opcode_bytes[0] != b'_' {
        return Ok((ScLineKind::Unknown, language_guard));
    }
    let opcode = std::str::from_utf8(opcode_bytes)
        .map_err(|_| ScParseError::OperandSchema(span.offset as usize))?
        .to_ascii_lowercase();
    let operand_start = trimmed[token_end..]
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'))
        .map_or(trimmed.len(), |relative| token_end + relative);
    let raw_operands = trimmed[operand_start..].to_vec();
    let spec = catalog.specs.get(&opcode);
    let operands = tokenize_operands_with_encoding(&raw_operands, span.offset as usize, encoding)?
        .into_iter()
        .map(classify_operand)
        .collect();
    let control_flow = spec.map_or(Ok(ScControlFlow::Unknown), |spec| {
        decode_control_flow(
            &spec.control_flow,
            &raw_operands,
            span.offset as usize,
            encoding,
        )
    })?;
    Ok((
        ScLineKind::Command {
            command: ScCommand {
                encoding,
                ordinal,
                opcode,
                raw_operands,
                span,
                operands,
                control_flow,
                known: spec.is_some(),
            },
        },
        language_guard,
    ))
}

fn classify_operand(value: String) -> ScOperand {
    if let Ok(value) = value.parse::<i64>() {
        ScOperand::Integer { value }
    } else if matches!(value.as_str(), "t" | "true") {
        ScOperand::Boolean { value: true }
    } else if matches!(value.as_str(), "f" | "false") {
        ScOperand::Boolean { value: false }
    } else if matches!(
        value.as_str(),
        "=" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "+" | "-" | "*" | "/" | "%" | "|" | "&"
    ) {
        ScOperand::Operator { value }
    } else if safe_symbol(&value) {
        ScOperand::Symbol { value }
    } else {
        ScOperand::Text { value }
    }
}

fn decode_control_flow(
    kind: &ScControlFlowKind,
    operands: &[u8],
    offset: usize,
    encoding: ScriptEncoding,
) -> Result<ScControlFlow, ScParseError> {
    match kind {
        ScControlFlowKind::Next => return Ok(ScControlFlow::Next),
        ScControlFlowKind::Return => return Ok(ScControlFlow::Return),
        ScControlFlowKind::Terminate => return Ok(ScControlFlow::Terminate),
        ScControlFlowKind::Unknown => return Ok(ScControlFlow::Unknown),
        _ => {}
    }
    let tokens = tokenize_operands_with_encoding(operands, offset, encoding)?;
    let symbol = |position: usize| {
        tokens
            .get(position)
            .filter(|value| safe_symbol(value))
            .cloned()
            .ok_or(ScParseError::OperandSchema(offset))
    };
    Ok(match kind {
        ScControlFlowKind::Next => unreachable!("handled before operand tokenization"),
        ScControlFlowKind::LabelSymbol { operand } => ScControlFlow::Label {
            id: symbol(*operand)?,
        },
        ScControlFlowKind::JumpSymbol { operand } => ScControlFlow::Jump {
            target: symbol(*operand)?,
        },
        ScControlFlowKind::ConditionalJumpSymbol { operand } => ScControlFlow::ConditionalJump {
            target: symbol(*operand)?,
        },
        ScControlFlowKind::ChainSymbol { operand } => ScControlFlow::Chain {
            target: tokens
                .get(*operand)
                .filter(|target| chain_target_parts(target).is_some())
                .cloned()
                .ok_or(ScParseError::OperandSchema(offset))?,
        },
        ScControlFlowKind::Return => unreachable!("handled before operand tokenization"),
        ScControlFlowKind::Terminate => unreachable!("handled before operand tokenization"),
        ScControlFlowKind::ChoiceSymbols { operands } => ScControlFlow::Choice {
            targets: operands
                .iter()
                .map(|operand| symbol(*operand))
                .collect::<Result<_, _>>()?,
        },
        ScControlFlowKind::ChoicePairs => {
            if tokens.is_empty() || tokens.len() > 4 {
                return Err(ScParseError::OperandSchema(offset));
            }
            let targets = tokens
                .iter()
                .map(|token| {
                    let (_, target) = token
                        .split_once(':')
                        .filter(|(display, target)| !display.is_empty() && safe_symbol(target))
                        .ok_or(ScParseError::OperandSchema(offset))?;
                    Ok(target.to_owned())
                })
                .collect::<Result<Vec<_>, ScParseError>>()?;
            ScControlFlow::Choice { targets }
        }
        ScControlFlowKind::Unknown => unreachable!("handled before operand tokenization"),
    })
}

#[cfg(test)]
pub(crate) fn tokenize_operands(bytes: &[u8], offset: usize) -> Result<Vec<String>, ScParseError> {
    tokenize_operands_with_encoding(bytes, offset, ScriptEncoding::ShiftJis)
}

pub(super) fn tokenize_operands_with_encoding(
    bytes: &[u8],
    offset: usize,
    encoding: ScriptEncoding,
) -> Result<Vec<String>, ScParseError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut tokens = Vec::new();
    let mut start = 0usize;
    for cursor in 0..=bytes.len() {
        if cursor != bytes.len() && !matches!(bytes[cursor], b' ' | b'\t') {
            continue;
        }
        let Some(decoded) = encoding.decode(&bytes[start..cursor]) else {
            return Err(ScParseError::Encoding(offset + start));
        };
        tokens.push(decoded.into_owned());
        start = cursor + 1;
    }
    Ok(tokens)
}
