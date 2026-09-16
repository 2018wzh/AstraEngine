use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_47E690` consumes a channel and a string reference,
        // starting the named sound through the channel table entry; a failed
        // start raises 0x80 and an out-of-range or unoccupied channel raises
        // 0x10000, both at byte offset 10532. Returns 0x4008.
        368 => (
            8,
            CmvsPs2aCommandEffectKind::PlayChannelSound {
                channel_table_dword_index: 2968,
                max_channel: 7,
                play_error_mask: 0x80,
                channel_error_mask: 0x0001_0000,
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
        // Handler `sub_47D770` consumes a channel and one opaque word,
        // forwarding the word to the occupied channel's effect object;
        // otherwise it raises 0x10000. Returns 0x4008.
        376 => (
            8,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelBlock {
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
        // Handler `sub_47D7B0` consumes a channel and one opaque word,
        // forwarding the word to the effect engine while the channel slot is
        // occupied; otherwise it raises 0x10000. Returns 0x4008.
        377 => (
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
        // Handler `sub_47D850` consumes a channel, a child selector and a
        // boolean, forwarding the boolean to the selected child of the
        // occupied channel; otherwise it raises 0x10000. Returns 0x400c.
        378 => (
            12,
            CmvsPs2aCommandEffectKind::SetEffectElementVisible {
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
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D630` (case 379): consumes a channel and a child
        // selector and stores whether the element exists at byte offset
        // 81220. Returns 0x4008.
        379 => (
            8,
            CmvsPs2aCommandEffectKind::QueryEffectElementExists {
                error_mask: 0x0001_0000,
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
        // Handler `sub_47D7F0` (case 380) -> `sub_445E80`: consumes a channel,
        // a child selector and two size values. Returns 0x4010.
        380 => (
            16,
            CmvsPs2aCommandEffectKind::SetEffectElementSize {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D590` (case 382) -> `sub_445E40`: consumes a channel
        // and a child selector. Returns 0x4008.
        382 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary {
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
        // Handler `sub_47D5E0` consumes a channel and a child selector and
        // runs the selected child of the occupied channel; otherwise it
        // raises 0x10000. Returns 0x4008.
        383 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectElementAuxiliaryPair {
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        384 => (
            8,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 8 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        385 => (
            8,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 8 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        386 => (
            24,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 24 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        387 => (
            16,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 16 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        388 => (
            12,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 12 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        389 => (
            16,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 16 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        390 => (
            12,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 12 },
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
        // Handler group `sub_47E2xx`/`sub_47E4xx`/`sub_47E630`: channel +
        // child selector (+ opaque action words) on the occupied channel.
        // Returns 0x4000 | pop bytes.
        391 => (
            12,
            CmvsPs2aCommandEffectKind::EffectChildCommand { pop_bytes: 12 },
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
        // Handler `sub_47E300` (case 397) consumes a channel and a quad
        // index and copies the quad sub-object's fields 17/18/19/8/9/10
        // into result fields 81220/81224/81228/81232/81252/81256. The
        // recovered headless graph has no animated quad state, so the six
        // fields are canonical zeros. Returns 0x4008.
        397 => (
            8,
            CmvsPs2aCommandEffectKind::QueryEffectQuadState {
                effect_table_dword_index: 742,
                empty_error_mask: 0x0000_0010,
                missing_error_mask: 0x0001_0000,
                result0_field_offset: 81220,
                result1_field_offset: 81224,
                result2_field_offset: 81228,
                result3_field_offset: 81232,
                result8_field_offset: 81252,
                result9_field_offset: 81256,
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
        // Handler `sub_47F400` (case 400) -> `sub_466CB0`: consumes a channel,
        // a quad selector below 32, a geometry sub-selector and six
        // `__int16` geometry values, storing them at word
        // `38*quad + 6*sub + 22` of the channel object. Channel faults and
        // selector overflow raise 0x20000 at byte offset 10532. Returns
        // 0x4024.
        400 => (
            36,
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F530` (case 401) -> `sub_466D10`: consumes a channel,
        // a quad selector below 32 and four `__int16` hit-rectangle words,
        // storing them at word `38*quad + 52` of the channel object. Faults
        // raise 0x20000. Returns 0x4018.
        401 => (
            24,
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect {
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
        _ => return None,
    })
}
