use super::*;

pub(super) fn execute(
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ClearMessagePanel,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _voice_control)],
        ) => Ok(Some(CmvsPs2aVmAction::ClearMessagePanel)),
        (CmvsPs2aCommandEffectKind::ResetMessagePanel, []) => {
            Ok(Some(CmvsPs2aVmAction::ResetMessagePanel))
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
