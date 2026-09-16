use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_479400` reads two words: with a non-zero selector it
        // stores the second word as the boolean flag at byte offset 1464,
        // otherwise it copies that flag into byte offset 81220. Returns
        // 0x4008.
        747 => (
            8,
            CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip {
                flag_field_offset: 1464,
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
        // Dispatcher case 750 writes one to byte offset 81220 when the
        // interpreter word at dword index 399 (byte offset 1596) is
        // non-zero, and zero otherwise. Returns 0x4000.
        750 => (
            0,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1596,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4793B0` (case 751): the texture-readiness poll. The
        // original stores `(manager 293 != 0 || manager 296 != 0)` while the
        // override field at byte offset 1464 is zero and forces success
        // otherwise; the headless texture pipeline decodes synchronously, so
        // the canonical readiness value is always success. Returns 0x4000.
        751 => (
            0,
            CmvsPs2aCommandEffectKind::StoreTextureReadyFlag {
                result_field_offset: 81220,
                override_field_offset: 1464,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C920` pops one selector and frees the object in
        // that slot of the three-entry table at dword index 796; selectors
        // above two raise the 0x1000000 mask at byte offset 10532. Returns
        // 0x4004.
        778 => (
            4,
            CmvsPs2aCommandEffectKind::DestroyBoundedSlotObject {
                table_dword_index: 796,
                max_slot: 2,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0100_0000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 845 invokes `sub_45CBC0` on `this+0xCC8`; that
        // method writes one to its field at byte offset 0x568.
        845 => (
            0,
            CmvsPs2aCommandEffectKind::StoreComponentConstant {
                component_offset: 0x0cc8,
                field_offset: 0x0568,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479C90` (case 846) writes `sub_45CBB0`'s value from
        // the input-manager field at byte offset 1388 into the result field
        // at 81220. The headless host has no auxiliary input device, so the
        // canonical value is zero. Returns 0x4000.
        846 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480000` forwards `top != 0` to the settings setter
        // `sub_45C2C0`, which resets the recovered settings fields and
        // stores the boolean at field offset 0. Returns 0x4004.
        848 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 0,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FC50` forwards `top != 0` and `second != 0` to
        // `sub_45AEA0`, which resets the recovered settings fields and
        // stores the booleans at field offsets 24 and 28. Returns 0x4008.
        849 => (
            8,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBooleanPair {
                first_field_offset: 24,
                second_field_offset: 28,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FC20` forwards `top != 0` to the settings setter
        // `sub_45AE80`, which resets the recovered settings fields and
        // stores the boolean at field offset 0. Returns 0x4004.
        850 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 0,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FDC0` copies settings field 44 (`sub_45A0E0`) into
        // the interpreter word at byte offset 81220 without a stack pop.
        // Returns 0x4000.
        851 => (
            0,
            CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord {
                settings_field_offset: 44,
                target_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FBE0` reads the settings field selected by the top
        // stack word through `sub_45C300` (negative indices fall back to
        // field 0, indices above five resolve to zero) and stores its
        // truthiness at byte offset 81220. Returns 0x4004.
        853 => (
            4,
            CmvsPs2aCommandEffectKind::StoreIndexedSettingBoolean {
                target_field_offset: 81220,
                max_index: 5,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FCE0` forwards `top != 0` to `sub_45AF00`
        // (reset + store at field offset 12). Returns 0x4004.
        854 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 12,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FC80` forwards `top != 0` to `sub_45AEC0`
        // (reset + store at field offset 16). Returns 0x4004.
        855 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 16,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FCB0` forwards `top != 0` to `sub_45AEE0`
        // (reset + store at field offset 20). Returns 0x4004.
        856 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 20,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FEC0` derives a result through `sub_45C390` from
        // settings fields 72/76/80 and stores the result at byte offset
        // 81220 and the second selector's truthiness at 81236, without a
        // stack pop. Returns 0x4000.
        857 => (
            0,
            CmvsPs2aCommandEffectKind::StoreDerivedSettingsPair {
                first_selector_field_offset: 72,
                second_selector_field_offset: 76,
                result_field_offset: 80,
                result_target_field_offset: 81220,
                flag_target_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF00` forwards the raw top word to `sub_45C430`,
        // which stores it at field offset 36 only when it lies in 1..=90.
        // No reset path. Returns 0x4004.
        858 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoundedWord {
                setting_field_offset: 36,
                minimum: 1,
                maximum: 90,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF30` forwards the raw top word to `sub_45C450`,
        // which stores it at field offset 32 only when non-negative.
        // No reset path. Returns 0x4004.
        859 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingNonNegativeWord {
                setting_field_offset: 32,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FE20` copies settings field 88 (`sub_45C360`) into
        // the interpreter word at byte offset 81220 without a stack pop.
        // Returns 0x4000.
        864 => (
            0,
            CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord {
                settings_field_offset: 88,
                target_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FE90` stores `top == 1 && settings[88] >= 0`
        // (`sub_45C370`, signed compare) at byte offset 81220. Returns
        // 0x4004.
        866 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSettingsPredicate {
                settings_field_offset: 88,
                target_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF60` forwards the top stack word to the settings
        // setter `sub_45C470`, which resets the recovered settings fields
        // and stores the word at field offset 4. Returns 0x4004.
        868 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSettingsWordReset {
                setting_field_offset: 4,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FE40` derives the case-869 snapshot through
        // `sub_45C3E0`: settings gate field 120 copies fields 100/104/124/128
        // into interpreter words 81236/81240/81224/81228; settings field 176
        // is always copied to 81244, and the gate's truthiness is stored at
        // 81220. No stack pop. Returns 0x4000.
        869 => (
            0,
            CmvsPs2aCommandEffectKind::DeriveSettingsSnapshot {
                gate_field_offset: 120,
                copy_source_field_offsets: [100, 104, 124, 128],
                copy_target_field_offsets: [81236, 81240, 81224, 81228],
                always_source_field_offset: 176,
                always_target_field_offset: 81244,
                gate_target_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF90` forwards six stack words to `sub_45B260`,
        // which bounds the top index to five and stores the remaining five
        // words as a 20-byte record at settings field 188 + 20*index.
        // Returns 0x4018.
        870 => (
            24,
            CmvsPs2aCommandEffectKind::StoreSettingsRecord {
                record_base_field_offset: 188,
                record_stride_bytes: 20,
                max_index: 5,
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
        // Handler `sub_487160` forwards `stack_top != 0` to `sub_41A0D0`.
        // The setter writes `dword_4F0580`; the handler returns 0x4004.
        940 => (
            4,
            CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean {
                address: 0x004f_0580,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
