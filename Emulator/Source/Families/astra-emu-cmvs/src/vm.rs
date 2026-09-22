mod dispatch;
mod state;
use astra_emu_sdk::CoreError;
pub use dispatch::execute_cmvs390_frame;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
pub use state::CmvsPs2aVmState;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    cmvs390_command_contract, CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWordKind,
    CmvsPs2aControlInstruction, CmvsPs2aInstructionFrame, CmvsPs2aPrivateStringReference,
    CmvsResourceChannelBank,
};

/// The decoded-script byte budget shared with the PS2A parser; segment
/// sizes come from decoded scripts, so the same bound applies to the
/// snapshot they round-trip through.
const MAX_SCRIPT_BYTES: usize = 64 * 1024 * 1024;

/// Gates the per-instruction development traces so production and evidence
/// runs do not pay for stderr formatting in the interpreter hot loop.
fn vm_trace_enabled() -> bool {
    tracing::enabled!(tracing::Level::TRACE)
}

/// Gates the development dump of recovered effect-graph operands. This is a
/// reverse-engineering aid for recovering the channel/quad pipeline; it never
/// participates in execution.
fn effect_trace_enabled() -> bool {
    tracing::enabled!(tracing::Level::TRACE)
}

fn is_effect_trace_target(kind: CmvsPs2aCommandEffectKind) -> bool {
    matches!(
        kind,
        CmvsPs2aCommandEffectKind::CreateEffectChannel { .. }
            | CmvsPs2aCommandEffectKind::SetEffectChannelEnabled { .. }
            | CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag { .. }
            | CmvsPs2aCommandEffectKind::ForwardEffectChannelWord { .. }
            | CmvsPs2aCommandEffectKind::ForwardEffectChannelBlock { .. }
            | CmvsPs2aCommandEffectKind::ForwardEffectChannelPair { .. }
            | CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad { .. }
            | CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry { .. }
            | CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect { .. }
            | CmvsPs2aCommandEffectKind::SelectEffectQuad { .. }
            | CmvsPs2aCommandEffectKind::DeselectEffectQuad { .. }
            | CmvsPs2aCommandEffectKind::ActivateEffectQuad { .. }
            | CmvsPs2aCommandEffectKind::EffectChildCommand { .. }
            | CmvsPs2aCommandEffectKind::SetEffectChildEnabled { .. }
            | CmvsPs2aCommandEffectKind::SelectEffectChild { .. }
    )
}

const MAX_STACK_BYTES: usize = 64 * 1024;
const MAX_PROCESS_FLAG_BITS: u32 = 0x0004_0000;
const MAX_PROCESS_INDEXED_WORDS: u32 = 0x0000_1000;
const MAX_PROCESS_FLOAT_WORDS: u32 = 0x0000_0800;
const MAX_PROCESS_STRING_SLOTS: u32 = 0x0000_0080;
/// Bounds proven by the CMVS 3.90 slot-table handlers; indices at or above
/// this limit only raise the recovered interpreter error flag.
const SLOT_OBJECT_COUNT: u32 = 256;
/// Byte offset of the interpreter error-flag word OR-ed by the recovered
/// slot-table bounds checks (`this+2633` on the original object layout).
const INTERPRETER_ERROR_FLAG_OFFSET: u16 = 10532;
const INTERPRETER_ERROR_FLAG_MASK: u32 = 0x10;
const MAX_FILTER_CHAIN_RECORDS_PER_BANK: usize = 4096;

