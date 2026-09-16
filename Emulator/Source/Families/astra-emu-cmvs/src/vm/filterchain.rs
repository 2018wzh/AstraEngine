use super::*;

/// Rebuilds the per-frame entry queue (`sub_46DF10`).
///
/// At every frame boundary the original scheduler pushes the suspended
/// `(delay counter, pc, frame)` triple back onto the value stack, pushes
/// `(0, entry_pc, entry_frame)` for every active per-frame entry slot, and
/// finally switches `(13216, 13220, 13224)` to the topmost triple, which is
/// the last activated slot. The dispatch loop then consumes queued triples
/// through `0x410`/`0x411`/`0x414` transfers until `command 0` returns
/// `0xC000` and ends the frame. The whole re-queue is gated on the
/// scheduler-enable word that `command 137` sets; the 64 per-frame enable
/// flags (`command 137`'s companion table at the record base) are cleared
/// unconditionally afterwards, so a flag set in one frame never survives
/// into the next.
/// Applies one delivered control to the case-535 filter-chain selection and
/// returns the value the original `sub_468740` poll would report: the active
/// record id on confirm, all-ones while a directional edge only moves the
/// highlight or the poll stays idle, and -2 on escape.
pub(super) fn apply_filter_chain_control(
    state: &mut CmvsPs2aVmState,
    bank: u8,
    control: &str,
) -> u32 {
    let order = state
        .filter_chain_record_order
        .get(&bank)
        .cloned()
        .unwrap_or_default();
    let active = state.filter_chain_active_records.get(&bank).copied();
    match control {
        "arrow_up" | "arrow_left" => {
            if let Some(next) = previous_selection(&order, active) {
                state.filter_chain_active_records.insert(bank, next);
            }
            u32::MAX
        }
        "arrow_down" | "arrow_right" => {
            if let Some(next) = next_selection(&order, active) {
                state.filter_chain_active_records.insert(bank, next);
            }
            u32::MAX
        }
        "enter" | "space" => active.unwrap_or((-10_i32) as u32),
        "escape" => (-2_i32) as u32,
        _ => u32::MAX,
    }
}

pub(super) fn previous_selection(order: &[u32], active: Option<u32>) -> Option<u32> {
    let Some(active) = active else {
        return order.last().copied();
    };
    let index = order.iter().position(|record| *record == active)?;
    index
        .checked_sub(1)
        .and_then(|index| order.get(index).copied())
        .or_else(|| order.last().copied())
}

pub(super) fn next_selection(order: &[u32], active: Option<u32>) -> Option<u32> {
    match active.and_then(|active| order.iter().position(|record| *record == active)) {
        Some(index) => order
            .get(index + 1)
            .copied()
            .or_else(|| order.first().copied()),
        None => order.first().copied(),
    }
}
