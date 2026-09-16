mod parse;
pub use parse::*;
mod instructions;
pub use instructions::*;
mod payload;
pub(super) use payload::*;
mod expressions;
use expressions::*;

use astra_emu_sdk::CoreError;
use encoding_rs::SHIFT_JIS;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PS2A_MAGIC: &[u8; 4] = b"PS2A";
const HEADER_BYTES: usize = 0x30;
const LZSS_WINDOW_BYTES: usize = 0x800;
const MAX_SCRIPT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Ps2aHeader {
    pub header_length: u32,
    pub program_key: u32,
    pub name_index_count: u32,
    pub bytecode_size: u32,
    pub metadata_size: u32,
    pub name_index_size: u32,
    /// The program counter the loader installs when the script becomes the
    /// active frame (dword at header offset 0x20), proven by `sub_478080`.
    pub initial_pc: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsScriptString {
    pub offset: u32,
    pub byte_length: u32,
    pub text_hash: [u8; 32],
}

/// In-process decoded script.  It deliberately has no serialization derive:
/// `bytecode` is commercial payload and must not enter a snapshot, report or
/// package merely because a caller serializes an adjacent DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmvsScript {
    pub schema: String,
    pub header: Ps2aHeader,
    /// Absolute program counters selected by the PS2A name-index table.
    ///
    /// These are control-flow offsets, not filenames or script text.  Keeping
    /// them in-process lets the typed control decoder validate indirect jumps
    /// without exposing commercial script payload.
    pub name_index: Vec<u32>,
    pub bytecode_offset: u32,
    pub bytecode: Vec<u8>,
    /// The runtime-writable data segment between the program section and the
    /// string pool (`sub_4781B0` publishes its base at `this+15100` per
    /// frame, and `sub_46D270`/`sub_46E250` read and write dwords through
    /// it). The loader's buffer keeps the decoded bytes, so the initial
    /// contents come from the script file itself.
    pub data_segment: Vec<u8>,
    /// The validated Shift-JIS pool, retained solely for an active runtime
    /// session to materialize an ephemeral text lease. This is commercial
    /// payload and must never be serialized into a snapshot, report or
    /// package.
    string_pool: Vec<u8>,
    pub strings: Vec<CmvsScriptString>,
    pub decoded_size: u32,
}

impl CmvsScript {
    /// Builds the payload-free table mapping each validated pool-relative
    /// string offset to its `lstrlenA` byte length (the terminator is not
    /// counted). The recovered case-248 handler needs only this length, so
    /// the text itself stays out of the VM snapshot.
    pub fn private_string_length_table(&self) -> std::collections::BTreeMap<u32, u32> {
        let pool_start = self.string_pool_start_u64();
        self.strings
            .iter()
            .filter_map(|entry| {
                let relative = u64::from(entry.offset).checked_sub(pool_start)?;
                Some((u32::try_from(relative).ok()?, entry.byte_length))
            })
            .collect()
    }

    /// The decoded-buffer byte offset of the string pool: the header, the
    /// name-index table, the program section and the runtime data segment
    /// all precede it (`sub_4781B0` publishes the pool base through
    /// `this+15116`).
    fn string_pool_start_u64(&self) -> u64 {
        u64::from(self.bytecode_offset)
            + u64::try_from(self.bytecode.len()).unwrap_or(u64::MAX)
            + u64::try_from(self.data_segment.len()).unwrap_or(u64::MAX)
    }

    /// Resolves a tag-zero PS2A string-pool relative offset into an
    /// in-process text value. The offset must name the beginning of one
    /// validated pool entry; offsets into the middle of a string are rejected
    /// instead of exposing an arbitrary payload slice.
    pub fn resolve_private_string_relative(
        &self,
        relative_offset: u32,
    ) -> Result<String, CoreError> {
        let pool_start = usize::try_from(self.string_pool_start_u64()).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_OFFSET",
                "PS2A string-pool offset is too large",
            )
        })?;
        let relative = usize::try_from(relative_offset).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_STRING_OFFSET",
                "PS2A string reference exceeds platform bounds",
            )
        })?;
        let absolute = pool_start.checked_add(relative).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_STRING_OFFSET",
                "PS2A string reference overflowed",
            )
        })?;
        let entry = self
            .strings
            .iter()
            .find(|entry| usize::try_from(entry.offset).ok() == Some(absolute))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_STRING_OFFSET",
                    "PS2A string reference does not name a validated entry",
                )
            })?;
        let start = relative;
        let length = usize::try_from(entry.byte_length).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_STRING_SIZE",
                "PS2A string length exceeds platform bounds",
            )
        })?;
        let end = start.checked_add(length).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_STRING_SIZE",
                "PS2A string range overflowed",
            )
        })?;
        let raw = self.string_pool.get(start..end).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_STRING_RANGE",
                "PS2A validated string lies outside the private pool",
            )
        })?;
        let (text, malformed) = SHIFT_JIS.decode_without_bom_handling(raw);
        if malformed {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_ENCODING",
                "PS2A validated string became invalid Shift-JIS",
            ));
        }
        Ok(text.into_owned())
    }

    /// Resolves the tag-zero string reference form accepted by the original
    /// CMVS 3.90 string resolver. Other tag domains refer to interpreter or
    /// engine state and remain unavailable until their backing stores have a
    /// proven runtime contract.
    pub fn resolve_private_tag_zero_string(
        &self,
        tagged_reference: u32,
    ) -> Result<String, CoreError> {
        if tagged_reference & 0xc000_0000 != 0 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_STRING_TAG",
                "PS2A string reference uses an unrecovered tag domain",
            ));
        }
        self.resolve_private_string_relative(tagged_reference)
    }
}

