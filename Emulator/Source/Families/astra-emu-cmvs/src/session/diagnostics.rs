use crate::CmvsPs2aInstructionFrame;
use astra_emu_family_api::FamilyError;

pub(super) fn instruction(
    frame: &CmvsPs2aInstructionFrame,
    mut failure: FamilyError,
) -> FamilyError {
    let kind = match frame {
        CmvsPs2aInstructionFrame::Control(_) => "control".to_owned(),
        CmvsPs2aInstructionFrame::ValueExpression(_) => "value expression".to_owned(),
        CmvsPs2aInstructionFrame::StackExpression(_) => "stack expression".to_owned(),
        CmvsPs2aInstructionFrame::NumericExpression(_) => "numeric expression".to_owned(),
        CmvsPs2aInstructionFrame::Command { command_id, .. } => format!("command {command_id}"),
    };
    failure.message = format!(
        "CMVS {kind} at offset {}: {}",
        frame.span().offset,
        failure.message
    )
    .into();
    failure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn every_instruction_kind_preserves_error_code_and_adds_only_safe_context() {
        let span = CmvsPs2aSourceSpan {
            offset: 42,
            byte_length: 2,
        };
        let frames = [
            (
                CmvsPs2aInstructionFrame::Command {
                    span,
                    command_id: 353,
                },
                "command 353",
            ),
            (
                CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span,
                    stack_bytes: 4,
                    value: 9999,
                }),
                "control",
            ),
            (
                CmvsPs2aInstructionFrame::ValueExpression(CmvsPs2aValueExpression {
                    span,
                    tokens: Vec::new(),
                }),
                "value expression",
            ),
            (
                CmvsPs2aInstructionFrame::StackExpression(CmvsPs2aStackExpression {
                    span,
                    nodes: Vec::new(),
                }),
                "stack expression",
            ),
            (
                CmvsPs2aInstructionFrame::NumericExpression(CmvsPs2aNumericExpression {
                    span,
                    nodes: Vec::new(),
                }),
                "numeric expression",
            ),
        ];
        for (frame, kind) in frames {
            let failure = instruction(&frame, crate::family::error("CODE", "failure"));
            assert_eq!(failure.code.as_str(), "CODE");
            assert_eq!(
                failure.message.as_str(),
                format!("CMVS {kind} at offset 42: failure")
            );
        }
    }
}
