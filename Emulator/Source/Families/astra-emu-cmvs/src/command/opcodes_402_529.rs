use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_47F5F0` (case 402) -> `sub_466D50`: shows one of 32
        // quads on the occupied channel by setting the quad record's
        // visibility flag and the element's visible flag. Returns 0x4008.
        402 => (
            8,
            CmvsPs2aCommandEffectKind::SelectEffectQuad {
                error_mask: 0x0002_0000,
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
        // Handler `sub_47F640` (case 403) -> `sub_466D90`: hides one of 32
        // quads on the occupied channel by clearing the quad record's
        // visibility flag and the element's visible flag. Returns 0x4008.
        403 => (
            8,
            CmvsPs2aCommandEffectKind::DeselectEffectQuad {
                error_mask: 0x0002_0000,
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
        // Handler `sub_47F4D0` (case 404): queries three state words from
        // the effect engine into byte offsets 81236/81240/81244. The
        // headless engine holds no animated state, so they stay zero.
        // Returns 0x4004.
        404 => (
            4,
            CmvsPs2aCommandEffectKind::QueryEffectState {
                first_field_offset: 81236,
                second_field_offset: 81240,
                third_field_offset: 81244,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F470` (case 405): stores whether the selected quad
        // exists on the occupied channel into byte offset 81220. The
        // headless engine never registers quads, so the canonical result is
        // zero. Returns 0x4008.
        405 => (
            8,
            CmvsPs2aCommandEffectKind::QueryEffectQuadActive {
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
        // Handler `sub_47F330` (case 406) selects/activates one quad slot on        // an occupied effect channel with a value word; the recovered graph
        // keeps only occupancy. Returns 0x400C.
        406 => (
            12,
            CmvsPs2aCommandEffectKind::ActivateEffectQuad {
                error_mask: 0x0002_0000,
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
        // Handler `sub_47ACA0`-adjacent case 410 via `sub_45C5A0`: clears
        // four internal presentation manager counters and re-derives its
        // state; nothing is observable in the headless presentation. Takes
        // no operands and returns 0x4000.
        410 => (
            0,
            CmvsPs2aCommandEffectKind::ResetPresentationBuffers,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479C20` (case 416): copies the texture manager
        // status pair (`sub_45CCB0` reads manager dwords 271/272) into the
        // interpreter words at byte offsets 81220/81236 and takes no stack
        // operands. Returns 0x4000.
        416 => (
            0,
            CmvsPs2aCommandEffectKind::QueryTextureManagerState {
                state_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A0B0` (case 424): writes the scene layer group at
        // base word 289 (`sub_45D290` reads words 289/290) into byte offsets
        // 81220 and 81236. Returns 0x4000.
        424 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 289,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4616E0` (case 425): clears scene words 289 and 291.
        // Returns 0x4000.
        425 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 289 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479D70` (case 426): writes the scene layer group at
        // base word 298 (`sub_45CDB0` reads words 298/299) into byte offsets
        // 81220 and 81236. Returns 0x4000.
        426 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 298,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4612B0` (case 427): clears scene words 298 and 300.
        // Returns 0x4000.
        427 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 298 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479E30` (case 428): writes the scene layer group at
        // base word 301 (`sub_45CE10` reads words 301/302). Returns 0x4000.
        428 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 301,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_461310` (case 429): clears scene words 301 and 303.
        // Returns 0x4000.
        429 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 301 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A070` (case 430): writes the scene layer group at
        // base word 304 (`sub_45D270` reads words 304/305) into byte offsets
        // 81220 and 81236. Returns 0x4000.
        430 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 304,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4616C0` (case 431): clears scene words 304 and 306.
        // Returns 0x4000.
        431 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 304 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479FB0` (case 432): writes the scene layer group at
        // base word 307 (`sub_45CFE0` reads words 307/308). Returns 0x4000.
        432 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 307,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4613D0` (case 433): clears scene words 307 and 309.
        // Returns 0x4000.
        433 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 307 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DC80` (case 463): consumes a channel and a child
        // selector and forwards the element's texture object. Returns 0x4008.
        463 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DCE0` (case 464): consumes a channel, a child
        // selector and nine configuration words. Returns 0x402C.
        464 => (
            44,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 44,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DE00` (case 465): consumes a channel, a child selector and 1 configuration word. Returns 0x400C.
        465 => (
            12,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DDA0` (case 466): consumes a channel and a child selector. Returns 0x4008.
        466 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DAC0` (case 468): consumes a channel and a child selector. Returns 0x4008.
        468 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DB20` (case 469): consumes a channel, a child selector and 6 configuration words. Returns 0x4020.
        469 => (
            32,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DC20` (case 470): consumes a channel, a child selector and 1 configuration word. Returns 0x400C.
        470 => (
            12,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DBC0` (case 471): consumes a channel and a child selector. Returns 0x4008.
        471 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47B0B0` writes `owner != 0 && owner->field5 != 0`
        // into the version-pinned result field without touching the stack.
        // Returns 0x4000.
        524 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySaveImageEnabled {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485FD0` replaces the occupant of the six-slot
        // filter-chain table at byte offset 3196, bounded by bank six and
        // channel 256 (0x100000 error mask at byte offset 10532 otherwise).
        // Returns 0x4008.
        528 => (
            8,
            CmvsPs2aCommandEffectKind::ReplaceFilterChainSlot {
                table_byte_offset: 3196,
                max_bank: 5,
                max_channel_id: 255,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486C80` destroys one of the six filter-chain banks at
        // byte offset 3196. Banks above five raise 0x100000 at byte offset
        // 10532; an already-empty valid bank is a no-op. Returns 0x4004.
        529 => (
            4,
            CmvsPs2aCommandEffectKind::DestroyFilterChainSlot {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
