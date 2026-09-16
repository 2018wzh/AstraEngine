use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Dispatcher case 0 returns 0xc000. The high bit terminates its
        // current loop, while the 0x4000 bit advances the opcode by two.
        0 => (
            0,
            CmvsPs2aCommandEffectKind::StopDispatch,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478B50` zeroes the interpreter word selected by the
        // stack-top dword index into `this+3307` (the recovered 10-word
        // wait-state family) and returns 0x4004.
        11 => (
            4,
            CmvsPs2aCommandEffectKind::ClearInterpreterTableWord {
                table_base_offset: 13228,
                table_word_count: 10,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478B80` copies the table word selected by the
        // stack-top dword index into `this+20305` (byte offset 81220) and
        // returns 0x4004.
        12 => (
            4,
            CmvsPs2aCommandEffectKind::LoadInterpreterTableWord {
                table_base_offset: 13228,
                table_word_count: 10,
                field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handlers `sub_47BC30`, `sub_47BED0`, `sub_47B980` and `sub_47BDA0`
        // all forward the stack-top channel and the resolved stack/8 string
        // (prefixed through the recovered buffer helper) to `sub_48AE50`,
        // which rejects channels above four and otherwise rebuilds the
        // five-slot channel record. All return 0x4008.
        16 | 18 | 19 | 20 => (
            8,
            CmvsPs2aCommandEffectKind::StartResourceChannel {
                bank: CmvsResourceChannelBank::FiveSlotSetup,
            },
            &RESOURCE_CHANNEL_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handlers `sub_47B720` and `sub_47B5F0` forward the same
        // channel/string pair to `sub_4405F0`, whose bounds check rejects
        // channels at or above ten. Both return 0x4008.
        17 | 23 => (
            8,
            CmvsPs2aCommandEffectKind::StartResourceChannel {
                bank: CmvsResourceChannelBank::TenSlotSetup,
            },
            &RESOURCE_CHANNEL_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BCB0` resolves the top stack word through the
        // tagged-string resolver, copies the interpreter prefix field at
        // byte offset 10764 into the string buffer at byte offset 3356,
        // appends the resolved string, and returns 0x4004. Both segments
        // are retained payload-free; the buffer's consumers are not yet
        // recovered.
        21 => (
            4,
            CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset: 3356,
                prefix_field_offset: 10764,
                ensure_directories: false,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BB30` behaves like case 21 for the buffer at byte
        // offset 5404 with the prefix field at byte offset 7452, then walks
        // the assembled path creating missing directories, and returns
        // 0x4004.
        22 => (
            4,
            CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset: 5404,
                prefix_field_offset: 7452,
                ensure_directories: true,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BAB0` forwards the same pair to `sub_4331B0`,
        // whose bounds check rejects channels above six. Returns 0x4008.
        24 => (
            8,
            CmvsPs2aCommandEffectKind::StartResourceChannel {
                bank: CmvsResourceChannelBank::SevenSlotSetup,
            },
            &RESOURCE_CHANNEL_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B850` forwards the same pair to `sub_441A70`,
        // whose bounds check rejects channels above four. Returns 0x4008.
        26 => (
            8,
            CmvsPs2aCommandEffectKind::StartResourceChannel {
                bank: CmvsResourceChannelBank::FiveSlotAltSetup,
            },
            &RESOURCE_CHANNEL_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C270` copies the top stack word into `this+400`
        // (dword index; byte offset 1600) and returns 0x4004. Byte offset
        // 1600 is the interpreter identity field the system-save loader
        // (`sub_427B30`) compares against the container identity word.
        29 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset: 1600 },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487190` copies the top stack word into `this+2678`
        // (dword index; byte offset 10712) and returns 0x4004. The
        // destination field has no recovered public behavior, so retain it
        // as a version-pinned opaque interpreter word.
        30 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10712,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487140` forwards the top stack word to `sub_41A310`.
        // That setter writes `dword_4F0578` and the handler returns 0x4004.
        31 => (
            4,
            CmvsPs2aCommandEffectKind::StoreProcessGlobalWord {
                address: 0x004f_0578,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4807E0` reads the top slot index, rejects indices at
        // or above 256 by OR-ing 0x10 into `this+10532`, and otherwise
        // replaces the slot's object with a fresh 0x3088-byte instance.
        // Returns 0x4004.
        32 => (
            4,
            CmvsPs2aCommandEffectKind::CreateSlotObject {
                table_byte_offset: 1924,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4808A0` bounds-checks the top slot index the same way
        // and destroys the object held there. Returns 0x4004.
        33 => (
            4,
            CmvsPs2aCommandEffectKind::DestroySlotObject {
                table_byte_offset: 1924,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4805E0` selects the top-level texture container with
        // the stack-top word and recreates the child selected by word two.
        34 => (
            8,
            CmvsPs2aCommandEffectKind::ResetTextureChild {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 0x3ff,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480620` (case 35): the stack top selects a script
        // slot (occupancy required, else the error mask) and the next word
        // selects the renderer viewport. Nothing is observable headless.
        // Returns 0x4008.
        35 => (
            8,
            CmvsPs2aCommandEffectKind::SelectRenderViewport {
                slot_table_dword_index: 1924,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480560` (case 39): the stack top selects a script
        // slot and the next word is a selector; the slot occupancy lands in
        // the result field. An out-of-range slot raises the error mask.
        // Returns 0x4008.
        39 => (
            8,
            CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy {
                slot_table_dword_index: 1924,
                max_slot: 255,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
                result_field_offset: 81220,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_481C30` reads destination/source slots at top/8,
        // rejects either index at or above 256 via the same error flag, and
        // otherwise moves the source object into the destination slot.
        // Returns 0x4008.
        40 => (
            8,
            CmvsPs2aCommandEffectKind::MoveSlotObject {
                table_byte_offset: 1924,
            },
            &SLOT_PAIR_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4804C0` (case 46): the stack top selects a script
        // slot and the next word optionally names a sub-object; the chosen
        // slot becomes the presentation target. Out-of-range slots raise
        // the error mask. Returns 0x4008.
        46 => (
            8,
            CmvsPs2aCommandEffectKind::SelectPresentationSlot {
                slot_table_dword_index: 1924,
                max_slot: 255,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4803D0` tears down every live entry of the
        // 256-slot texture channel table at byte offset 1924 without reading
        // the stack. Returns 0x4000.
        47 => (
            0,
            CmvsPs2aCommandEffectKind::ClearTextureChannelTable {
                channel_table_base_offset: 1924,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483D70` resolves stack/8 as the parent PB resource,
        // selects the stack-top container and replaces its resource loader.
        // The loader success flag is stored at byte offset 81220.
        48 => (
            8,
            CmvsPs2aCommandEffectKind::LoadTextureParentResource {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                result_field_offset: 81220,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484020` (case 51): reads one script slot selector and
        // stores the slot's presentation state into the interpreter words at
        // byte offsets 81220/81224/81228/81232. An unoccupied slot raises
        // 0x10 at byte offset 10532. Returns 0x4004.
        51 => (
            4,
            CmvsPs2aCommandEffectKind::StoreScriptSlotPresentationState {
                slot_table_dword_index: 1924,
                error_mask: 0x10,
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480700` resolves the third stack word as the
        // resource name and plays it through the stack-top channel when the
        // channel table entry is live; otherwise it only raises the error
        // flag at byte offset 10532. Returns 0x400c.
        56 => (
            12,
            CmvsPs2aCommandEffectKind::LoadTextureChildResource {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 0x3ff,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482F20` recreates the top-level texture surface owner
        // selected by the stack-top parent id.
        64 => (
            4,
            CmvsPs2aCommandEffectKind::InitializeTextureParentSurface {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4838D0` consumes a parent, a signed surface selector
        // and four opaque transform words. A negative selector retains the
        // parent surface; non-negative selectors address one child surface.
        // Returns 0x4018.
        68 => (
            24,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildRect {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 20,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 24,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483A00` writes a distinct two-word field pair through
        // `sub_446B90`; the signed selector contract matches case 68.
        69 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483600` is the four-word sibling of case 68 and writes
        // the second recovered pair on the selected surface.
        70 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483B10` writes one additional opaque surface word
        // through `sub_446BE0`; the signed selector contract matches case 68.
        71 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
