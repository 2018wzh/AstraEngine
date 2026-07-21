use std::collections::{BTreeMap, BTreeSet};

use encoding_rs::SHIFT_JIS;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::MINORI_SCRIPT_IR_SCHEMA;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceSpan {
    pub offset: u64,
    pub length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScCommand {
    pub ordinal: u32,
    pub opcode: String,
    pub known: bool,
    pub span: SourceSpan,
    pub raw_operands: Vec<u8>,
    pub operands: Vec<ScOperand>,
    pub control_flow: ScControlFlow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScOperand {
    Integer { value: i64 },
    Boolean { value: bool },
    Operator { value: String },
    Symbol { value: String },
    Text { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScControlFlow {
    Next,
    Label { id: String },
    Jump { target: String },
    ConditionalJump { target: String },
    Chain { target: String },
    Return,
    Terminate,
    Choice { targets: Vec<String> },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScLineKind {
    Command { command: ScCommand },
    Comment,
    Blank,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScLine {
    pub span: SourceSpan,
    pub raw: Vec<u8>,
    pub kind: ScLineKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScScript {
    pub schema: String,
    pub encoding: String,
    pub lines: Vec<ScLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScOpcodeSpec {
    pub name: String,
    pub control_flow: ScControlFlowKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScControlFlowKind {
    Next,
    LabelSymbol { operand: usize },
    JumpSymbol { operand: usize },
    ConditionalJumpSymbol { operand: usize },
    ChainSymbol { operand: usize },
    Return,
    Terminate,
    ChoiceSymbols { operands: Vec<usize> },
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct ScOpcodeCatalog {
    specs: BTreeMap<String, ScOpcodeSpec>,
}

impl ScOpcodeCatalog {
    pub fn insert(
        &mut self,
        opcode: impl Into<String>,
        spec: ScOpcodeSpec,
    ) -> Result<(), ScParseError> {
        let opcode = opcode.into().to_ascii_lowercase();
        if !safe_symbol(&opcode) || self.specs.insert(opcode.clone(), spec).is_some() {
            return Err(ScParseError::DuplicateOpcode(opcode));
        }
        Ok(())
    }

    /// Catalog entries whose syntax was verified across the authorized 89-file sample.
    /// `select` remains explicitly unknown because its branch semantics are not proven.
    pub fn observed_minori() -> Self {
        let mut catalog = Self::default();
        for opcode in [
            "message",
            "transition",
            "stage",
            "panel",
            "playbgm",
            "char",
            "playse",
            "wait",
            "playse2",
            "pragma",
            "setglobal",
            "set",
            "playse3",
            "effect",
            "movie",
            "playvoice",
            "shakescreen",
            "endscroll",
            "vscroll",
            "scrollxf",
            "effect2",
            "hscroll",
            "scroll",
        ] {
            catalog
                .insert(
                    opcode,
                    ScOpcodeSpec {
                        name: opcode.into(),
                        control_flow: ScControlFlowKind::Next,
                    },
                )
                .expect("static Minori opcode catalog is unique");
        }
        for (opcode, control_flow) in [
            ("label", ScControlFlowKind::LabelSymbol { operand: 0 }),
            ("goto", ScControlFlowKind::JumpSymbol { operand: 0 }),
            (
                "if",
                ScControlFlowKind::ConditionalJumpSymbol { operand: 3 },
            ),
            ("chain", ScControlFlowKind::ChainSymbol { operand: 0 }),
            ("end", ScControlFlowKind::Terminate),
            ("select", ScControlFlowKind::Unknown),
        ] {
            catalog
                .insert(
                    opcode,
                    ScOpcodeSpec {
                        name: opcode.into(),
                        control_flow,
                    },
                )
                .expect("static Minori opcode catalog is unique");
        }
        catalog
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScParseError {
    #[error("ASTRA_EMU_MINORI_SC_ENCODING at offset {0}")]
    Encoding(usize),
    #[error("ASTRA_EMU_MINORI_SC_DUPLICATE_OPCODE: {0}")]
    DuplicateOpcode(String),
    #[error("ASTRA_EMU_MINORI_SC_DUPLICATE_LABEL: {0}")]
    DuplicateLabel(String),
    #[error("ASTRA_EMU_MINORI_SC_INVALID_TARGET: {0}")]
    InvalidTarget(String),
    #[error("ASTRA_EMU_MINORI_SC_OPERAND_SCHEMA at offset {0}")]
    OperandSchema(usize),
    #[error("ASTRA_EMU_MINORI_SC_SOURCE_INVARIANT at offset {0}")]
    SourceInvariant(usize),
}

/// Parses the observed CP932, CRLF-oriented `.command operands` source form.
/// Every source line and newline is retained verbatim for lossless round-trip.
pub fn parse_sc(bytes: &[u8], catalog: &ScOpcodeCatalog) -> Result<ScScript, ScParseError> {
    let mut cursor = 0usize;
    let mut ordinal = 0u32;
    let mut lines = Vec::new();
    while cursor < bytes.len() {
        let end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |relative| cursor + relative + 1);
        let raw = &bytes[cursor..end];
        let logical_end = raw
            .strip_suffix(b"\r\n")
            .or_else(|| raw.strip_suffix(b"\n"))
            .map_or(raw.len(), <[u8]>::len);
        let logical = &raw[..logical_end];
        if SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(logical)
            .is_none()
        {
            return Err(ScParseError::Encoding(cursor));
        }
        let span = SourceSpan {
            offset: cursor as u64,
            length: u32::try_from(raw.len()).map_err(|_| ScParseError::SourceInvariant(cursor))?,
        };
        let kind = parse_line(logical, span, ordinal, catalog)?;
        if matches!(kind, ScLineKind::Command { .. }) {
            ordinal = ordinal
                .checked_add(1)
                .ok_or(ScParseError::SourceInvariant(cursor))?;
        }
        lines.push(ScLine {
            span,
            raw: raw.to_vec(),
            kind,
        });
        cursor = end;
    }
    validate_cfg(&lines)?;
    Ok(ScScript {
        schema: MINORI_SCRIPT_IR_SCHEMA.into(),
        encoding: "cp932".into(),
        lines,
    })
}

pub fn encode_sc(script: &ScScript) -> Result<Vec<u8>, ScParseError> {
    let mut bytes = Vec::new();
    for line in &script.lines {
        if line.span.offset != bytes.len() as u64 || line.span.length as usize != line.raw.len() {
            return Err(ScParseError::SourceInvariant(bytes.len()));
        }
        bytes.extend_from_slice(&line.raw);
    }
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScCensus {
    pub file_count: u64,
    pub line_count: u64,
    pub command_count: u64,
    pub opcode_counts: BTreeMap<String, u64>,
    pub opcode_arity_counts: BTreeMap<String, BTreeMap<u32, u64>>,
    pub opcode_whitespace_arity_counts: BTreeMap<String, BTreeMap<u32, u64>>,
    pub opcode_operand_kind_counts: BTreeMap<String, BTreeMap<u32, BTreeMap<String, u64>>>,
    pub operand_size_counts: BTreeMap<u32, u64>,
    pub unknown_opcode_count: u64,
}

impl ScCensus {
    pub fn from_scripts<'a>(scripts: impl IntoIterator<Item = &'a ScScript>) -> Self {
        let mut result = Self {
            file_count: 0,
            line_count: 0,
            command_count: 0,
            opcode_counts: BTreeMap::new(),
            opcode_arity_counts: BTreeMap::new(),
            opcode_whitespace_arity_counts: BTreeMap::new(),
            opcode_operand_kind_counts: BTreeMap::new(),
            operand_size_counts: BTreeMap::new(),
            unknown_opcode_count: 0,
        };
        for script in scripts {
            result.file_count += 1;
            result.line_count += script.lines.len() as u64;
            for line in &script.lines {
                let ScLineKind::Command { command } = &line.kind else {
                    continue;
                };
                result.command_count += 1;
                *result
                    .opcode_counts
                    .entry(command.opcode.clone())
                    .or_default() += 1;
                *result
                    .opcode_arity_counts
                    .entry(command.opcode.clone())
                    .or_default()
                    .entry(command.operands.len() as u32)
                    .or_default() += 1;
                let whitespace_arity = command
                    .raw_operands
                    .split(|byte| matches!(byte, b' ' | b'\t'))
                    .filter(|part| !part.is_empty())
                    .count() as u32;
                *result
                    .opcode_whitespace_arity_counts
                    .entry(command.opcode.clone())
                    .or_default()
                    .entry(whitespace_arity)
                    .or_default() += 1;
                let positions = result
                    .opcode_operand_kind_counts
                    .entry(command.opcode.clone())
                    .or_default();
                for (position, operand) in command.operands.iter().enumerate() {
                    *positions
                        .entry(position as u32)
                        .or_default()
                        .entry(operand.kind_name().into())
                        .or_default() += 1;
                }
                *result
                    .operand_size_counts
                    .entry(command.raw_operands.len() as u32)
                    .or_default() += 1;
                if !command.known {
                    result.unknown_opcode_count += 1;
                }
            }
        }
        result
    }
}

impl ScOperand {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Integer { .. } => "integer",
            Self::Boolean { .. } => "boolean",
            Self::Operator { .. } => "operator",
            Self::Symbol { .. } => "symbol",
            Self::Text { .. } => "text",
        }
    }
}

pub fn disassemble_sc(script: &ScScript) -> Result<String, ScParseError> {
    let mut output = String::new();
    for line in &script.lines {
        let Some(decoded) = SHIFT_JIS.decode_without_bom_handling_and_without_replacement(
            line.raw
                .strip_suffix(b"\r\n")
                .or_else(|| line.raw.strip_suffix(b"\n"))
                .unwrap_or(&line.raw),
        ) else {
            return Err(ScParseError::Encoding(line.span.offset as usize));
        };
        output.push_str(&format!("{:08x}: {decoded}\n", line.span.offset));
    }
    Ok(output)
}

fn parse_line(
    logical: &[u8],
    span: SourceSpan,
    ordinal: u32,
    catalog: &ScOpcodeCatalog,
) -> Result<ScLineKind, ScParseError> {
    let start = logical
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'));
    let Some(start) = start else {
        return Ok(ScLineKind::Blank);
    };
    let trimmed = &logical[start..];
    if trimmed.starts_with(b";") || trimmed.starts_with(b"#") || trimmed.starts_with(b"//") {
        return Ok(ScLineKind::Comment);
    }
    if !trimmed.starts_with(b".") {
        return Ok(ScLineKind::Unknown);
    }
    let token_end = trimmed[1..]
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        .map_or(trimmed.len(), |relative| relative + 1);
    if token_end == 1 {
        return Ok(ScLineKind::Unknown);
    }
    let opcode_bytes = &trimmed[1..token_end];
    if !opcode_bytes[0].is_ascii_alphabetic() && opcode_bytes[0] != b'_' {
        return Ok(ScLineKind::Unknown);
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
    let operands = tokenize_operands(&raw_operands, span.offset as usize)?
        .into_iter()
        .map(classify_operand)
        .collect();
    let control_flow = spec.map_or(Ok(ScControlFlow::Unknown), |spec| {
        decode_control_flow(&spec.control_flow, &raw_operands, span.offset as usize)
    })?;
    Ok(ScLineKind::Command {
        command: ScCommand {
            ordinal,
            opcode,
            known: spec.is_some(),
            span,
            raw_operands,
            operands,
            control_flow,
        },
    })
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
) -> Result<ScControlFlow, ScParseError> {
    match kind {
        ScControlFlowKind::Next => return Ok(ScControlFlow::Next),
        ScControlFlowKind::Return => return Ok(ScControlFlow::Return),
        ScControlFlowKind::Terminate => return Ok(ScControlFlow::Terminate),
        ScControlFlowKind::Unknown => return Ok(ScControlFlow::Unknown),
        _ => {}
    }
    let tokens = tokenize_operands(operands, offset)?;
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
            target: symbol(*operand)?,
        },
        ScControlFlowKind::Return => unreachable!("handled before operand tokenization"),
        ScControlFlowKind::Terminate => unreachable!("handled before operand tokenization"),
        ScControlFlowKind::ChoiceSymbols { operands } => ScControlFlow::Choice {
            targets: operands
                .iter()
                .map(|operand| symbol(*operand))
                .collect::<Result<_, _>>()?,
        },
        ScControlFlowKind::Unknown => unreachable!("handled before operand tokenization"),
    })
}

pub(crate) fn tokenize_operands(bytes: &[u8], offset: usize) -> Result<Vec<String>, ScParseError> {
    let mut tokens = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        let start = cursor;
        while cursor < bytes.len() && !matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        let Some(decoded) =
            SHIFT_JIS.decode_without_bom_handling_and_without_replacement(&bytes[start..cursor])
        else {
            return Err(ScParseError::Encoding(offset + start));
        };
        tokens.push(decoded.into_owned());
    }
    Ok(tokens)
}

fn validate_cfg(lines: &[ScLine]) -> Result<(), ScParseError> {
    let mut labels = BTreeSet::new();
    for command in commands(lines) {
        if let ScControlFlow::Label { id } = &command.control_flow {
            if !labels.insert(id.clone()) {
                return Err(ScParseError::DuplicateLabel(id.clone()));
            }
        }
    }
    for command in commands(lines) {
        let targets: Vec<&String> = match &command.control_flow {
            ScControlFlow::Jump { target } | ScControlFlow::ConditionalJump { target } => {
                vec![target]
            }
            ScControlFlow::Choice { targets } => targets.iter().collect(),
            _ => Vec::new(),
        };
        for target in targets {
            if !labels.contains(target) {
                return Err(ScParseError::InvalidTarget(target.clone()));
            }
        }
    }
    Ok(())
}

fn commands(lines: &[ScLine]) -> impl Iterator<Item = &ScCommand> {
    lines.iter().filter_map(|line| match &line.kind {
        ScLineKind::Command { command } => Some(command),
        _ => None,
    })
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'/' | b'\\')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_cp932_source_round_trips_losslessly() {
        let source = b"; fixture\r\n.pragma entry\r\n.unknown raw operands\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        assert_eq!(script.lines.len(), 4);
        assert_eq!(encode_sc(&script).unwrap(), source);
        let census = ScCensus::from_scripts([&script]);
        assert_eq!(census.command_count, 3);
        assert_eq!(census.unknown_opcode_count, 1);
    }

    #[test]
    fn textual_cfg_is_validated() {
        let source =
            b".label start\r\n.if flag == 1 done\r\n.goto start\r\n.label done\r\n.end\r\n";
        parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let invalid = b".goto missing\r\n.end\r\n";
        assert_eq!(
            parse_sc(invalid, &ScOpcodeCatalog::observed_minori()).unwrap_err(),
            ScParseError::InvalidTarget("missing".into())
        );
    }

    #[test]
    fn duplicate_catalog_opcode_is_blocking() {
        let mut catalog = ScOpcodeCatalog::default();
        let spec = ScOpcodeSpec {
            name: "wait".into(),
            control_flow: ScControlFlowKind::Next,
        };
        catalog.insert("wait", spec.clone()).unwrap();
        assert_eq!(
            catalog.insert("WAIT", spec).unwrap_err(),
            ScParseError::DuplicateOpcode("wait".into())
        );
    }

    #[test]
    fn operands_are_typed_without_discarding_raw_source() {
        let source =
            b".movie 9989 op.avi 1280 720 t\r\n.if flag == 1 done\r\n.label done\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let ScLineKind::Command { command } = &script.lines[0].kind else {
            panic!("first line must be a command");
        };
        assert_eq!(
            command.operands,
            vec![
                ScOperand::Integer { value: 9989 },
                ScOperand::Symbol {
                    value: "op.avi".into()
                },
                ScOperand::Integer { value: 1280 },
                ScOperand::Integer { value: 720 },
                ScOperand::Boolean { value: true },
            ]
        );
        assert_eq!(command.raw_operands, b"9989 op.avi 1280 720 t");
        assert_eq!(encode_sc(&script).unwrap(), source);
    }

    #[test]
    fn tokenizer_matches_the_observed_engine_space_tab_contract() {
        let tokens = tokenize_operands(b"one, two\t\"three four\" 'five'", 0).unwrap();
        assert_eq!(tokens, vec!["one,", "two", "\"three", "four\"", "'five'"]);
    }
}
