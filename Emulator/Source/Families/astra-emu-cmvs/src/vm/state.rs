use super::*;

/// Serializable, payload-free state for the recovered PS2A execution subset.
/// A string reference is only a numeric offset/tag; script bytes remain owned
/// by the active VFS-backed `CmvsScript` and are never copied here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsPs2aVmState {
    /// An instruction failed or unwound after beginning execution. Only a new
    /// machine or a validated successful snapshot may resume execution.
    pub(super) execution_failed: bool,
    pub program_counter: u32,
    pub current_value: Option<u32>,
    pub condition_flag: bool,
    /// Backing bytes of the original zero-initialized interpreter variable
    /// buffer. `stack_cursor_bytes` is the logical stack pointer; this vector
    /// can extend beyond it when a positive frame-local write addresses the
    /// current call frame without moving the stack pointer.
    pub stack_bytes: Vec<u8>,
    pub stack_initialized: Vec<bool>,
    pub stack_cursor_bytes: u32,
    /// Absolute byte bases indexed by the recovered intra-script call depth.
    /// Depth zero is the entry frame. Calls append the stack pointer after the
    /// return address has been pushed; returns remove exactly one base.
    pub call_frame_bases: Vec<u32>,
    /// Version-pinned words copied by recovered command handlers. Keys are
    /// original interpreter object byte offsets, not script-visible
    /// variables.
    pub interpreter_words: BTreeMap<u32, u32>,
    /// Version-pinned words assigned through recovered CMVS process-global
    /// setters. Keys are original addresses, not game-visible variable ids.
    pub process_global_words: BTreeMap<u32, u32>,
    /// Opaque flag ids mutated through the recovered bitmap helper.
    pub process_flag_bits: BTreeSet<u32>,
    /// Opaque indexed words mutated through the recovered range helper.
    pub process_indexed_words: BTreeMap<u32, u32>,
    /// Opaque raw f32 bit-patterns mutated through the recovered float table.
    pub process_float_words: BTreeMap<u32, u32>,
    /// Private script references copied by the recovered process string table.
    pub process_string_slots: BTreeMap<u32, CmvsPs2aPrivateStringReference>,
    /// Opaque component fields keyed by packed parent-component and field
    /// offsets from the recovered CMVS 3.90 object graph.
    pub component_words: BTreeMap<u32, u32>,
    /// Ordered, payload-free segments of interpreter-owned string buffers
    /// written by recovered command handlers. Keys are the original buffer
    /// byte offsets; interpreter prefix contents stay unnamed fields.
    pub interpreter_string_buffers: BTreeMap<u16, Vec<CmvsStringSegment>>,
    /// Private resource references held by the recovered channel setup
    /// path, keyed by recovered bank and channel index.
    pub resource_channels:
        BTreeMap<CmvsResourceChannelBank, BTreeMap<u8, CmvsPs2aPrivateStringReference>>,
    /// Opaque object identities held in recovered interpreter slot tables.
    /// Keys pack the version-pinned table byte offset and slot index; values
    /// are deterministic identities assigned by the creation order.
    pub slot_objects: BTreeMap<u32, u64>,
    /// Recovered effect-channel objects, keyed by channel 0..=7. Cases
    /// 400/401/402/403/405/406 read and write the quad records inside these
    /// objects at the original byte offsets; case 405 reports the inverse of
    /// the quad record's visibility dword.
    pub effect_channels: BTreeMap<u8, CmvsEffectChannel>,
    /// Recovered effect playback table entries, keyed by effect index 0..=11.
    /// Cases 276/277/278 write the record fields at dwords 41/44/45.
    pub effect_playback: BTreeMap<u8, CmvsEffectPlaybackRecord>,
    /// Recovered effect channel child elements, keyed by `(channel, child)`.
    /// Cases 378/379/380/382/383 address one element through `sub_466AE0`.
    pub effect_elements: BTreeMap<(u8, u32), CmvsEffectElement>,
    /// Recovered renderer screen-commit flag (`renderer[21]`), committed by
    /// case 104 and polled by cases 105/106. The headless texture pipeline is
    /// synchronous and the host owns input pacing, so the recovered subset
    /// commits on the first poll instead of blocking the frame.
    pub screen_pending: bool,
    /// Recovered renderer screen-object dwords, keyed by the original field
    /// index. Cases 88/89/90/91 write the RGB (1..3), offset (6..8), scale
    /// (9/10/15/16) and rotation (14) fields; unwritten fields read zero.
    pub screen_words: BTreeMap<u32, u32>,
    /// Persisted dwords of the recovered scene object (`game[818]`, byte
    /// offset 3272). The `424`/`426`/`428`/`430`/`432` queries publish
    /// `(base != 0, base + 1 != 0)` for the five layer groups at base words
    /// `289`/`298`/`301`/`304`/`307`, and the matching `425`/`427`/`429`/
    /// `431`/`433` clear base and `base + 2`. Unwritten words read zero, like
    /// the original zero-initialized object.
    pub scene_words: BTreeMap<u32, u32>,
    /// The recovered case-176 resource channel slot records, keyed by slot
    /// index 0..=5. The fixed 256 and zero fields of the original 52-byte
    /// records are constants and stay out of the snapshot.
    pub resource_channel_slots: BTreeMap<u32, CmvsResourceChannelSlotRecord>,
    /// Specialized state owned by each live case-32 top-level texture
    /// container. Object lifetime remains authoritative in `slot_objects`;
    /// this table retains only the recovered resource/surface fields.
    pub texture_parents: BTreeMap<u8, CmvsTextureParentState>,
    /// Child textures owned by each live top-level container in the case-32
    /// table. Parent slots are bounded to 256 and child ids to 1024 by the
    /// original helpers. Resource content remains in the VFS/script pool;
    /// this snapshot retains only typed presentation state and references.
    pub texture_children: BTreeMap<u8, BTreeMap<u16, CmvsTextureChildState>>,
    /// Live resource slot membership behind the recovered case-321 clear
    /// handler. Slot contents are unrecovered; only occupancy is retained.
    pub resource_slots: BTreeSet<u8>,
    /// Monotonic identity source for recovered slot objects.
    pub next_slot_object_id: u64,
    /// The opaque construction words of recovered singleton slot objects,
    /// keyed by the packed slot-object key. The case-548 handler inspects
    /// only the first word (`sub_458E50` treats a non-zero first word as a
    /// fault); the rest stay opaque.
    pub slot_object_seed_words: BTreeMap<u32, Vec<u32>>,
    /// Keyed opaque records owned by each recovered six-bank filter chain.
    /// The outer key is the bank; inner values are the four words updated by
    /// case 531. Record allocation is bounded independently of host memory.
    pub filter_chain_records: BTreeMap<u8, BTreeMap<u32, CmvsFilterChainRecord>>,
    /// Record ids in the linked-list insertion order recovered from
    /// `sub_468560`; key ordering is not an equivalent interaction order.
    pub filter_chain_record_order: BTreeMap<u8, Vec<u32>>,
    /// The currently highlighted record for each interactive chain bank.
    pub filter_chain_active_records: BTreeMap<u8, u32>,
    /// Case-528 channel binding for each live interactive-chain bank. The
    /// original constructor retains the selected object from the bounded
    /// case-32 table (`this+481+channel`); retaining the channel is required
    /// to reproduce case-533 apply and case-535 selection presentation.
    pub filter_chain_channels: BTreeMap<u8, u8>,
    /// Monotonic apply revision per filter-chain bank. The original walks the
    /// complete queued list at command 533; retaining the boundary makes the
    /// side effect replayable without inventing meanings for opaque fields.
    pub filter_chain_apply_revisions: BTreeMap<u8, u64>,
    /// The deterministic PRNG state behind the recovered case-212 `rand()`
    /// draw. The original uses the C runtime generator without an explicit
    /// seed in the boot path; the retained state keeps save/restore and
    /// replay reproducible.
    pub prng_state: u32,
    /// Deterministic session clock in milliseconds. The runtime host derives
    /// it from the validated fixed tick and delta before dispatch; recovered
    /// handlers that called the original host clock read this value.
    pub session_clock_millis: u32,
    /// Interpreter-owned flag words mutated through recovered OR-setters.
    pub interpreter_flag_words: BTreeMap<u16, u32>,
    /// Words of the recovered global settings object, keyed by version-
    /// pinned byte offsets.
    pub settings_words: BTreeMap<u16, u32>,
    /// Ordered private string references held by recovered string-list
    /// append paths, keyed by version-pinned list ids.
    pub private_string_lists: BTreeMap<u16, Vec<CmvsPs2aPrivateStringReference>>,
    /// The outstanding host storage request, if any. Dispatch stays blocked
    /// until the host resolves it through the runtime provider.
    pub storage_await: Option<CmvsPs2aStorageRequest>,
    /// True while case 548 is waiting for the original confirm/pointer
    /// input latch. The host owns the physical input and resolves it at a
    /// fixed-tick boundary.
    pub filter_graph_input_await: bool,
    /// The bank whose case-535 input poll is waiting on a physical edge.
    pub filter_chain_selection_await: Option<u8>,
    /// The physical control delivered for the pending case-535 poll. The
    /// original poll (`sub_468740`) is synchronous and reads the live input
    /// latches every frame, so the completed control is consumed by the next
    /// poll instead of being applied out of band.
    pub filter_chain_pending_control: Option<String>,
    /// The `(pc, frame)` a filter-graph or storage wait suspended at. The
    /// original frame loop resumes the interrupted handler at the saved
    /// program counter; the recovered per-tick queue instead restores it so
    /// a handler that spans a wait keeps its continuation.
    pub wait_resume: Option<(u32, u16)>,
    /// The applied system-save tables, retained as serializable player
    /// progress state once the host resolves the system-state load.
    pub system_save: Option<crate::CmvsSystemSave>,
    /// The active script frame index; frame 0 is the entry script.
    pub current_frame: u16,
    /// Script identities loaded into call frames, keyed by frame index. The
    /// host re-reads and re-validates each script from the mounted VFS on
    /// restore; bytecode itself never enters the snapshot.
    pub script_frames: BTreeMap<u16, crate::CmvsScriptFrameIdentity>,
    /// The PS2A name-index table of each loaded script frame, keyed by
    /// frame index. Entries are absolute program counters (control-flow
    /// data, not payload) needed by the `0x10e` expression read proven by
    /// `sub_46D270`.
    pub script_name_indices: BTreeMap<u16, Vec<u32>>,
    /// The payload-free string-length table of each loaded script frame,
    /// keyed by frame index and pool-relative offset. The recovered
    /// case-248 handler reads only these lengths; string content stays
    /// owned by the active script pool.
    pub frame_string_lengths: BTreeMap<u16, BTreeMap<u32, u32>>,
    /// Byte length of each loaded frame's runtime data segment, the
    /// `sub_4781B0` region published at `this+15100` between the program
    /// section and the string pool. A missing entry blocks segment access;
    /// a zero length makes every access out of bounds, matching a script
    /// without a declared segment.
    pub script_data_segment_sizes: BTreeMap<u16, u32>,
    /// Written dwords of each frame's data segment, keyed by byte offset.
    /// Unwritten words read as their loaded initial value, which is zero
    /// for every recovered case; the provider materializes non-zero
    /// initial dwords at script load.
    pub script_data_segment_words: BTreeMap<u16, BTreeMap<u32, u32>>,
    /// The recovered ephemeral message buffer as ordered payload-free
    /// segments, mirroring the original interpreter buffer that the `0x201`
    /// evaluator resets and assembles. String content stays owned by the
    /// active script pool.
    pub message_buffer_segments: Vec<CmvsMessageBufferSegment>,
    /// The recovered screenshot owner at the version-pinned handle field
    /// (byte offset 3256), absent until case 549 recreates it.
    pub save_image_owner: Option<CmvsSaveImageOwnerState>,
    /// The last streamed pointer position in stage coordinates. The original
    /// input-manager object stores it at fields 180/181 every frame
    /// (`sub_461630`) and the hit-test commands read it through `sub_45CF40`.
    pub input_confirm_held: bool,
    pub input_advance_release: bool,
    pub input_advance_press: bool,
    pub pointer_x: i32,
    pub pointer_y: i32,
    /// The bounded global string-slot table the message buffer materializes
    /// into, keyed by slot index (`sub_48AC90` bounds indices to 0x7f).
    pub message_string_slots: BTreeMap<u32, Vec<CmvsMessageBufferSegment>>,
    pub dispatch_stopped: bool,
}

