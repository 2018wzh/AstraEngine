use super::*;

pub(super) const AUDIO_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
];
pub(super) const PAIRED_AUDIO_WORDS: [CmvsPs2aCommandStackWord; 4] = [
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
];
pub(super) const OPAQUE_WORD: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
}];
pub(super) const OPAQUE_BOOLEAN: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
}];
pub(super) const FOUR_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 4] = [
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
];
pub(super) const TAGGED_STRING_WORD: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
}];
pub(super) const FLAG_RANGE_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
];
pub(super) const INDEXED_RANGE_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
];
pub(super) const STRING_RANGE_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
];
pub(super) const RESOURCE_CHANNEL_WORDS: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];
pub(super) const SLOT_PAIR_WORDS: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
