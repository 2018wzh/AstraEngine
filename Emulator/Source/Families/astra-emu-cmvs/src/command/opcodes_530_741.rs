use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_486BC0` queries the six-slot filter-chain table at
        // byte offset 3196: a missing bank raises the 0x100000 mask, a live
        // bank appends one opaque queue node and stores one at byte offset
        // 81220. Returns 0x4008 either way.
        530 => (
            8,
            CmvsPs2aCommandEffectKind::QueryFilterChainSlot {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
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
        // Handler `sub_486B60` updates four opaque words in the keyed list
        // record of one live filter-chain bank through `sub_468640`. A
        // missing record is ignored; a missing bank raises 0x100000 at the
        // interpreter error field. Returns 0x4018.
        531 => (
            24,
            CmvsPs2aCommandEffectKind::UpdateFilterChainRecord {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
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
        // Handler `sub_486C10` forwards nine record words plus the bank to
        // `sub_468AD0`. The helper updates one keyed parameter block; its
        // layout is selected by process-global backend mode 0x4F057C. A
        // missing bank raises the 0x100000 error mask. Returns 0x4028.
        532 => (
            40,
            CmvsPs2aCommandEffectKind::UpdateFilterChainParameterBlock {
                table_byte_offset: 3196,
                max_bank: 5,
                backend_mode_address: 0x004f_057c,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 32,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 36,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 40,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486D30` validates one of the six filter-chain banks
        // and applies its complete queued record list through `sub_4686A0`.
        // A missing bank raises 0x100000 at the interpreter error field.
        // Returns 0x4004.
        533 => (
            4,
            CmvsPs2aCommandEffectKind::ApplyFilterChainRecords {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486CE0` returns the active record id from the selected
        // bank through `sub_468690`, or -1 when no record is active.
        534 => (
            4,
            CmvsPs2aCommandEffectKind::ReadActiveFilterChainSelection {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
                result_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486A50` validates a six-bank interactive chain and
        // forwards one input poll to `sub_468740`. The helper walks records
        // in insertion order, changes the active record on directional
        // input, and returns the active record id on confirm. Returns 0x4004.
        535 => (
            4,
            CmvsPs2aCommandEffectKind::PollFilterChainSelection {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
                result_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A4F0` consumes four words to rebuild the singleton
        // filter-graph owner behind slot-table byte offset 3252 (replacing
        // any previous occupant) and stores one at byte offset 81220; the
        // consumed words themselves are not inspected. Returns 0x4010.
        544 => (
            16,
            CmvsPs2aCommandEffectKind::CreateSingletonSlotObject {
                table_byte_offset: 3252,
                slot: 0,
                field_offset: 81220,
                value: 1,
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
        // Handler `sub_47A5F0` frees the singleton filter-graph owner at
        // slot-table byte offset 3252 when present. Returns 0x4000.
        545 => (
            0,
            CmvsPs2aCommandEffectKind::DestroySingletonSlotObject {
                table_byte_offset: 3252,
                slot: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A6B0` reads the control word, clears byte offset
        // 81220 and branches on the singleton filter-graph owner created by
        // case 544; the healthy zero-control path returns 0xA000 (no pop),
        // which the nucleus reproduces by restoring the contract pop.
        548 => (
            4,
            CmvsPs2aCommandEffectKind::RunFilterGraphControl {
                table_byte_offset: 3252,
                slot: 0,
                field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47AFF0` frees any previous owner, builds a fresh
        // 0x28-byte owner from the four consumed words (flag and progress
        // fields zeroed by `sub_459BE0`) and publishes the handle.
        // Returns 0x4010.
        549 => (
            16,
            CmvsPs2aCommandEffectKind::RecreateSaveImageOwner {
                handle_field_offset: 3256,
            },
            &FOUR_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47AFC0` writes the owner's remaining progress
        // (`sub_459CC0`) or all-ones when no owner exists into the
        // version-pinned result field. Returns 0x4000.
        550 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySaveImageProgress {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B0F0` stores the consumed boolean into the
        // owner's enabled flag only when the owner exists. Returns 0x4004.
        551 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSaveImageEnabled,
            &OPAQUE_BOOLEAN as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B160` distinguishes a missing owner (writes one,
        // 0x4004), a present owner with a set boolean (writes zero and
        // waits, 0xA000) and a present owner with a clear boolean
        // (destroy, writes one, 0xC004).
        552 => (
            4,
            CmvsPs2aCommandEffectKind::SaveImageOwnerWaitOrDestroy {
                result_field_offset: 81220,
            },
            &OPAQUE_BOOLEAN as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B120` destroys a present owner without reading the
        // stack. Returns 0x4000.
        553 => (
            0,
            CmvsPs2aCommandEffectKind::DestroySaveImageOwner,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47ACA0` (case 554): the stack top selects a parent
        // slot, the next word is the tagged PB resource name and three
        // opaque words follow. The container is created when absent, the
        // previous one is replaced, and the loader success lands at byte
        // offset 81220. Returns 0x4014.
        554 => (
            20,
            CmvsPs2aCommandEffectKind::LoadTextureParentResourceExtended {
                texture_table_base_offset: 1924,
                result_field_offset: 81220,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47AF00` (case 555): the texture presentation commit.
        // Without a live presentation object it stores zero (0x4004); with
        // one and a zero mode it returns 0xA000 (frame wait); otherwise it
        // stores success and returns 0xC004 (show and end the frame).
        555 => (
            4,
            CmvsPs2aCommandEffectKind::CommitTexturePresentation {
                result_field_offset: 81220,
                present_object_field_offset: 3260,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484F90` stores the consumed boolean at field 72 of
        // the settings object reached through `sub_432F10(this+480)`.
        // Returns 0x4004.
        592 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSettingBooleanPlain {
                setting_field_offset: 72,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485A10` stores the top and second stack words into
        // the interpreter fields at byte offsets 10296 and 10300, then
        // writes one or zero to byte offset 10292 depending on whether both
        // are non-zero. Returns 0x4008.
        688 => (
            8,
            CmvsPs2aCommandEffectKind::StorePresentationFrameFields {
                top_field_offset: 10296,
                second_field_offset: 10300,
                active_field_offset: 10292,
            },
            &SLOT_PAIR_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485D40` clamps the signed top word (negative to ten,
        // above 256 to 256), stores it at byte offset 10452, and feeds the
        // recovered window-layout helpers. Returns 0x4004.
        691 => (
            4,
            CmvsPs2aCommandEffectKind::StoreClampedPresentationSize {
                field_offset: 10452,
                negative_replacement: 10,
                maximum: 256,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485DD0` passes `top != 0` with the interpreter
        // fields at byte offsets 1440 and 15064 to the recovered
        // presentation-mode helper. Returns 0x4004.
        692 => (
            4,
            CmvsPs2aCommandEffectKind::TogglePresentationMode {
                first_field_offset: 1440,
                second_field_offset: 15064,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485970` (case 695) scans the save-slot range
        // `[start, start + count)` for the newest `saveNN.dat` and writes
        // its slot index, or -1 when the range is invalid. The headless
        // host exposes no writable save files, so the canonical result is
        // -1. Returns 0x4008.
        695 => (
            8,
            CmvsPs2aCommandEffectKind::QueryNewestSaveSlot {
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
        // Handler `sub_485B90` (case 697): copies the top stack word into
        // the interpreter word at byte offset 10312. Returns 0x4004.
        697 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10312,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A340` stores the top stack word into byte offset
        // 76 of the component referenced by interpreter byte offset 1920;
        // the companion getter result is discarded. Returns 0x4004.
        715 => (
            4,
            CmvsPs2aCommandEffectKind::StoreComponentWord {
                component_field_offset: 1920,
                field_offset: 76,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4846C0` stores `stack_top != 0` into the interpreter
        // word at byte offset 10568 and returns 0x4004.
        717 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10568,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 718 copies the interpreter word at dword index
        // 2948 (byte offset 11792) into the result field at byte offset
        // 81220 without touching the stack. Returns 0x4000.
        718 => (
            0,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 11792,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478EC0` (case 721): the stack top is a config key and
        // the next word its boolean value, stored into the interpreter config
        // word. A zero key instead stores the skip flag into the result.
        // Returns 0x4008.
        721 => (
            8,
            CmvsPs2aCommandEffectKind::StoreSystemConfigBoolean {
                config_field_offset: 1440,
                skip_flag_field_offset: 1452,
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
        // Handler `sub_479270` (case 741): a non-zero selector stores the
        // second word's boolean at byte offset 1500; a zero selector copies
        // that flag back into the result field at 81220. Returns 0x4008.
        741 => (
            8,
            CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip {
                flag_field_offset: 1500,
                result_field_offset: 81220,
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
        _ => return None,
    })
}