/// Validates snapshot-carried interpreter memory before it can resume live
/// execution. The wire payload is untrusted even when its outer envelope and
/// hash are valid.
pub fn validate_cmvs390_vm_state(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    if state.execution_failed {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_FAILED",
            "failed VM state cannot be saved or restored",
        ));
    }
    if state.stack_bytes.len() > MAX_STACK_BYTES
        || state.stack_initialized.len() != state.stack_bytes.len()
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS snapshot variable-buffer shape is invalid",
        ));
    }
    let cursor = usize::try_from(state.stack_cursor_bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS snapshot stack cursor exceeds platform bounds",
        )
    })?;
    if cursor > state.stack_bytes.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS snapshot stack cursor exceeds its variable buffer",
        ));
    }
    if state.call_frame_bases.is_empty()
        || state.call_frame_bases[0] != 0
        || state.call_frame_bases.len() > MAX_STACK_BYTES / 4
        || state
            .call_frame_bases
            .windows(2)
            .any(|bases| bases[0] > bases[1])
        || state
            .call_frame_bases
            .last()
            .is_some_and(|base| *base > state.stack_cursor_bytes)
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_CALL_DEPTH",
            "CMVS snapshot call-frame bases are invalid",
        ));
    }
    for parent in state
        .texture_parents
        .keys()
        .chain(state.texture_children.keys())
    {
        if !state
            .slot_objects
            .contains_key(&slot_object_key(1924, u32::from(*parent))?)
        {
            return Err(invalid(
                "ASTRA_EMU_CMVS_VM_TEXTURE",
                "CMVS snapshot texture state has no live parent object",
            ));
        }
    }
    if state.texture_parents.values().any(|parent| {
        parent.resource.is_some() != parent.resource_frame.is_some()
            || parent
                .resource_frame
                .is_some_and(|frame| !state.script_frames.contains_key(&frame))
    }) || state.texture_children.values().flatten().any(|(_, child)| {
        child.resource.is_some() != child.resource_frame.is_some()
            || child
                .resource_frame
                .is_some_and(|frame| !state.script_frames.contains_key(&frame))
    }) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_TEXTURE_IDENTITY",
            "CMVS snapshot texture binding has no matching script-frame identity",
        ));
    }
    // The data-segment budget mirrors the script decode bound; recorded
    // words must stay inside the loaded segment declared for their frame.
    for (frame, size) in &state.script_data_segment_sizes {
        if u64::from(*size) > MAX_SCRIPT_BYTES as u64 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS snapshot data-segment size exceeds the script budget",
            ));
        }
        if *size > 0 && !state.script_frames.contains_key(frame) {
            return Err(invalid(
                "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS snapshot data-segment size has no matching script frame",
            ));
        }
    }
    for (frame, words) in &state.script_data_segment_words {
        let Some(size) = state.script_data_segment_sizes.get(frame) else {
            return Err(invalid(
                "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS snapshot data-segment words have no declared segment size",
            ));
        };
        for offset in words.keys() {
            let end = offset.checked_add(4).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
                    "CMVS snapshot data-segment word offset overflowed",
                )
            })?;
            if end > *size {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
                    "CMVS snapshot data-segment word is outside the loaded segment",
                ));
            }
        }
    }
    Ok(())
}

/// One recovered resource channel slot record from the case-176 handler.
/// String content stays owned by the active script pool; only the private
/// reference and the control words are retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsResourceChannelSlotRecord {
    pub enabled: bool,
    pub volume: u32,
    pub loop_playback: bool,
    pub resource: CmvsPs2aPrivateStringReference,
}

/// Recovered state of one child texture below a case-32 container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsTextureChildState {
    pub resource: Option<CmvsPs2aPrivateStringReference>,
    pub resource_frame: Option<u16>,
    pub surface_initialized: bool,
    pub rect_words: Option<[u32; 4]>,
    pub position_words: Option<[u32; 2]>,
    pub auxiliary_pair_words: Option<[u32; 2]>,
    pub auxiliary_word: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsTextureParentState {
    pub resource: Option<CmvsPs2aPrivateStringReference>,
    pub resource_frame: Option<u16>,
    pub surface_initialized: bool,
    pub rect_words: Option<[u32; 4]>,
    pub position_words: Option<[u32; 2]>,
    pub auxiliary_pair_words: Option<[u32; 2]>,
    pub auxiliary_word: Option<u32>,
}

/// One keyed record allocated by the recovered filter-chain helper.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsFilterChainRecord {
    pub words: [u32; 4],
    pub parameter_blocks: BTreeMap<u8, CmvsFilterChainParameterBlock>,
}

/// The recovered screenshot owner (`sub_47AFF0`): the four construction
/// words, the case-551 enabled flag and the case-550 progress delta word,
/// which only the unrecovered render path updates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsSaveImageOwnerState {
    pub construction_words: [u32; 4],
    pub enabled: bool,
    pub progress_delta: u32,
}