/// A bounded span in the decoded PS2A program section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aSourceSpan {
    pub offset: u32,
    pub byte_length: u16,
}

/// The subset of PS2A VM control instructions whose field layout is proven by
/// the CMVS 3.90 interpreter dispatch.  Command opcodes deliberately remain
/// outside this type until their handler-specific operand contracts are
/// recovered; treating their stack input as a fixed byte layout would corrupt
/// the program counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aControlInstruction {
    AbsoluteJump {
        span: CmvsPs2aSourceSpan,
        target: u32,
    },
    JumpWhenFlagClear {
        span: CmvsPs2aSourceSpan,
        target: u32,
    },
    JumpWhenFlagSet {
        span: CmvsPs2aSourceSpan,
        target: u32,
    },
    JumpWhenValueEquals {
        span: CmvsPs2aSourceSpan,
        expected_value: u32,
        target: u32,
    },
    /// The CMVS 3.90 interpreter transfers to the trailing target after
    /// reading a four-byte field whose high-level meaning is not recovered.
    /// Retain that field without naming it as a value or condition.
    JumpWithOpaqueField {
        span: CmvsPs2aSourceSpan,
        opaque_field: u32,
        target: u32,
    },
    NameIndexJump {
        span: CmvsPs2aSourceSpan,
        name_index: u16,
        target: u32,
    },
    NameIndexCall {
        span: CmvsPs2aSourceSpan,
        name_index: u16,
        target: u32,
    },
    StackReturn {
        span: CmvsPs2aSourceSpan,
        stack_adjust: u16,
    },
    StackDrop {
        span: CmvsPs2aSourceSpan,
        stack_bytes: u16,
    },
    FrameReturn {
        span: CmvsPs2aSourceSpan,
    },
    ScriptReturn {
        span: CmvsPs2aSourceSpan,
    },
    /// A call frame whose destination comes from interpreter state rather
    /// than bytecode. The span is proven, but a static decoder cannot resolve
    /// the state-owned target.
    StateTargetCall {
        span: CmvsPs2aSourceSpan,
    },
    PushCurrentCondition {
        span: CmvsPs2aSourceSpan,
    },
    PushImmediate {
        span: CmvsPs2aSourceSpan,
        stack_bytes: u16,
        value: u32,
    },
}

/// One lossless token from the `0x201` PS2A value-expression form.
///
/// The interpreter consumes ordinary value tokens as an opcode plus a 32-bit
/// payload and consumes `0x160..=0x172` as postfix operators.  The token kind
/// intentionally does not assign variable, string or arithmetic semantics
/// until the corresponding evaluator case has been recovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aValueExpressionToken {
    Value {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
        payload: u32,
    },
    Operator {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
    },
    NestedStackExpression {
        span: CmvsPs2aSourceSpan,
        expression: Box<CmvsPs2aStackExpression>,
    },
}

/// A `0x201` value expression, including its terminating `0x20f` marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aValueExpression {
    pub span: CmvsPs2aSourceSpan,
    pub tokens: Vec<CmvsPs2aValueExpressionToken>,
}

/// A tag-zero reference to the current PS2A string pool.
///
/// The type intentionally excludes other tag domains exposed by the original
/// string resolver.  They require state-owned backing stores and must not be
/// coerced into a pool offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aPrivateStringReference {
    pub relative_offset: u32,
}

/// Lossless node from the `0x200` stack-expression grammar.
///
/// Variants retain only the bytecode field layout proven by the static
/// evaluator.  In particular, names such as `Indexed` describe the encoded
/// shape, not a claim about the source-level variable model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aStackExpressionNode {
    Value {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
        payload: u32,
    },
    Operator {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
    },
    Nested {
        expression: Box<CmvsPs2aStackExpression>,
    },
    Prefix {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
        payload: Option<u32>,
        index: Option<u16>,
        operands: Vec<CmvsPs2aStackExpression>,
    },
}

/// A bounded `0x200` stack expression including its `0x20f` terminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aStackExpression {
    pub span: CmvsPs2aSourceSpan,
    pub nodes: Vec<CmvsPs2aStackExpressionNode>,
}

/// Lossless node from the CMVS 3.90 `0x202` numeric-expression grammar.
///
/// This retains the evaluator's byte framing only.  The child expressions are
/// the separately proven `0x200` stack-expression form; no arithmetic or
/// source-language type semantics are inferred from the opcode value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aNumericExpressionNode {
    Value {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
        payload: u32,
    },
    Operator {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
    },
    StackExpression {
        expression: Box<CmvsPs2aStackExpression>,
    },
    Prefix {
        span: CmvsPs2aSourceSpan,
        opcode: u16,
        payload: Option<u32>,
        index: Option<u16>,
        operands: Vec<CmvsPs2aStackExpression>,
    },
}

