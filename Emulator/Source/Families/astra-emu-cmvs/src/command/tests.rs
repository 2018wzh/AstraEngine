use super::*;

#[test]
fn exposes_resource_channel_aliases_with_the_same_contract() {
    let words = vec![
        CmvsPs2aCommandStackWord {
            offset_from_top_bytes: 4,
            kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
        },
        CmvsPs2aCommandStackWord {
            offset_from_top_bytes: 8,
            kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
        },
    ];
    let expected: [(u16, CmvsResourceChannelBank); 8] = [
        (16, CmvsResourceChannelBank::FiveSlotSetup),
        (17, CmvsResourceChannelBank::TenSlotSetup),
        (18, CmvsResourceChannelBank::FiveSlotSetup),
        (19, CmvsResourceChannelBank::FiveSlotSetup),
        (20, CmvsResourceChannelBank::FiveSlotSetup),
        (23, CmvsResourceChannelBank::TenSlotSetup),
        (24, CmvsResourceChannelBank::SevenSlotSetup),
        (26, CmvsResourceChannelBank::FiveSlotAltSetup),
    ];
    for (command_id, bank) in expected {
        assert_eq!(
            cmvs390_command_contract(command_id),
            Some(CmvsPs2aCommandContract {
                command_id,
                stack_pop_bytes: 8,
                effect_kind: CmvsPs2aCommandEffectKind::StartResourceChannel { bank },
                stack_words: words.clone(),
            }),
            "command {command_id} must match its recovered channel bank"
        );
    }
}

#[test]
fn exposes_only_static_cmvs390_recovered_contracts() {
    assert_eq!(
        cmvs390_command_contract(21),
        Some(CmvsPs2aCommandContract {
            command_id: 21,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset: 3356,
                prefix_field_offset: 10764,
                ensure_directories: false,
            },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(22),
        Some(CmvsPs2aCommandContract {
            command_id: 22,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset: 5404,
                prefix_field_offset: 7452,
                ensure_directories: true,
            },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(29),
        Some(CmvsPs2aCommandContract {
            command_id: 29,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset: 1600 },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(30),
        Some(CmvsPs2aCommandContract {
            command_id: 30,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10712
            },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(31),
        Some(CmvsPs2aCommandContract {
            command_id: 31,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StoreProcessGlobalWord {
                address: 0x004f_0578,
            },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(144),
        Some(CmvsPs2aCommandContract {
            command_id: 144,
            stack_pop_bytes: 0,
            effect_kind: CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 2940,
                value: 1,
            },
            stack_words: Vec::new(),
        })
    );
    assert_eq!(
        cmvs390_command_contract(940),
        Some(CmvsPs2aCommandContract {
            command_id: 940,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean {
                address: 0x004f_0580,
            },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(145),
        Some(CmvsPs2aCommandContract {
            command_id: 145,
            stack_pop_bytes: 12,
            effect_kind: CmvsPs2aCommandEffectKind::MutateProcessFlagRange,
            stack_words: vec![
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
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ],
        })
    );
    assert_eq!(
        cmvs390_command_contract(146),
        Some(CmvsPs2aCommandContract {
            command_id: 146,
            stack_pop_bytes: 12,
            effect_kind: CmvsPs2aCommandEffectKind::StoreProcessIndexedRange,
            stack_words: vec![
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
            ],
        })
    );
    assert_eq!(
        cmvs390_command_contract(147),
        Some(CmvsPs2aCommandContract {
            command_id: 147,
            stack_pop_bytes: 12,
            effect_kind: CmvsPs2aCommandEffectKind::StoreProcessFloatRange,
            stack_words: vec![
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
            ],
        })
    );
    assert_eq!(
        cmvs390_command_contract(148),
        Some(CmvsPs2aCommandContract {
            command_id: 148,
            stack_pop_bytes: 12,
            effect_kind: CmvsPs2aCommandEffectKind::StoreProcessStringRange,
            stack_words: vec![
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
            ],
        })
    );
    assert_eq!(
        cmvs390_command_contract(845),
        Some(CmvsPs2aCommandContract {
            command_id: 845,
            stack_pop_bytes: 0,
            effect_kind: CmvsPs2aCommandEffectKind::StoreComponentConstant {
                component_offset: 0x0cc8,
                field_offset: 0x0568,
                value: 1,
            },
            stack_words: Vec::new(),
        })
    );
    assert_eq!(
        cmvs390_command_contract(160),
        Some(CmvsPs2aCommandContract {
            command_id: 160,
            stack_pop_bytes: 12,
            effect_kind: CmvsPs2aCommandEffectKind::PlayAudio,
            stack_words: vec![
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ],
        })
    );
    assert_eq!(
        cmvs390_command_contract(164),
        Some(CmvsPs2aCommandContract {
            command_id: 164,
            stack_pop_bytes: 16,
            effect_kind: CmvsPs2aCommandEffectKind::PlayPairedAudio,
            stack_words: vec![
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ],
        })
    );
    assert_eq!(
        cmvs390_command_contract(717),
        Some(CmvsPs2aCommandContract {
            command_id: 717,
            stack_pop_bytes: 4,
            effect_kind: CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10568,
            },
            stack_words: vec![CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }],
        })
    );
    assert_eq!(
        cmvs390_command_contract(718),
        Some(CmvsPs2aCommandContract {
            command_id: 718,
            stack_pop_bytes: 0,
            effect_kind: CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 11792,
                result_field_offset: 81220,
            },
            stack_words: vec![],
        })
    );
}