/// One physical 16-byte parameter block in a filter-chain record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsFilterChainParameterBlock {
    pub word: u32,
    pub short_words: [u16; 6],
}

/// The serialized identity of one loaded script frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsScriptFrameIdentity {
    pub script_uri: String,
    pub script_hash: astra_core::Hash256,
}

/// The recovered maximum nested-script frame index accepted by `sub_478080`.
pub const CMVS_MAX_SCRIPT_CALL_INDEX: u32 = 3;

/// One payload-free segment of a recovered interpreter string buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsStringSegment {
    /// An interpreter-owned prefix field copied before any script string.
    /// The field content is engine state and remains unrecovered.
    InterpreterPrefixField { field_offset: u16 },
    /// A resolved tag-zero private reference into the active script pool.
    PrivateString(CmvsPs2aPrivateStringReference),
    /// A fixed run of ASCII spaces proven by the case-353 caption gap.
    LiteralSpaces { count: u8 },
}

/// A host storage request emitted by a recovered command. The nucleus keeps
/// dispatch blocked until the runtime provider resolves the request; the
/// request itself carries no payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aStorageRequest {
    /// The recovered case-296 path: load the system state archive assembled
    /// from the interpreter path buffer at the named byte offset.
    LoadSystemState { path_buffer_offset: u16 },
}

