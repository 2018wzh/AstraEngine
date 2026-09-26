use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeTextOperation {
    Create,
    Destroy,
    Position,
    Rect,
    Clear,
    Metrics,
    Font,
    Colors,
    Kinsoku,
    DrawStatic,
    Cursor,
    ColorMode,
}

const fn word(offset: u8, string: bool) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind: if string {
            CmvsPs2aCommandStackWordKind::TaggedStringReference
        } else {
            CmvsPs2aCommandStackWordKind::OpaqueU32
        },
    }
}

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    use CmvsNativeTextOperation::*;
    const WORDS: [CmvsPs2aCommandStackWord; 5] = [
        word(4, false),
        word(8, false),
        word(12, false),
        word(16, false),
        word(20, false),
    ];
    const STRINGS: [CmvsPs2aCommandStackWord; 3] = [word(4, false), word(8, true), word(12, false)];
    let (operation, bytes, words): (_, _, &'static [_]) = match command_id {
        256 => (Create, 12, &STRINGS),
        257 => (Destroy, 4, &WORDS[..1]),
        258 => (Position, 12, &WORDS[..3]),
        259 => (Rect, 20, &WORDS),
        260 => (Clear, 4, &WORDS[..1]),
        261 => (Metrics, 16, &WORDS[..4]),
        262 => (Font, 8, &STRINGS[..2]),
        263 => (Colors, 12, &WORDS[..3]),
        264 => (Kinsoku, 8, &WORDS[..2]),
        267 => (DrawStatic, 8, &STRINGS[..2]),
        268 => (Cursor, 12, &WORDS[..3]),
        275 => (ColorMode, 8, &WORDS[..2]),
        _ => return None,
    };
    Some((
        bytes,
        CmvsPs2aCommandEffectKind::NativeText { operation },
        words,
    ))
}
