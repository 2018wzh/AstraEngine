use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_47CB00` masks the stack-top label with 0x3F, advances
        // the program counter past the two-byte opcode, pushes the frame-
        // counter field, the advanced PC and the current frame index as the
        // nested-call return record, clears the frame-counter field and
        // transfers dispatch to the record's registered frame and PC words
        // (byte offsets 13276/13280 + 28*label). The dispatcher result is 0:
        // the handler pops the label itself and the interpreter loop must
        // not advance the PC again. A record without a registered PC blocks
        // because the original would execute at an unregistered address.
        138 => (
            4,
            CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord {
                record_table_base_offset: 13272,
                record_stride_bytes: 28,
                max_index_mask: 63,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CC80` writes -1 into the first word of the
        // 28-byte record selected by the stack-top index (64 records at
        // byte offset 13280); indices above 63 are ignored. Returns 0x4004.
        139 => (
            4,
            CmvsPs2aCommandEffectKind::MarkInterpreterRecordUnused {
                record_table_base_offset: 13280,
                record_count: 64,
                record_stride_bytes: 28,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C2D0` resolves the top stack word, builds the
        // prefix_10764 + string caption in a local buffer, and passes it to
        // the window-caption setter for the interpreter's main window.
        // Returns 0x4004.
        143 => (
            4,
            CmvsPs2aCommandEffectKind::RequestWindowCaption {
                prefix_field_offset: Some(10764),
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 144 writes one to `this+2940` and returns 0x4000.
        144 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 2940,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486F40` reads start/count/enabled at stack top/8/12,
        // updates each bit through `sub_48ABC0`, and returns 0x400c.
        145 => (
            12,
            CmvsPs2aCommandEffectKind::MutateProcessFlagRange,
            &FLAG_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487000` reads start/count/value at stack top/8/12,
        // writes each index through `sub_48AC50`, and returns 0x400c.
        146 => (
            12,
            CmvsPs2aCommandEffectKind::StoreProcessIndexedRange,
            &INDEXED_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486F80` reads start/count/f32 bits at stack top/8/12,
        // writes each index through `sub_48AC30`, and returns 0x400c.
        147 => (
            12,
            CmvsPs2aCommandEffectKind::StoreProcessFloatRange,
            &INDEXED_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4871B0` resolves the top/12 string, copies it to each
        // stack top/8 range slot through `sub_48AC90`, and returns 0x400c.
        148 => (
            12,
            CmvsPs2aCommandEffectKind::StoreProcessStringRange,
            &STRING_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4802A0` resolves the top stack word, appends it to
        // the recovered case-152 string list through `sub_491470`, and
        // stores the new length at byte offset 81220. Returns 0x4004.
        152 => (
            4,
            CmvsPs2aCommandEffectKind::AppendPrivateStringToList {
                list_id: 152,
                length_field_offset: 81220,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4802E0` passes the top word as a cursor index to the
        // cursor manager referenced by interpreter byte offset 32; the
        // manager skips work when its current-index field (byte offset 4 of
        // the manager object) already matches. Returns 0x4004.
        153 => (
            4,
            CmvsPs2aCommandEffectKind::SelectSystemCursor {
                manager_field_offset: 32,
                current_index_field_offset: 4,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Case 156: the recovered dispatcher body is empty (`LABEL_119` sets
        // the result to 0x4008 and returns); two stack words are popped and
        // never read.
        156 => (
            8,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        160 => (
            12,
            CmvsPs2aCommandEffectKind::PlayAudio,
            &AUDIO_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Stop both alternating audio channels; no stack words.
        161 => (
            0,
            CmvsPs2aCommandEffectKind::StopAudio,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Fade out the active audio channel using the consumed duration.
        162 => (
            4,
            CmvsPs2aCommandEffectKind::FadeOutAudio,
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        164 => (
            16,
            CmvsPs2aCommandEffectKind::PlayPairedAudio,
            &PAIRED_AUDIO_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A110` registers one resource channel slot record
        // (slot/name/enabled/loop/volume) in the fixed table at byte
        // offset 1608 and returns 0x4014. Slots above 5 only raise the
        // error flag at byte offset 10532.
        176 => (
            20,
            CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot {
                max_slot: 5,
                table_base_offset: 1608,
                record_stride_bytes: 52,
                gate_field_offset: 1556,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x200,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 20,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A230` rejects channels above five through the
        // error-flag mask 0x200, otherwise writes `enabled != 0` at byte
        // offset 1640 + 52*channel, one at 1656 + 52*channel, and forwards
        // a notification only when the unrecovered gate field at byte
        // offset 1556 is non-zero. Returns 0x4008.
        177 => (
            8,
            CmvsPs2aCommandEffectKind::StoreChannelVisibilityRecord {
                enabled_field_base_offset: 1640,
                touched_field_base_offset: 1656,
                record_stride_bytes: 52,
                notify_gate_field_offset: 1556,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A2B0` rejects channels above five through the
        // error-flag mask 0x200; otherwise it resets the channel name
        // buffer at 1608 + 52*channel to the interpreter prefix and zeroes
        // the touched word at 1656 + 52*channel. Returns 0x4004.
        179 => (
            4,
            CmvsPs2aCommandEffectKind::ResetChannelVisibilityRecord {
                name_buffer_base_offset: 1608,
                touched_field_base_offset: 1656,
                record_stride_bytes: 52,
                error_flag_mask: 0x200,
                teardown_slot_table_offset: 2368,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480280` reads the input-manager object's persistent
        // pointer fields 180/181 through `sub_45CF40` and writes them into
        // system registers 1 and 2. Returns 0x4000.
        200 => (
            0,
            CmvsPs2aCommandEffectKind::QueryPointerPosition {
                x_field_offset: 81224,
                y_field_offset: 81228,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480060` reads the persistent pointer and tests it
        // against the stack-provided rectangle (x, y, width, height) with
        // inclusive bounds, storing the boolean into the version-pinned
        // result field. Returns 0x4010.
        202 => (
            16,
            CmvsPs2aCommandEffectKind::HitTestPointerRect {
                result_field_offset: 81220,
            },
            &FOUR_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4800F0` builds a GDI region from the stack-provided
        // shape (type, x1, y1, x2, y2, width, height) and tests the
        // persistent pointer through `PtInRegion`, storing the boolean into
        // the version-pinned result field. Only the rectangle type is
        // recovered; the curved and polygon rasterization stays blocking.
        // Returns 0x401C.
        203 => (
            28,
            CmvsPs2aCommandEffectKind::HitTestPointerRegion {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 28,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484F60` stores `rand() % stack_top` into the
        // interpreter word at byte offset 81220 and returns 0x4004.
        212 => (
            4,
            CmvsPs2aCommandEffectKind::StoreRandomModulo {
                target_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D220` stores `lstrlenA(resolved_top)` into the
        // interpreter word at byte offset 81220 and returns 0x4004.
        248 => (
            4,
            CmvsPs2aCommandEffectKind::StoreStringLength {
                target_field_offset: 81220,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488FC0` (case 276) -> `sub_464AA0`: consumes a playback
        // index below 12 and a value, storing the record's dword 41. Faults
        // raise 0x1000. Returns 0x4008.
        276 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 41,
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
        // Handler `sub_488F80` (case 277) -> `sub_464A90`: as case 276 but the
        // record's dword 44. Returns 0x4008.
        277 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 44,
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
        // Handler `sub_488F40` (case 278) -> `sub_464A80`: as case 276 but the
        // record's dword 45. Returns 0x4008.
        278 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 45,
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
        // Handler `sub_488EF0` (case 279): as case 276 but forwards the
        // boolean to `sub_429850`, a graphics-subsystem global. Faults raise
        // 0x1000. Returns 0x4008.
        279 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 46,
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
        // Handler `sub_488E70` (case 286) -> `sub_462C10`: consumes a playback
        // index below 12, two script strings and a value; stores them on the
        // record's text surface. Faults raise 0x1000. Returns 0x4010.
        286 => (
            16,
            CmvsPs2aCommandEffectKind::SetEffectTextSurface {
                error_mask: 0x0000_1000,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A570` publishes the render object's inner
        // display-mode fields 6 and 7 into system registers 1 and 2. The
        // renderer object model is not recovered, so the effect blocks.
        // Returns 0x4000.
        294 => (
            0,
            CmvsPs2aCommandEffectKind::QueryRendererDisplayMode {
                renderer_field_offset: 1920,
                result_field_offset: 81224,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Inline dispatcher case 296 calls `sub_427B30`, which opens
        // `system.dat` under the case-22 path buffer, verifies, decrypts
        // and decompresses it, and restores the recovered system tables.
        // The result depends entirely on host storage, so the nucleus turns
        // it into a storage request and waits for the host. Returns 0x4000.
        296 => (
            0,
            CmvsPs2aCommandEffectKind::RequestSystemStateLoad {
                path_buffer_offset: 5404,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CE30` (case 304) writes the message-window list
        // length from the window manager at byte offset 3096 into the
        // result field at 81220. The host owns message presentation, so the
        // recovered native window list stays empty. Returns 0x4000.
        304 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F8D0` clears the resource slot named by the
        // stack-top index (0..=7) and returns 0x4004; indices at or above
        // 8 raise the 0x10000 error mask at byte offset 10532.
        321 => (
            4,
            CmvsPs2aCommandEffectKind::ClearResourceSlot {
                max_slot: 7,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F830` consumes a channel and one opaque word,
        // forwarding the word to the effect engine only while the channel
        // slot is occupied; otherwise it raises 0x10000. Returns 0x4008.
        322 => (
            8,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index: 742,
                max_channel: 7,
                error_mask: 0x0001_0000,
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
        _ => return None,
    })
}
