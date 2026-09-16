//! Version-fingerprinted PS2A command contracts proven from the CMVS 3.90
//! dispatch table and command handlers.
//!
//! A contract is intentionally absent until both its stack consumption and
//! observable effect boundary are static-analysis evidence.  Callers must
//! block on `None`; command ids are not an extension point for guessed
//! behavior.

mod words;
use words::*;
mod opcodes_0_71;
mod opcodes_138_322;
mod opcodes_323_354;
mod opcodes_368_401;
mod opcodes_402_529;
mod opcodes_530_741;
mod opcodes_747_940;
mod opcodes_80_137;
type CommandFields = (
    u8,
    CmvsPs2aCommandEffectKind,
    &'static [CmvsPs2aCommandStackWord],
);

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The command result shape consumed by the CMVS 3.90 interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aCommandEffectKind {
    /// The dispatch table returns a negative result, ending the current
    /// interpreter dispatch cycle. It is not evidence that the entire game
    /// session has reached a terminal state.
    StopDispatch,
    /// Creates a message-panel entry using the default speaker field held by
    /// the original engine.
    MessageBody,
    /// `sub_478BB0` clears both 64-byte message-panel text buffers through
    /// `sub_48EB60` and hands the consumed word to the panel's voice object
    /// (`sub_48C0E0`), toggling the panel playback bit. The voice object is
    /// outside the recovered subset; the consumed word stays opaque.
    ClearMessagePanel,
    /// Case 161 (`sub_48EC20` on the panel): both text buffers clear, the
    /// status word resets, the voice object stops both channels and the
    /// playback bit clears. No stack words are consumed.
    ResetMessagePanel,
    /// Case 778 (`sub_47C920`): releases the object in one of three slots at
    /// dword indices 796..=798; a selector above two raises the 0x1000000
    /// error mask.
    DestroyBoundedSlotObject {
        table_dword_index: u32,
        max_slot: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// `sub_4164F0` stores the boolean at one fixed field of the settings
    /// object without the reset preamble some handlers perform.
    StoreSettingBooleanPlain {
        setting_field_offset: u16,
    },
    /// Case 349 (`sub_47ED00`): rebuilds the effect playback object for one
    /// of eight effect slots bound to one of twelve playback entries and
    /// consumes six opaque configuration words. Out-of-range channels raise
    /// the 0x10000 mask and out-of-range effects the 0x1000 mask. The media
    /// graph behind the objects is not recovered; occupancy is observable.
    SetEffectChannelEnabled {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47F830` (case 322): forwards one opaque word to an occupied
    /// effect channel; out-of-range or unoccupied channels raise the mask.
    ForwardEffectChannelWord {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47D770` (case 376): forwards one opaque block word to an
    /// occupied effect channel's object.
    ForwardEffectChannelQuad {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// Case 350: reads the effect engine's current two-word position into
    /// two result fields.
    QueryEffectPosition {
        result_x_field_offset: u32,
        result_y_field_offset: u32,
    },
    /// Case 351: runs the pointer hit test and stores the canonical result.
    QueryEffectPointerHit {
        result_field_offset: u32,
    },
    ForwardEffectChannelPair {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    ForwardEffectChannelBlock {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47D850` (case 378): selects one child of an occupied effect
    /// channel by value and forwards a boolean to it.
    SetEffectChildEnabled {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// `sub_47E690` (case 368): resolves the consumed string reference and
    /// starts it through one of eight sound channels; failures raise 0x80
    /// and channel table faults 0x10000.
    SelectEffectChild {
        effect_table_dword_index: u32,
        max_channel: u32,
        error_mask: u32,
    },
    /// Cases 384-391 (`sub_47E240`/`E2B0`/`E4C0`/`E540`/`E630`/`E3A0`/`E400`/
    /// `E460`): select one child of an occupied effect channel by value and
    /// run a child-specific action over the remaining opaque words. The
    /// channel table faults raise 0x10000; a missing child raises it too.
    SelectEffectQuad {
        error_mask: u32,
    },
    /// `sub_47F640` (case 403) -> `sub_466D90`: hide one of 32 quads on the
    /// occupied channel by clearing the quad record's visibility flag at
    /// byte offset `76*quad + 40`; a channel or selector fault raises
    /// `error_mask`. Returns `0x4008`.
    DeselectEffectQuad {
        error_mask: u32,
    },
    QueryEffectState {
        first_field_offset: u32,
        second_field_offset: u32,
        third_field_offset: u32,
    },
    /// `sub_479C20` (case 416): copy the texture manager status words
    /// (`sub_45CCB0` reads manager dwords 271/272) into the interpreter
    /// state fields. The command takes no stack operands and returns
    /// `0x4000`, so the dispatcher pops nothing.
    QueryTextureManagerState {
        state_field_offset: u32,
        aux_field_offset: u32,
    },
    /// `sub_4793B0` (case 751): texture-readiness poll. When the override
    /// field is zero the canonical readiness value is stored; any non-zero
    /// override forces success. No stack operands; returns `0x4000`.
    StoreTextureReadyFlag {
        result_field_offset: u32,
        override_field_offset: u32,
    },
    /// `sub_484020` (case 51): one script slot selector; occupancy of the
    /// slot-table object is required (else the slot error mask raises) and
    /// the presentation state lands in four consecutive interpreter words.
    /// Returns `0x4004`.
    StoreScriptSlotPresentationState {
        slot_table_dword_index: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47EFC0` (case 334): sets one effect channel's playback activity
    /// flag and clears its transient flag. Two stack words (channel, flag);
    /// returns `0x4008`. Out-of-range selectors raise the error mask and an
    /// unoccupied channel fails fast instead of dereferencing null.
    SetEffectChannelPlaybackFlag {
        effect_table_dword_index: u32,
        error_mask: u32,
        activity_state_key_base: u32,
    },
    QueryEffectQuadActive {
        result_field_offset: u32,
    },
    /// `sub_47E770` (case 331): stores whether the effect channel slot is
    /// occupied into the interpreter result field; an out-of-range channel
    /// raises the channel error mask. Returns `0x4004`.
    QueryChannelOccupancy {
        result_field_offset: u32,
        channel_table_dword_index: u32,
        error_mask: u32,
    },
    EffectChildCommand {
        pop_bytes: u16,
    },
    /// `sub_47F400` (case 400) -> `sub_466CB0`: one of 32 quads on an occupied
    /// effect channel receives six `__int16` geometry values at a sub-selector
    /// slot; faults raise 0x20000.
    ConfigureEffectQuadGeometry {
        error_mask: u32,
    },
    /// `sub_47F530` (case 401) -> `sub_466D10`: one of 32 quads on an occupied
    /// effect channel receives four `__int16` hit-rectangle words; faults
    /// raise 0x20000.
    ConfigureEffectQuadRect {
        error_mask: u32,
    },
    /// `sub_47FA40` (case 336) -> `sub_4678C0`: sets the occupied channel's
    /// origin from a mode word and two coordinates; an out-of-range or empty
    /// channel raises 0x10000. Returns 0x4010.
    SetEffectChannelOrigin {
        error_mask: u32,
    },
    /// `sub_47F9A0` (case 337) -> `sub_4679B0`: stores the occupied channel's
    /// playback mode and, for modes 1/3, two coordinates; an out-of-range or
    /// empty channel raises 0x10000. Returns 0x4010.
    SetEffectChannelPlaybackMode {
        error_mask: u32,
    },
    /// `sub_47F1B0` (case 346) -> `sub_467CF0`: stores two dwords on the
    /// occupied channel (record words 618/619); an out-of-range or empty
    /// channel raises 0x10000. Returns 0x400C.
    SetEffectChannelValuePair {
        error_mask: u32,
    },
    /// `sub_47FB20` (case 330) -> `sub_467A10`: forwards the occupied
    /// channel's sub-object at record word 631 to `sub_464C00`; an
    /// out-of-range or empty channel raises 0x10000. Returns 0x4004.
    ApplyEffectChannelOperation {
        error_mask: u32,
    },
    /// `sub_484D00` (case 104) -> `sub_457B90`: commits the renderer's pending
    /// screen parameters when the boolean operand is non-zero. Returns
    /// 0x4004.
    CommitScreenParams {
        error_mask: u32,
    },
    /// `sub_484D40` (case 105): waits for the pending screen commit. With no
    /// pending commit it clears the result field and returns; otherwise it
    /// commits and reports success. Returns 0x4004.
    WaitScreenCommit {
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_484CD0` (case 106): publishes the renderer's pending-commit flag
    /// into the interpreter result field. No stack operands; returns 0x4000.
    QueryScreenPending {
        result_field_offset: u32,
    },
    /// `sub_484A70` (case 90) -> `sub_456DF0`: stores the screen object's RGB
    /// fields 1..3 as hundredth-rounded floats. Returns 0x4010.
    ConfigureScreenRgb {
        error_mask: u32,
    },
    /// `sub_484820` (case 91) -> `sub_456C00`: stores the screen object's
    /// rotation field 14. Returns 0x4008.
    ConfigureScreenRotation {
        error_mask: u32,
    },
    /// `sub_4849D0` (case 89) -> `sub_456D80`: stores the screen object's
    /// scale fields 9/10 and their halves at 15/16. Returns 0x400C.
    ConfigureScreenScale {
        error_mask: u32,
    },
    /// `sub_484A20` (case 88) -> `sub_456DC0`: stores the screen object's
    /// offset fields 7/8 and field 6. Returns 0x4010.
    ConfigureScreenOffset {
        error_mask: u32,
    },
    /// `sub_484760`/`sub_4847F0` (cases 92/95): store one value into the
    /// screen object's field 11 or 0. Returns 0x4008.
    ConfigureScreenField {
        error_mask: u32,
        field_offset: u32,
    },
    /// `sub_484980` (case 93) -> `sub_456D60`: stores two values into the
    /// screen object's fields 15/16. Returns 0x400C.
    ConfigureScreenScalePair {
        error_mask: u32,
    },
    /// `sub_484940` (case 94) -> `sub_457CA0`: stores one boolean into the
    /// screen object's field 37. Returns 0x4008.
    ConfigureScreenFlag {
        error_mask: u32,
    },
    /// `sub_4847A0` (case 103) -> `sub_456BE0`: stores a pair into the screen
    /// object's fields 17/18. Returns 0x400C.
    ConfigureScreenPair {
        error_mask: u32,
    },
    /// `sub_484E80` (case 96) -> `sub_456DC0`: stores the offset fields 7/8
    /// and field 6 without a target selector. Returns 0x400C.
    ConfigureScreenOffsetDirect {
        error_mask: u32,
    },
    /// `sub_484E40` (case 97) -> `sub_456D80`: stores the scale fields 9/10
    /// and their halves without a target selector. Returns 0x4008.
    ConfigureScreenScaleDirect {
        error_mask: u32,
    },
    /// `sub_484ED0` (case 98) -> `sub_456DF0`: the RGB fields 1..3 without a
    /// target selector. Returns 0x400C.
    ConfigureScreenRgbDirect {
        error_mask: u32,
    },
    /// `sub_484AC0` (case 99) -> `sub_456C00`: the rotation field 14 without a
    /// target selector. Returns 0x4004.
    ConfigureScreenRotationDirect {
        error_mask: u32,
    },
    /// `sub_484730` (case 100) -> `sub_456690`: the field 11 without a target
    /// selector. Returns 0x4004.
    ConfigureScreenFieldDirect {
        error_mask: u32,
        field_offset: u32,
    },
    /// `sub_484E00` (case 101) -> `sub_456D60`: the fields 15/16 without a
    /// target selector. Returns 0x4008.
    ConfigureScreenScalePairDirect {
        error_mask: u32,
    },
    /// Case 156: the recovered dispatcher body is empty and returns `0x4008`;
    /// it consumes two stack words without reading them. Returns 0x4008.
    NoOpCommand,
    /// Cases 276/277/278 (`sub_488FC0`/`sub_488F80`/`sub_488F40`): write one
    /// field of an effect playback record selected below 12; an out-of-range
    /// or unregistered index raises 0x1000. Returns 0x4008.
    SetEffectPlaybackField {
        error_mask: u32,
        field_offset: u32,
    },
    /// `sub_488E70` (case 286) -> `sub_462C10`: stores a value dword and two
    /// script strings on an effect playback record's text surface; an
    /// out-of-range or unregistered index raises 0x1000. Returns 0x4010.
    SetEffectTextSurface {
        error_mask: u32,
    },
    /// `sub_47D850` (case 378) -> `sub_445EA0`: stores one effect child
    /// element's visibility; a missing element raises 0x10000. Returns
    /// 0x400C.
    SetEffectElementVisible {
        error_mask: u32,
    },
    /// `sub_47D630` (case 379): answers whether one effect child element
    /// exists; a missing element raises 0x10000. Returns 0x4008.
    QueryEffectElementExists {
        error_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47D7F0` (case 380) -> `sub_445E80`: stores one effect child
    /// element's size; a missing element raises 0x10000. Returns 0x4010.
    SetEffectElementSize {
        error_mask: u32,
    },
    /// `sub_47D590` (case 382) -> `sub_445E40`: forwards one effect child
    /// element; a missing element raises 0x10000. Returns 0x4008.
    SetEffectElementAuxiliary {
        error_mask: u32,
    },
    /// `sub_47D5E0` (case 383) -> `sub_445E60`: forwards one effect child
    /// element; a missing element raises 0x10000. Returns 0x4008.
    SetEffectElementAuxiliaryPair {
        error_mask: u32,
    },
    /// `sub_47DC80` (case 463): forwards one effect child element's texture
    /// object to `sub_42BF50`; a missing element raises 0x10000. Returns
    /// 0x4008.
    ApplyEffectElementOperation {
        error_mask: u32,
    },
    PlayChannelSound {
        channel_table_dword_index: u32,
        max_channel: u32,
        play_error_mask: u32,
        channel_error_mask: u32,
    },
    CreateEffectChannel {
        effect_table_dword_index: u32,
        playback_table_dword_index: u32,
        max_channel: u32,
        max_effect: u32,
        channel_error_mask: u32,
        effect_error_mask: u32,
    },
    /// Creates a message-panel entry with an explicit speaker and body.
    MessageSpeakerBody,
    /// Stores one opaque word in a CMVS interpreter-owned field. The field
    /// offset is part of the version-pinned contract; its game-level meaning
    /// remains deliberately unnamed until a consumer is recovered.
    StoreInterpreterWord {
        field_offset: u32,
    },
    /// Writes a proven immediate value to a CMVS interpreter-owned field.
    StoreInterpreterConstant {
        field_offset: u32,
        value: u32,
    },
    /// Zeroes one word of a bounded interpreter-owned word table selected by
    /// the stack-top index (`sub_478B50` indexes the 10-word family starting
    /// at the version-pinned base offset); out-of-range indices block
    /// dispatch because the original writes outside the recovered fields.
    ClearInterpreterTableWord {
        table_base_offset: u32,
        table_word_count: u32,
    },
    /// Copies one word of the bounded interpreter-owned word table selected
    /// by the stack-top index into a version-pinned destination field
    /// (`sub_478B80` reads the zero-initialized `this+3307` family); indices
    /// outside the family block dispatch.
    LoadInterpreterTableWord {
        table_base_offset: u32,
        table_word_count: u32,
        field_offset: u32,
    },
    /// Writes all-ones into the first word of one 28-byte record of the
    /// bounded interpreter record table (`sub_47CC80` indexes the 64-record
    /// family at byte offset 13280); indices above 63 are silently ignored.
    MarkInterpreterRecordUnused {
        record_table_base_offset: u32,
        record_count: u32,
        record_stride_bytes: u32,
    },
    /// The recovered case-747 flag round trip (`sub_479400`): a non-zero
    /// selector stores the second word as the boolean flag at the
    /// version-pinned field; a zero selector copies the stored flag back
    /// into the expression-result field.
    InterpreterFlagRoundTrip {
        flag_field_offset: u32,
        result_field_offset: u32,
    },
    /// Resets one interpreter-owned string buffer to a recovered interpreter
    /// prefix field and appends the resolved private string reference. The
    /// original helper copies the prefix, not an empty string, so the buffer
    /// content is retained as an ordered, payload-free segment sequence.
    /// `ensure_directories` marks the case-22 path that additionally walks
    /// the assembled path and creates missing directories.
    StorePrefixedInterpreterString {
        buffer_offset: u16,
        prefix_field_offset: u16,
        ensure_directories: bool,
    },
    /// Stores one opaque word through a recovered CMVS process-global setter.
    /// The absolute address is a CMVS 3.90 fingerprint, not a portable symbol.
    StoreProcessGlobalWord {
        address: u32,
    },
    /// Stores a canonical zero/one value through a recovered CMVS
    /// process-global boolean setter.
    StoreProcessGlobalBoolean {
        address: u32,
    },
    /// Sets or clears a bounded range in the recovered CMVS process flag
    /// bitmap. The bit ids remain opaque until their consumers are recovered.
    MutateProcessFlagRange,
    /// Writes one opaque value across a bounded CMVS process-indexed range.
    StoreProcessIndexedRange,
    /// Writes one raw f32 bit-pattern across a bounded CMVS process range.
    StoreProcessFloatRange,
    /// Copies one resolved private string reference to a bounded CMVS process
    /// string-slot range.
    StoreProcessStringRange,
    /// Writes a proven immediate value to a field on an interpreter-owned
    /// component selected by its parent object offset.
    StoreComponentConstant {
        component_offset: u16,
        field_offset: u16,
        value: u32,
    },
    /// Begins the recovered CMVS resource-channel setup boundary for one of
    /// the statically proven channel banks. The bank fingerprint is the
    /// original setup-handler address; slot counts come from its bounds
    /// check.
    StartResourceChannel {
        bank: CmvsResourceChannelBank,
    },
    /// Creates one opaque object in the bounded interpreter slot table,
    /// replacing any previous occupant. Out-of-range slots only update the
    /// recovered interpreter error-flag word, mirroring the original handler.
    CreateSlotObject {
        table_byte_offset: u16,
    },
    /// Creates one opaque object in a fixed singleton slot of the bounded
    /// interpreter slot table (the case-544 filter-graph owner replaces any
    /// previous occupant) and stores a version-pinned interpreter word, all
    /// without inspecting the four consumed stack words.
    CreateSingletonSlotObject {
        table_byte_offset: u16,
        slot: u8,
        field_offset: u32,
        value: u32,
    },
    /// The recovered case-548 filter-graph control. The handler clears the
    /// version-pinned flag word first; a missing singleton or a faulted
    /// remaining-length check (`sub_458E50`) then returns 0x4004, a healthy
    /// zero control returns 0xA000 without a pop, and the non-zero control
    /// playback start (0xC004) needs the unrecovered media graph.
    RunFilterGraphControl {
        table_byte_offset: u16,
        slot: u8,
        field_offset: u32,
    },
    /// Destroys the occupant of a fixed singleton slot of the bounded
    /// interpreter slot table (`sub_47A5F0` frees the case-544 filter-graph
    /// owner when present) without touching the stack.
    DestroySingletonSlotObject {
        table_byte_offset: u16,
        slot: u8,
    },
    /// Replaces the occupant of one six-slot filter-chain table
    /// (`sub_485FD0` bounds the bank to six and the channel to 256,
    /// raising the 0x100000 error mask otherwise); the construction words
    /// stay opaque and only slot occupancy is observable.
    ReplaceFilterChainSlot {
        table_byte_offset: u16,
        max_bank: u32,
        max_channel_id: u32,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Destroys one of the six recovered interactive-chain banks. The
    /// original frees the owned chain object, clears the table entry and
    /// raises the same error mask as construction when the bank is outside
    /// the recovered range.
    DestroyFilterChainSlot {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Queries one six-slot filter-chain table entry (`sub_486BC0`): a
    /// missing bank raises the 0x100000 error mask, while a live bank
    /// appends one opaque queue node through `sub_468560` and stores one
    /// into the expression-result field; the query word stays unobserved.
    QueryFilterChainSlot {
        table_byte_offset: u16,
        max_bank: u32,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// Updates four opaque words in the keyed record of one live filter-chain
    /// bank. A missing key is a no-op; a missing or out-of-range bank raises
    /// the recovered error mask.
    UpdateFilterChainRecord {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Writes one recovered 16-byte parameter block in a keyed filter-chain
    /// record. The process-global backend selector controls the original
    /// block layout; selectors outside its recovered range are no-ops.
    UpdateFilterChainParameterBlock {
        table_byte_offset: u16,
        max_bank: u8,
        backend_mode_address: u32,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Applies every queued record of one live filter-chain bank through the
    /// original backend path (`sub_486D30` -> `sub_4686A0`). The command has
    /// no expression result; the deterministic revision records the apply
    /// boundary while the still-opaque backend fields remain in VM state.
    ApplyFilterChainRecords {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Polls one recovered six-bank interactive record chain. The original
    /// handler updates the active record on directional input and returns
    /// the active record id on confirm, `-2` on cancel, or `-1` while idle.
    PollFilterChainSelection {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// Reads the active record id from one recovered interactive chain. A
    /// live bank without an active record yields `-1`.
    ReadActiveFilterChainSelection {
        table_byte_offset: u16,
        max_bank: u8,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// Destroys the opaque object held in one bounded interpreter slot.
    DestroySlotObject {
        table_byte_offset: u16,
    },
    /// Moves the opaque object from the second bounded slot into the first,
    /// clearing the source slot.
    MoveSlotObject {
        table_byte_offset: u16,
    },
    /// Builds the recovered prefix+string caption and hands it to the host as
    /// a window-caption request. The VM never materializes the text itself.
    /// `prefix_field_offset` is `None` for the raw-caption path proven by
    /// case 352.
    RequestWindowCaption {
        prefix_field_offset: Option<u16>,
    },
    /// Copies the resolved stack string into one bounded interpreter buffer,
    /// raising the recovered error-flag mask when the resolved text exceeds
    /// the byte bound, mirroring `sub_48A670`.
    StoreBoundedInterpreterString {
        buffer_offset: u16,
        max_text_bytes: u16,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// Builds the case-353 caption: the interpreter prefix field, a
    /// conditional two-space gap selected by the unrecovered host gate and
    /// the resolved stack string; stores the resolved reference into the
    /// named buffer and hands the segments to the host, mirroring
    /// `sub_48A710`.
    BuildPrefixedWindowCaption {
        prefix_field_offset: u16,
        buffer_offset: u16,
    },
    /// Writes the recovered per-channel visibility record: an enabled flag
    /// and a touched constant, both at version-pinned interpreter offsets
    /// derived from the bounded channel index. Out-of-range channels only
    /// raise the recovered error-flag mask `0x200`.
    StoreChannelVisibilityRecord {
        enabled_field_base_offset: u16,
        touched_field_base_offset: u16,
        record_stride_bytes: u8,
        notify_gate_field_offset: u16,
    },
    /// Resets one channel visibility record (`sub_48A2B0`): the per-channel
    /// name buffer goes back to the interpreter prefix field and the touched
    /// word is zeroed; channels above five only raise the 0x200 error mask.
    ResetChannelVisibilityRecord {
        name_buffer_base_offset: u16,
        touched_field_base_offset: u16,
        record_stride_bytes: u8,
        error_flag_mask: u32,
        teardown_slot_table_offset: u16,
    },
    /// Writes one stack word to a field on an interpreter-referenced
    /// component selected by its parent field offset.
    StoreComponentWord {
        component_field_offset: u16,
        field_offset: u16,
    },
    /// Stores the stack pair into two interpreter fields and derives a
    /// non-zero pair flag, all at version-pinned byte offsets.
    StorePresentationFrameFields {
        top_field_offset: u16,
        second_field_offset: u16,
        active_field_offset: u16,
    },
    /// Clamps the signed stack word into the recovered presentation size
    /// range and stores it; the host receives a layout request referencing
    /// the stored field.
    StoreClampedPresentationSize {
        field_offset: u16,
        negative_replacement: u32,
        maximum: u32,
    },
    /// Loads the named script into a nested frame through the host and
    /// transfers dispatch there, mirroring `sub_478080`. The frame index
    /// bound is enforced before any state changes.
    CallScript,
    /// Loads the named script as the new root frame-0 script, resets the
    /// interpreter stack, frame table and current value, and transfers
    /// dispatch to the new script's entry PC, mirroring `sub_4781B0`.
    ReloadRootScript,
    /// Writes one bounded 28-byte record into the recovered interpreter
    /// record table: a zero flag, the current frame index, four stack
    /// words and a clock word. Out-of-range indices leave the table
    /// untouched, matching `sub_47CBE0`.
    StoreInterpreterTimestampedRecord {
        table_base_offset: u16,
        record_stride_bytes: u8,
        max_index: u8,
    },
    /// Pops the stack-top coroutine label and transfers dispatch to the
    /// (frame, PC) pair the 28-byte interpreter record table registered for
    /// it, mirroring `sub_47CB00`. Before the transfer the advanced PC, the
    /// current frame index and the frame-counter field are pushed as the
    /// nested-call return record and the frame-counter field clears, matching
    /// the `sub_478080` push order. The original masks the label to the
    /// 64-record bound; a record without a registered PC (missing or all-
    /// ones) blocks instead of emulating the original's unchecked jump.
    ResumeInterpreterCoroutineRecord {
        record_table_base_offset: u16,
        record_stride_bytes: u8,
        max_index_mask: u8,
    },
    /// Case 549 (`sub_47AFF0`): the previous screenshot owner is destroyed,
    /// a fresh one is built from the four consumed words with zeroed flag
    /// and progress fields, and the version-pinned owner handle field
    /// replaces its value. Returns 0x4010.
    RecreateSaveImageOwner {
        handle_field_offset: u16,
    },
    /// Case 550 (`sub_47AFC0`): a missing owner writes the all-ones result
    /// and a present owner writes `construction_words[3]` minus its progress
    /// delta (all-ones unchanged), mirroring `sub_459CC0`. No stack pop.
    QuerySaveImageProgress {
        result_field_offset: u32,
    },
    /// Case 551 (`sub_47B0F0`): only a present owner's enabled flag is set
    /// from the consumed boolean; the pop happens regardless. Returns 0x4004.
    StoreSaveImageEnabled,
    /// Case 552 (`sub_47B160`): a missing owner writes one; a present owner
    /// with a set boolean waits for the screenshot completion, which the
    /// recovered subset blocks because the render path is not recovered;
    /// a present owner with a clear boolean is destroyed and writes one.
    SaveImageOwnerWaitOrDestroy {
        result_field_offset: u32,
    },
    /// Case 553 (`sub_47B120`): destroys a present owner. No stack pop.
    DestroySaveImageOwner,
    /// Case 524 (`sub_47B0B0`): writes one only when an owner exists and
    /// its enabled flag is set, mirroring `sub_432F60` on the owner's
    /// field 5. No stack pop.
    QuerySaveImageEnabled {
        result_field_offset: u32,
    },
    /// Case 294 (`sub_48A570`): the version-pinned result pair (system
    /// registers 1 and 2) receives the render object's inner display-mode
    /// fields 6 and 7 through `sub_432F70`/`sub_432F30`. The renderer
    /// object model is not recovered, so execution blocks instead of
    /// inventing the register values. No stack pop.
    QueryRendererDisplayMode {
        renderer_field_offset: u32,
        result_field_offset: u32,
    },
    /// Case 200 (`sub_480280`): writes the session pointer position into the
    /// version-pinned result pair (system registers 1/2) through
    /// `sub_45CF40`, which reads the input-manager object's fields 180/181.
    /// No stack pop.
    QueryPointerPosition {
        x_field_offset: u32,
        y_field_offset: u32,
    },
    /// Case 202 (`sub_480060`): inclusive-bounds hit test of the session
    /// pointer against the stack-provided rectangle; the boolean lands in
    /// the version-pinned result field. Returns 0x4010.
    HitTestPointerRect {
        result_field_offset: u32,
    },
    /// Case 203 (`sub_4800F0`): GDI region hit test (`PtInRegion`) of the
    /// session pointer against the stack-provided shape. Only the
    /// rectangular shape (type 1) is recovered; ellipse, round-rectangle
    /// and polygon rasterization stay blocking. Returns 0x401C.
    HitTestPointerRegion {
        result_field_offset: u32,
    },
    /// Case 333 (`sub_47F270`): reads one occupied effect channel's
    /// transient activity flag (the object's field +8, which case 334
    /// resets when the playback flag changes) into the version-pinned
    /// result field. An out-of-range or empty channel only raises the
    /// channel error mask. Returns 0x4004.
    QueryEffectChannelActivity {
        activity_state_key_base: u32,
        max_channel: u32,
        effect_table_dword_index: u32,
        error_mask: u32,
        result_field_offset: u32,
    },
    /// Case 718 (`sub_472F10` case body): copies one version-pinned
    /// interpreter word directly into the result field without touching the
    /// stack. Returns 0x4000.
    LoadInterpreterWordToResult {
        source_field_offset: u32,
        result_field_offset: u32,
    },
    /// Case 750 (`sub_472F10` case body): writes one when the version-pinned
    /// interpreter word at byte offset 1596 is non-zero into the result
    /// field, and zero otherwise. Returns 0x4000.
    LoadInterpreterWordBooleanToResult {
        source_field_offset: u32,
        result_field_offset: u32,
    },
    /// Cases 424/426/428/430/432 (`sub_47A0B0`/`sub_479D70`/`sub_479E30`/
    /// `sub_47A070`/`sub_479FB0`): write the two booleans of one scene-object
    /// layer group into the version-pinned result pair at byte offsets 81220
    /// and 81236. The group is addressed by its base dword index; the reader
    /// touches `base` and `base + 1`. Returns 0x4000.
    QuerySceneLayer {
        base_word: u32,
        result_field_offset: u32,
        aux_field_offset: u32,
    },
    /// Cases 425/427/429/431/433 (`sub_4616E0`/`sub_4612B0`/`sub_461310`/
    /// `sub_4616C0`/`sub_4613D0`): clear the base and `base + 2` words of one
    /// scene-object layer group, leaving `base + 1` untouched. No stack
    /// operands; returns 0x4000.
    ClearSceneLayer {
        base_word: u32,
    },
    /// Case 397 (`sub_47E300`): copies one effect quad sub-object's fields    /// (indices 17, 18, 19, 8, 9, 10) into six version-pinned result
    /// fields. The headless effect graph holds no animated state, so the
    /// recovered subset writes zeros. An out-of-range/empty channel raises
    /// `empty_error_mask`; a missing sub-object raises
    /// `missing_error_mask`. Returns 0x4008.
    QueryEffectQuadState {
        effect_table_dword_index: u32,
        empty_error_mask: u32,
        missing_error_mask: u32,
        result0_field_offset: u32,
        result1_field_offset: u32,
        result2_field_offset: u32,
        result3_field_offset: u32,
        result8_field_offset: u32,
        result9_field_offset: u32,
    },
    /// Case 406 (`sub_47F330`): selects/activates one quad slot on an
    /// occupied effect channel with a value word. The headless effect graph
    /// keeps only occupancy, so the value is not retained. Returns 0x400C.
    ActivateEffectQuad {
        error_mask: u32,
    },
    /// Case 695 (`sub_485970`): scans the save-slot range
    /// `[start, start + count)` for the newest `saveNN.dat` and writes its
    /// slot index, or -1 when the range is invalid. The headless host
    /// exposes no writable save files, so the result is always -1. Returns
    /// 0x4008.
    QueryNewestSaveSlot {
        result_field_offset: u32,
    },
    /// Applies the recovered settings-object reset and stores one boolean
    /// setting at a version-pinned field offset.
    StoreSystemSettingBoolean {
        setting_field_offset: u16,
    },
    /// Applies the recovered settings-object reset and stores one opaque
    /// stack word at a version-pinned field offset, mirroring `sub_45C470`.
    StoreSettingsWordReset {
        setting_field_offset: u16,
    },
    /// The recovered case-212 handler (`sub_484F60`) reduces the next
    /// pseudo-random draw modulo the stack-top bound and stores the result
    /// at a version-pinned interpreter field. The draw comes from the
    /// deterministic PRNG state retained in the snapshot; a zero bound is
    /// blocking because the original's divide would fault.
    StoreRandomModulo {
        target_field_offset: u32,
    },
    /// The recovered case-176 handler (`sub_48A110`) registers one resource
    /// channel slot record: out-of-range slots only raise the error-flag
    /// mask, otherwise the name/volume/enabled/loop words are copied into
    /// the fixed 52-byte record table. The playback gate (`sub_48DB70`)
    /// stays unrecovered and blocks when its gate field is non-zero.
    RegisterResourceChannelSlot {
        max_slot: u8,
        table_base_offset: u16,
        record_stride_bytes: u8,
        gate_field_offset: u16,
        error_flag_field_offset: u16,
        error_flag_mask: u32,
    },
    /// The recovered case-34 handler (`sub_4805E0`) selects the top-level
    /// texture container with the stack-top word and recreates its child
    /// texture selected by the second word. A missing/out-of-range parent
    /// raises the parent error mask; child ids at or above 1024 are ignored
    /// by `sub_4442C0`, matching the original no-op.
    ResetTextureChild {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Loads a tagged PB resource into a live top-level texture container.
    /// Handler `sub_483D70` stores the synchronous loader result in the
    /// interpreter expression-result word and returns 0x4008.
    LoadTextureParentResource {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        result_field_offset: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Case 410 (`sub_45C5A0`): resets the presentation manager's internal
    /// counters; no observable headless state.
    ResetPresentationBuffers,
    /// `sub_4804C0` (case 46): selects the presentation target slot.
    /// Returns 0x4008.
    SelectPresentationSlot {
        slot_table_dword_index: u32,
        max_slot: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// `sub_480620` (case 35): selects the renderer viewport for a slot.
    /// Returns 0x4008.
    SelectRenderViewport {
        slot_table_dword_index: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// `sub_478EC0` (case 721): stores a config boolean or reads the skip
    /// flag. Returns 0x4008.
    StoreSystemConfigBoolean {
        config_field_offset: u32,
        skip_flag_field_offset: u32,
        result_field_offset: u32,
    },
    /// `sub_480560` (case 39): stores whether the selected script slot is
    /// occupied. Returns 0x4008.
    QueryScriptSlotOccupancy {
        slot_table_dword_index: u32,
        max_slot: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
        result_field_offset: u32,
    },
    /// `sub_47AF00` (case 555): commits the presented texture for the
    /// frame; the wait or show outcome ends the dispatch for the frame.
    CommitTexturePresentation {
        result_field_offset: u32,
        present_object_field_offset: u32,
    },
    /// Loads a tagged PB resource while creating the top-level texture
    /// container if absent. Handler `sub_47ACA0` (case 554) consumes the
    /// name, slot and three opaque words, replaces any previous container,
    /// stores the loader success at the result field and returns 0x4014.
    LoadTextureParentResourceExtended {
        texture_table_base_offset: u32,
        result_field_offset: u32,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Recreates the live top-level container's 0x88-byte surface owner.
    /// Handler `sub_482F20` consumes one parent id and returns 0x4004.
    InitializeTextureParentSurface {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// The recovered case-47 handler (`sub_4803D0`) tears down every live
    /// entry of the 256-slot texture channel table without reading the stack.
    ClearTextureChannelTable {
        channel_table_base_offset: u32,
    },
    /// The recovered case-248 handler (`sub_47D220`) resolves the stack-top
    /// tagged string and stores its `lstrlenA` byte length into one
    /// interpreter word. The length comes from the frame's payload-free
    /// string-length table injected at script load.
    StoreStringLength {
        target_field_offset: u32,
    },
    /// The recovered case-56 handler (`sub_480700`) resolves the third stack
    /// word as a PB image and loads it into the child selected by the first
    /// two words of one live top-level texture container.
    LoadTextureChildResource {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// The recovered case-80 handler (`sub_480660`) creates the renderable
    /// surface state of one existing texture child.
    InitializeTextureChildSurface {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// The recovered case-68 handler stores four transform/rectangle words
    /// on one child texture (`sub_446B30`).
    ConfigureTextureChildRect {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// The recovered case-70 handler stores two position words on one child
    /// texture (`sub_446AD0`).
    ConfigureTextureChildPosition {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// Cases 69 and 71 address the same bounded parent/child texture tree,
    /// but their live mutation remains unrecovered and therefore blocking.
    ConfigureTextureAuxiliaryPair {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    ConfigureTextureAuxiliaryWord {
        texture_table_base_offset: u32,
        max_parent_slot: u8,
        max_child_id: u16,
        error_flag_field_offset: u32,
        parent_error_mask: u32,
        child_error_mask: u32,
    },
    /// The recovered case-321 handler (`sub_47F8D0`) clears the resource
    /// slot selected by the stack-top index (0..=7); out-of-range indices
    /// raise the slot error mask. Slot contents are unrecovered, so only
    /// membership is retained.
    ClearResourceSlot {
        max_slot: u8,
        error_flag_field_offset: u32,
        error_flag_mask: u32,
    },
    /// Copies one recovered settings-object field into one interpreter
    /// word without touching the settings object, mirroring `sub_45A0E0`.
    CopySettingsFieldToInterpreterWord {
        settings_field_offset: u16,
        target_field_offset: u32,
    },
    /// Reads the settings-object field selected by the bounded stack index
    /// (`sub_45C300`) and stores its truthiness into one interpreter word.
    StoreIndexedSettingBoolean {
        target_field_offset: u32,
        max_index: u8,
    },
    /// Applies the recovered case-857 settings derivation (`sub_45C390`):
    /// zeroes the result field when both selector fields agree on zero, or
    /// when both are non-zero, then stores the result field and the second
    /// selector's truthiness into two interpreter words.
    StoreDerivedSettingsPair {
        first_selector_field_offset: u16,
        second_selector_field_offset: u16,
        result_field_offset: u16,
        result_target_field_offset: u32,
        flag_target_field_offset: u32,
    },
    /// Applies the recovered settings predicate (`sub_45C370`): the stack
    /// word must equal one and the named signed settings field must be
    /// non-negative; the boolean result goes to one interpreter word.
    StoreSettingsPredicate {
        settings_field_offset: u16,
        target_field_offset: u32,
    },
    /// Applies the recovered case-869 snapshot derivation (`sub_45C3E0`):
    /// when the gate field is non-zero the four gated source fields are
    /// copied into the four interpreter targets; one additional settings
    /// field is always copied, and the gate's truthiness lands in the result
    /// word.
    DeriveSettingsSnapshot {
        gate_field_offset: u16,
        copy_source_field_offsets: [u16; 4],
        copy_target_field_offsets: [u32; 4],
        always_source_field_offset: u16,
        always_target_field_offset: u32,
        gate_target_field_offset: u32,
    },
    /// Writes five stack words into one bounded 20-byte settings record
    /// (`sub_45B260`); out-of-range indices leave the table untouched.
    StoreSettingsRecord {
        record_base_field_offset: u16,
        record_stride_bytes: u8,
        max_index: u8,
    },
    /// Applies the recovered settings-object reset and stores two boolean
    /// settings at version-pinned field offsets.
    StoreSystemSettingBooleanPair {
        first_field_offset: u16,
        second_field_offset: u16,
    },
    /// Stores one stack word into a bounded settings field without the
    /// reset path; values outside `[minimum, maximum]` leave the field
    /// untouched, matching the original setter.
    StoreSystemSettingBoundedWord {
        setting_field_offset: u16,
        minimum: u32,
        maximum: u32,
    },
    /// Stores one non-negative signed stack word into a settings field
    /// without the reset path; negative values leave the field untouched.
    StoreSystemSettingNonNegativeWord {
        setting_field_offset: u16,
    },
    /// Hands the recovered presentation-mode toggle to the host, naming the
    /// two interpreter fields the original layout helper consumes.
    TogglePresentationMode {
        first_field_offset: u16,
        second_field_offset: u16,
    },
    /// Appends the resolved private string to the version-pinned string list
    /// proven by case 152 and writes the new list length to the named
    /// interpreter field. List node payloads stay in the script pool.
    AppendPrivateStringToList {
        list_id: u16,
        length_field_offset: u32,
    },
    /// Selects the system cursor through the cursor-manager component
    /// referenced by the named interpreter field. The current-index field of
    /// the manager is retained so unchanged selections stay effect-free,
    /// matching the original guard.
    SelectSystemCursor {
        manager_field_offset: u16,
        current_index_field_offset: u16,
    },
    /// Asks the host to load the system state archive assembled from the
    /// named path buffer. The nucleus cannot continue past the request
    /// until the host resolves it, matching the original dependency on the
    /// file contents.
    RequestSystemStateLoad {
        path_buffer_offset: u16,
    },
}

/// One statically recovered resource-channel bank. Each bank is a fixed
/// array of channel records owned by a version-pinned interpreter component;
/// the discriminant order is part of the serialized contract.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum CmvsResourceChannelBank {
    /// Handler `sub_48AE50`; five channel records of 2052 bytes.
    FiveSlotSetup,
    /// Handler `sub_4405F0`; ten channel records of 2052 bytes.
    TenSlotSetup,
    /// Handler `sub_4331B0`; seven channel records.
    SevenSlotSetup,
    /// Handler `sub_441A70`; five channel records.
    FiveSlotAltSetup,
}

impl CmvsResourceChannelBank {
    /// The recovered slot count of this bank, taken from the original
    /// handler's bounds check.
    pub fn slot_count(self) -> u8 {
        match self {
            CmvsResourceChannelBank::FiveSlotSetup => 5,
            CmvsResourceChannelBank::TenSlotSetup => 10,
            CmvsResourceChannelBank::SevenSlotSetup => 7,
            CmvsResourceChannelBank::FiveSlotAltSetup => 5,
        }
    }
}

/// The proven representation of one four-byte word consumed by a CMVS 3.90
/// command handler.  These names deliberately describe the handler boundary,
/// rather than guessing game-level parameter names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsPs2aCommandStackWordKind {
    /// A value resolved by the original `sub_470D50` tagged-string resolver.
    /// Only tag-zero values can currently be materialized from the private
    /// PS2A string pool; other tag domains remain a runtime blocker.
    TaggedStringReference,
    /// A handler forwards this word without enough static evidence to assign a
    /// higher-level name or range.
    OpaqueU32,
    /// A handler uses this word as a zero/non-zero predicate.
    BooleanU32,
}

/// One word in the exact stack suffix a command handler consumes.  Offsets are
/// measured from the current stack top before the handler pop, so the topmost
/// word always has an offset of four bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aCommandStackWord {
    pub offset_from_top_bytes: u8,
    pub kind: CmvsPs2aCommandStackWordKind,
}

/// A fixed command stack contract. `stack_pop_bytes` is the low-byte pop
/// count returned by the original handler, not an inferred operand count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aCommandContract {
    pub command_id: u16,
    pub stack_pop_bytes: u8,
    pub effect_kind: CmvsPs2aCommandEffectKind,
    /// Static handler-derived stack word order. An empty slice means the
    /// handler does not pop stack data.
    pub stack_words: Vec<CmvsPs2aCommandStackWord>,
}

/// Looks up only CMVS 3.90 contracts recovered from the original command
/// dispatch and handler bodies.
pub fn cmvs390_command_contract(command_id: u16) -> Option<CmvsPs2aCommandContract> {
    let (stack_pop_bytes, effect_kind, stack_words) = match command_id {
        0 | 11 | 12 | 16 | 18 | 19 | 20 | 17 | 23 | 21 | 22 | 24 | 26 | 29 | 30 | 31 | 32 | 33
        | 34 | 35 | 39 | 40 | 46 | 47 | 48 | 51 | 56 | 64 | 68 | 69 | 70 | 71 => {
            opcodes_0_71::lookup(command_id)
        }
        80 | 88 | 89 | 90 | 91 | 92 | 93 | 94 | 95 | 96 | 97 | 98 | 99 | 100 | 101 | 102 | 103
        | 104 | 105 | 106 | 128 | 129 | 135 | 136 | 137 => opcodes_80_137::lookup(command_id),
        138 | 139 | 143 | 144 | 145 | 146 | 147 | 148 | 152 | 153 | 156 | 160 | 161 | 162 | 164
        | 176 | 177 | 179 | 200 | 202 | 203 | 212 | 248 | 276 | 277 | 278 | 279 | 286 | 294
        | 296 | 304 | 321 | 322 => opcodes_138_322::lookup(command_id),
        323 | 324 | 325 | 326 | 327 | 328 | 329 | 330 | 331 | 333 | 334 | 336 | 337 | 346 | 347
        | 349 | 350 | 351 | 352 | 353 | 354 => opcodes_323_354::lookup(command_id),
        368 | 376 | 377 | 378 | 379 | 380 | 382 | 383 | 384 | 385 | 386 | 387 | 388 | 389 | 390
        | 391 | 397 | 400 | 401 => opcodes_368_401::lookup(command_id),
        402 | 403 | 404 | 405 | 406 | 410 | 416 | 424 | 425 | 426 | 427 | 428 | 429 | 430 | 431
        | 432 | 433 | 463 | 464 | 465 | 466 | 468 | 469 | 470 | 471 | 524 | 528 | 529 => {
            opcodes_402_529::lookup(command_id)
        }
        530 | 531 | 532 | 533 | 534 | 535 | 544 | 545 | 548 | 549 | 550 | 551 | 552 | 553 | 554
        | 555 | 592 | 688 | 691 | 692 | 695 | 697 | 715 | 717 | 718 | 721 | 741 => {
            opcodes_530_741::lookup(command_id)
        }
        747 | 750 | 751 | 778 | 845 | 846 | 848 | 849 | 850 | 851 | 853 | 854 | 855 | 856 | 857
        | 858 | 859 | 864 | 866 | 868 | 869 | 870 | 940 => opcodes_747_940::lookup(command_id),
        _ => None,
    }?;
    Some(CmvsPs2aCommandContract {
        command_id,
        stack_pop_bytes,
        effect_kind,
        stack_words: stack_words.to_vec(),
    })
}

#[cfg(test)]
mod tests;
