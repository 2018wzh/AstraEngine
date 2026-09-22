mod crossfade_audio;
mod effect_channels;
mod effect_elements;
mod effect_quads;
mod filters;
mod memory;
mod objects;
mod presentation;
mod resources;
mod scripts;
mod settings;
mod system;
mod texture_children;
mod textures;

use super::*;

/// Whether an effect channel index below 8 holds a live channel object, the
/// recovered guard every channel-scoped handler applies through
/// `this[channel + 742]`.
pub(super) fn effect_channel_is_occupied(state: &CmvsPs2aVmState, channel: u32) -> bool {
    channel <= 7 && state.slot_objects.contains_key(&(742_u32 << 8 | channel))
}

pub(super) fn execute_command(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    stack_pop_bytes: u8,
    stack_words: &[crate::CmvsPs2aCommandStackWord],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    // The pop width and the read set are independent in the original:
    // handlers such as `sub_483B10` inspect stack words without popping
    // them. Each read is bounds-checked individually below.
    if effect_kind == CmvsPs2aCommandEffectKind::StopDispatch {
        state.dispatch_stopped = true;
        return Ok(None);
    }
    let mut values = Vec::with_capacity(stack_words.len());
    for word in stack_words {
        let value = read_stack_word_from_top(state, word.offset_from_top_bytes)?;
        values.push((word.kind, value));
    }
    if effect_trace_enabled() && is_effect_trace_target(effect_kind) {
        tracing::trace!(
            event = "astra.emu.cmvs.vm.effect",
            pc = state.program_counter,
            frame = state.current_frame
        );
    }
    drop_stack_bytes(state, u16::from(stack_pop_bytes))?;
    match effect_kind {
        CmvsPs2aCommandEffectKind::StoreComponentConstant { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterConstant { .. }
        | CmvsPs2aCommandEffectKind::ClearInterpreterTableWord { .. }
        | CmvsPs2aCommandEffectKind::LoadInterpreterTableWord { .. }
        | CmvsPs2aCommandEffectKind::MarkInterpreterRecordUnused { .. }
        | CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip { .. }
        | CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString { .. }
        | CmvsPs2aCommandEffectKind::RequestWindowCaption { .. }
        | CmvsPs2aCommandEffectKind::StoreComponentWord { .. }
        | CmvsPs2aCommandEffectKind::StoreBoundedInterpreterString { .. }
        | CmvsPs2aCommandEffectKind::BuildPrefixedWindowCaption { .. }
        | CmvsPs2aCommandEffectKind::NoOpCommand
        | CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult { .. }
        | CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult { .. }
        | CmvsPs2aCommandEffectKind::AppendPrivateStringToList { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterWord { .. }
        | CmvsPs2aCommandEffectKind::StoreRandomModulo { .. }
        | CmvsPs2aCommandEffectKind::StoreStringLength { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterTimestampedRecord { .. }
        | CmvsPs2aCommandEffectKind::StoreProcessGlobalWord { .. }
        | CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean { .. }
        | CmvsPs2aCommandEffectKind::MutateProcessFlagRange
        | CmvsPs2aCommandEffectKind::StoreProcessIndexedRange
        | CmvsPs2aCommandEffectKind::StoreProcessFloatRange
        | CmvsPs2aCommandEffectKind::StoreProcessStringRange => {
            memory::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::StartResourceChannel { .. }
        | CmvsPs2aCommandEffectKind::PlayChannelSound { .. }
        | CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot { .. }
        | CmvsPs2aCommandEffectKind::ClearResourceSlot { .. } => {
            resources::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CreateSlotObject { .. }
        | CmvsPs2aCommandEffectKind::CreateSingletonSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroySingletonSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroySlotObject { .. }
        | CmvsPs2aCommandEffectKind::MoveSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroyBoundedSlotObject { .. } => {
            objects::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::RunFilterGraphControl { .. }
        | CmvsPs2aCommandEffectKind::ReplaceFilterChainSlot { .. }
        | CmvsPs2aCommandEffectKind::DestroyFilterChainSlot { .. }
        | CmvsPs2aCommandEffectKind::QueryFilterChainSlot { .. }
        | CmvsPs2aCommandEffectKind::UpdateFilterChainRecord { .. }
        | CmvsPs2aCommandEffectKind::UpdateFilterChainParameterBlock { .. }
        | CmvsPs2aCommandEffectKind::ApplyFilterChainRecords { .. }
        | CmvsPs2aCommandEffectKind::PollFilterChainSelection { .. }
        | CmvsPs2aCommandEffectKind::ReadActiveFilterChainSelection { .. } => {
            filters::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::StoreChannelVisibilityRecord { .. }
        | CmvsPs2aCommandEffectKind::ResetChannelVisibilityRecord { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelWord { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelOrigin { .. }
        | CmvsPs2aCommandEffectKind::SetEffectPlaybackField { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackMode { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelValuePair { .. }
        | CmvsPs2aCommandEffectKind::ApplyEffectChannelOperation { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectState { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectChannelActivity { .. }
        | CmvsPs2aCommandEffectKind::QueryChannelOccupancy { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelPair { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelBlock { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelEnabled { .. }
        | CmvsPs2aCommandEffectKind::CreateEffectChannel { .. } => {
            effect_channels::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::StorePresentationFrameFields { .. }
        | CmvsPs2aCommandEffectKind::StoreClampedPresentationSize { .. }
        | CmvsPs2aCommandEffectKind::CommitScreenParams { .. }
        | CmvsPs2aCommandEffectKind::WaitScreenCommit { .. }
        | CmvsPs2aCommandEffectKind::QueryScreenPending { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRgb { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRotation { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScale { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenOffset { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenField { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScalePair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenFlag { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenPair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenOffsetDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScaleDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRgbDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRotationDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenFieldDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScalePairDirect { .. }
        | CmvsPs2aCommandEffectKind::StoreScriptSlotPresentationState { .. }
        | CmvsPs2aCommandEffectKind::QuerySceneLayer { .. }
        | CmvsPs2aCommandEffectKind::ClearSceneLayer { .. }
        | CmvsPs2aCommandEffectKind::TogglePresentationMode { .. }
        | CmvsPs2aCommandEffectKind::ResetPresentationBuffers
        | CmvsPs2aCommandEffectKind::SelectPresentationSlot { .. }
        | CmvsPs2aCommandEffectKind::SelectRenderViewport { .. } => {
            presentation::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::SelectEffectChild { .. }
        | CmvsPs2aCommandEffectKind::SetEffectTextSurface { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementVisible { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectElementExists { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementSize { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementAuxiliaryPair { .. }
        | CmvsPs2aCommandEffectKind::ApplyEffectElementOperation { .. }
        | CmvsPs2aCommandEffectKind::EffectChildCommand { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChildEnabled { .. } => {
            effect_elements::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect { .. }
        | CmvsPs2aCommandEffectKind::SelectEffectQuad { .. }
        | CmvsPs2aCommandEffectKind::DeselectEffectQuad { .. }
        | CmvsPs2aCommandEffectKind::ActivateEffectQuad { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectQuadState { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectQuadActive { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectPosition { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectPointerHit { .. } => {
            effect_quads::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch
        | CmvsPs2aCommandEffectKind::ReadInputLatches { .. }
        | CmvsPs2aCommandEffectKind::ReadAutomaticAdvance { .. }
        | CmvsPs2aCommandEffectKind::ClearTextureChannelTable { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureParentResource { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureParentResourceExtended { .. }
        | CmvsPs2aCommandEffectKind::CommitTexturePresentation { .. }
        | CmvsPs2aCommandEffectKind::InitializeTextureParentSurface { .. } => {
            textures::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::QueryNewestSaveSlot { .. }
        | CmvsPs2aCommandEffectKind::SelectSystemCursor { .. }
        | CmvsPs2aCommandEffectKind::RequestSystemStateLoad { .. }
        | CmvsPs2aCommandEffectKind::RecreateSaveImageOwner { .. }
        | CmvsPs2aCommandEffectKind::QuerySaveImageProgress { .. }
        | CmvsPs2aCommandEffectKind::StoreSaveImageEnabled
        | CmvsPs2aCommandEffectKind::SaveImageOwnerWaitOrDestroy { .. }
        | CmvsPs2aCommandEffectKind::DestroySaveImageOwner
        | CmvsPs2aCommandEffectKind::QuerySaveImageEnabled { .. }
        | CmvsPs2aCommandEffectKind::QueryRendererDisplayMode { .. }
        | CmvsPs2aCommandEffectKind::QueryPointerPosition { .. }
        | CmvsPs2aCommandEffectKind::HitTestPointerRect { .. }
        | CmvsPs2aCommandEffectKind::HitTestPointerRegion { .. } => {
            system::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::StoreSettingBooleanPlain { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean { .. }
        | CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord { .. }
        | CmvsPs2aCommandEffectKind::StoreIndexedSettingBoolean { .. }
        | CmvsPs2aCommandEffectKind::StoreDerivedSettingsPair { .. }
        | CmvsPs2aCommandEffectKind::StoreSettingsWordReset { .. }
        | CmvsPs2aCommandEffectKind::StoreSettingsPredicate { .. }
        | CmvsPs2aCommandEffectKind::DeriveSettingsSnapshot { .. }
        | CmvsPs2aCommandEffectKind::StoreSettingsRecord { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingBooleanPair { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingBoundedWord { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingNonNegativeWord { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemConfigBoolean { .. } => {
            settings::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CallScript
        | CmvsPs2aCommandEffectKind::ReloadRootScript
        | CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord { .. }
        | CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy { .. } => {
            scripts::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ResetTextureChild { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureChildResource { .. }
        | CmvsPs2aCommandEffectKind::InitializeTextureChildSurface { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildRect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord { .. } => {
            texture_children::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::FadeOutAudio
        | CmvsPs2aCommandEffectKind::StopAudio
        | CmvsPs2aCommandEffectKind::PlayAudio
        | CmvsPs2aCommandEffectKind::PlayPairedAudio => {
            crossfade_audio::execute(effect_kind, &values)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