impl CmvsPs2aVmState {
    pub fn new(program_counter: u32) -> Self {
        Self {
            program_counter,
            current_value: None,
            condition_flag: false,
            stack_bytes: Vec::new(),
            stack_initialized: Vec::new(),
            stack_cursor_bytes: 0,
            call_frame_bases: vec![0],
            interpreter_words: BTreeMap::from([(1452, 1), (1464, 0)]),
            process_global_words: BTreeMap::new(),
            process_flag_bits: BTreeSet::new(),
            process_indexed_words: BTreeMap::new(),
            process_float_words: BTreeMap::new(),
            process_string_slots: BTreeMap::new(),
            component_words: BTreeMap::new(),
            interpreter_string_buffers: BTreeMap::new(),
            resource_channels: BTreeMap::new(),
            slot_objects: BTreeMap::new(),
            effect_channels: BTreeMap::new(),
            effect_playback: BTreeMap::new(),
            effect_elements: BTreeMap::new(),
            screen_pending: false,
            screen_words: BTreeMap::new(),
            scene_words: BTreeMap::new(),
            resource_channel_slots: BTreeMap::new(),
            texture_parents: BTreeMap::new(),
            texture_children: BTreeMap::new(),
            resource_slots: BTreeSet::new(),
            next_slot_object_id: 1,
            slot_object_seed_words: BTreeMap::new(),
            filter_chain_records: BTreeMap::new(),
            filter_chain_record_order: BTreeMap::new(),
            filter_chain_active_records: BTreeMap::new(),
            filter_chain_channels: BTreeMap::new(),
            filter_chain_apply_revisions: BTreeMap::new(),
            prng_state: 1,
            session_clock_millis: 0,
            interpreter_flag_words: BTreeMap::new(),
            settings_words: BTreeMap::new(),
            private_string_lists: BTreeMap::new(),
            storage_await: None,
            filter_graph_input_await: false,
            filter_chain_selection_await: None,
            filter_chain_pending_control: None,
            wait_resume: None,
            system_save: None,
            current_frame: 0,
            script_frames: BTreeMap::new(),
            script_name_indices: BTreeMap::new(),
            frame_string_lengths: BTreeMap::new(),
            script_data_segment_sizes: BTreeMap::new(),
            script_data_segment_words: BTreeMap::new(),
            message_buffer_segments: Vec::new(),
            message_string_slots: BTreeMap::new(),
            save_image_owner: None,
            input_confirm_held: false,
            input_advance_release: false,
            input_advance_press: false,
            pointer_x: 0,
            pointer_y: 0,
            dispatch_stopped: false,
            execution_failed: false,
        }
    }
}