/// A bounded `0x202` numeric expression including its `0x20f` terminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aNumericExpression {
    pub span: CmvsPs2aSourceSpan,
    pub nodes: Vec<CmvsPs2aNumericExpressionNode>,
}

/// One PS2A instruction boundary that is safe to advance without interpreting
/// commercial script data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aInstructionFrame {
    Control(CmvsPs2aControlInstruction),
    ValueExpression(CmvsPs2aValueExpression),
    StackExpression(CmvsPs2aStackExpression),
    NumericExpression(CmvsPs2aNumericExpression),
    Command {
        span: CmvsPs2aSourceSpan,
        command_id: u16,
    },
}

impl CmvsPs2aInstructionFrame {
    pub fn span(&self) -> CmvsPs2aSourceSpan {
        match self {
            Self::Control(instruction) => match instruction {
                CmvsPs2aControlInstruction::AbsoluteJump { span, .. }
                | CmvsPs2aControlInstruction::JumpWhenFlagClear { span, .. }
                | CmvsPs2aControlInstruction::JumpWhenFlagSet { span, .. }
                | CmvsPs2aControlInstruction::JumpWhenValueEquals { span, .. }
                | CmvsPs2aControlInstruction::JumpWithOpaqueField { span, .. }
                | CmvsPs2aControlInstruction::NameIndexJump { span, .. }
                | CmvsPs2aControlInstruction::NameIndexCall { span, .. }
                | CmvsPs2aControlInstruction::StackReturn { span, .. }
                | CmvsPs2aControlInstruction::StackDrop { span, .. }
                | CmvsPs2aControlInstruction::FrameReturn { span }
                | CmvsPs2aControlInstruction::ScriptReturn { span }
                | CmvsPs2aControlInstruction::StateTargetCall { span }
                | CmvsPs2aControlInstruction::PushCurrentCondition { span }
                | CmvsPs2aControlInstruction::PushImmediate { span, .. } => *span,
            },
            Self::ValueExpression(expression) => expression.span,
            Self::StackExpression(expression) => expression.span,
            Self::NumericExpression(expression) => expression.span,
            Self::Command { span, .. } => *span,
        }
    }
}

/// Aggregate evidence for the PS2A bytecode pattern used by the reference
/// tooling to address the string pool.  This deliberately does not assign VM
/// semantics to the pattern and never exposes the referenced text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aStringReferenceCensus {
    pub candidate_count: u32,
    pub valid_string_start_count: u32,
    pub invalid_offset_count: u32,
}

fn source_span(
    start: usize,
    end: usize,
    code: &'static str,
) -> Result<CmvsPs2aSourceSpan, CoreError> {
    let byte_length = end
        .checked_sub(start)
        .ok_or_else(|| invalid(code, "PS2A source span underflowed"))?;
    Ok(CmvsPs2aSourceSpan {
        offset: u32::try_from(start)
            .map_err(|_| invalid(code, "PS2A source span offset is too large"))?,
        byte_length: u16::try_from(byte_length)
            .map_err(|_| invalid(code, "PS2A source span is too large"))?,
    })
}

fn validate_program_counter(script: &CmvsScript, target: u32) -> Result<u32, CoreError> {
    let target = usize::try_from(target).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_CONTROL",
            "PS2A control target exceeds platform bounds",
        )
    })?;
    if target >= script.bytecode.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_CONTROL",
            "PS2A control target lies outside the program section",
        ));
    }
    u32::try_from(target).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_CONTROL",
            "PS2A control target is too large",
        )
    })
}

fn le_u16(source: &[u8], offset: usize, code: &'static str) -> Result<u16, CoreError> {
    let bytes: [u8; 2] = source
        .get(offset..offset + 2)
        .ok_or_else(|| invalid(code, "PS2A instruction is truncated"))?
        .try_into()
        .map_err(|_| invalid(code, "PS2A instruction is truncated"))?;
    Ok(u16::from_le_bytes(bytes))
}

fn le_u32_at(source: &[u8], offset: usize) -> Result<u32, CoreError> {
    let bytes: [u8; 4] = source
        .get(offset..offset + 4)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_CONTROL",
                "PS2A instruction is truncated",
            )
        })?
        .try_into()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_CONTROL",
                "PS2A instruction is truncated",
            )
        })?;
    Ok(u32::from_le_bytes(bytes))
}

fn le_u32(source: &[u8], offset: usize) -> Result<u32, CoreError> {
    let bytes: [u8; 4] = source
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PS2A_HEADER", "PS2A header is truncated"))?
        .try_into()
        .map_err(|_| invalid("ASTRA_EMU_CMVS_PS2A_HEADER", "PS2A header is truncated"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn checked_usize(value: u32) -> Result<usize, CoreError> {
    usize::try_from(value).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_SIZE",
            "PS2A size exceeds platform bounds",
        )
    })
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
