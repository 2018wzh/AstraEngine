use super::*;

pub(super) fn execute(
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::FadeOutAudio,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, fade_ms)],
        ) => Ok(Some(CmvsPs2aVmAction::FadeOutAudio { fade_ms: *fade_ms })),
        (CmvsPs2aCommandEffectKind::StopAudio, []) => Ok(Some(CmvsPs2aVmAction::StopAudio)),
        (
            CmvsPs2aCommandEffectKind::PlayAudio,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, primary), (CmvsPs2aCommandStackWordKind::OpaqueU32, fade_ms), (CmvsPs2aCommandStackWordKind::BooleanU32, playback_flag)],
        ) => Ok(Some(CmvsPs2aVmAction::PlayCrossfadeAudio {
            secondary: None,
            primary: tag_zero_reference(*primary)?,
            fade_ms: *fade_ms,
            playback_flag: *playback_flag != 0,
        })),
        (
            CmvsPs2aCommandEffectKind::PlayPairedAudio,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, primary), (CmvsPs2aCommandStackWordKind::TaggedStringReference, secondary), (CmvsPs2aCommandStackWordKind::OpaqueU32, fade_ms), (CmvsPs2aCommandStackWordKind::BooleanU32, playback_flag)],
        ) => Ok(Some(CmvsPs2aVmAction::PlayCrossfadeAudio {
            secondary: Some(tag_zero_reference(*secondary)?),
            primary: tag_zero_reference(*primary)?,
            fade_ms: *fade_ms,
            playback_flag: *playback_flag != 0,
        })),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