/// A payload-free command boundary. The host resolves a reference against the
/// private script pool only when it needs to issue an ephemeral text lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aVmAction {
    /// Cases 160/164: alternate two audio channels, optionally crossfading.
    PlayCrossfadeAudio {
        primary: CmvsPs2aPrivateStringReference,
        secondary: Option<CmvsPs2aPrivateStringReference>,
        fade_ms: u32,
        playback_flag: bool,
    },
    /// Case 162: fade out the active channel and clear its resource names.
    FadeOutAudio { fade_ms: u32 },
    /// Case 161: stop both channels and reset the channel selector.
    StopAudio,
    /// Case 778: one bounded table slot object is released.
    DestroyBoundedSlotObject { table_dword_index: u32, slot: u32 },
    /// Case 349: the effect playback objects for one channel/effect pair
    /// were rebuilt; the media graph itself stays unrecovered.
    CreateEffectChannel { channel: u32, effect: u32 },
    /// Case 347: one effect channel's enable flag was forwarded to the
    /// effect engine.
    SetEffectChannelEnabled { channel: u32, enabled: bool },
    /// Case 322: one opaque word was forwarded to an occupied effect channel.
    ForwardEffectChannelWord { channel: u32 },
    /// Case 376: one opaque block word was forwarded to an occupied channel.
    ForwardEffectChannelBlock { channel: u32 },
    /// Case 324: two opaque words were forwarded to an occupied channel.
    ForwardEffectChannelPair { channel: u32 },
    /// Case 325: four opaque words were forwarded to an occupied channel.
    ForwardEffectChannelQuad { channel: u32 },
    /// Case 350: the effect position was queried into two result words.
    QueryEffectPosition,
    /// Cases 384-391: one child of an occupied channel ran its action.
    EffectChildCommand { channel: u32 },
    /// Case 400: one effect quad configuration was applied.
    ConfigureEffectQuad { channel: u32 },
    /// Case 286: one effect playback record's text surface received a value
    /// and two script string references; the host resolves them for length
    /// validation and never stores the text in the snapshot.
    ConfigureEffectTextSurface {
        effect: u32,
        first: CmvsPs2aPrivateStringReference,
        second: CmvsPs2aPrivateStringReference,
    },
    /// Case 351: the pointer hit test ran and its canonical result was
    /// stored.
    QueryEffectPointerHit,
    /// Case 402: quad `quad` on the occupied channel was shown.
    SelectEffectQuad { channel: u32, quad: u32 },
    /// Case 403: quad `quad` on the occupied channel was hidden.
    DeselectEffectQuad { channel: u32, quad: u32 },
    /// Case 404: the effect state was queried into three result words.
    QueryEffectState,
    /// Case 405: the quad existence query stored its canonical result.
    QueryEffectQuadActive,
    /// Case 378: a child of an occupied channel received a boolean.
    SetEffectChildEnabled { channel: u32, enabled: bool },
    /// Case 368: one named sound start was requested through a channel.
    SelectEffectChild { channel: u32 },
    PlayChannelSound {
        channel: u32,
        name: crate::CmvsPs2aPrivateStringReference,
    },
    /// Case 778: one bounded table slot object is released; out-of-range
    /// selectors only raise the error mask.

    /// A recovered resource-channel setup request. This is not yet bound to a
    /// media provider; the host must reject an unbound channel contract.
    StartResourceChannel {
        bank: CmvsResourceChannelBank,
        channel: u8,
        resource: CmvsPs2aPrivateStringReference,
    },
    /// The recovered case-56 path binds a private PB resource reference to
    /// one child texture. The host must resolve it through the active VFS and
    /// explicitly bound CMVS image decoder before presenting it.
    LoadTextureResource {
        parent_slot: u8,
        child_id: u16,
        resource: CmvsPs2aPrivateStringReference,
    },
    /// The recovered case-48 path binds a private PB resource directly to a
    /// top-level texture container.
    LoadTextureParentResource {
        parent_slot: u8,
        resource: CmvsPs2aPrivateStringReference,
    },
    /// Case 71 completes the recovered four-stage surface configuration.
    /// The host may present the selected parent or child only after this
    /// boundary; preceding setters remain state-only mutations.
    CommitTextureSurface {
        parent_slot: u8,
        child_id: Option<u16>,
    },
    /// The recovered case-22 path asks the host to create the directories
    /// along the assembled path buffer. The host owns the storage policy;
    /// the VM never touches the filesystem itself.
    EnsurePathDirectories { buffer_offset: u16 },
    /// The recovered case-143 path asks the host to set the main window
    /// caption from the ordered, payload-free segment sequence.
    SetWindowCaption { segments: Vec<CmvsStringSegment> },
    /// The recovered case-354 path asks the host to resolve the reference,
    /// apply the bounded buffer write, or raise the error-flag mask when the
    /// resolved text exceeds the bound.
    StoreBoundedString {
        buffer_offset: u16,
        max_text_bytes: u16,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        reference: CmvsPs2aPrivateStringReference,
    },
    /// The recovered case-177 notification path is only emitted when the
    /// version-pinned gate field is non-zero in the retained state.
    NotifyChannelVisibility { target_index: u8, enabled: bool },
    /// The recovered case-691 path asks the host to apply the window layout
    /// derived from the stored presentation size field.
    ApplyPresentationLayout { size_field_offset: u16 },
    /// The recovered case-692 path asks the host to toggle the presentation
    /// mode using the two named interpreter fields.
    TogglePresentationMode {
        first_field_offset: u16,
        second_field_offset: u16,
        enabled: bool,
    },
    /// The recovered case-153 path asks the host to select the system cursor
    /// with the given index.
    SelectSystemCursor { index: u32 },
    /// The recovered case-296 path asks the host to load the system state
    /// archive; dispatch waits for the provider resolution.
    StorageRequest(CmvsPs2aStorageRequest),
    /// The recovered case-548 path is waiting for a confirm or primary
    /// pointer edge before it commits the filter-graph control.
    FilterGraphInputWait,
    /// The recovered case-535 selection poll waits for a directional,
    /// confirm, cancel, or primary-pointer edge owned by the host.
    FilterChainSelectionWait { bank: u8 },
    /// The recovered case-129 path asks the host to load the named script
    /// into the given frame and continue dispatch there.
    CallScript {
        frame: u16,
        name: CmvsPs2aPrivateStringReference,
    },
    /// The recovered case-128 path asks the host to load the named script as
    /// the new root frame-0 script and continue dispatch at its entry PC.
    ReloadRootScript {
        name: CmvsPs2aPrivateStringReference,
    },
}

mod effect;
mod execute;
mod expression;
mod filterchain;
mod memory;
mod numeric;
mod runtime;
mod schedule;
mod stack;
mod text;

pub use self::effect::CmvsEffectChannel;
use self::effect::*;
use self::execute::*;
use self::expression::*;
use self::filterchain::*;
use self::memory::*;
use self::numeric::*;
use self::runtime::*;
use self::schedule::*;
use self::stack::*;
use self::text::*;

pub use self::schedule::{apply_system_save, rebuild_frame_entry_queue};

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
