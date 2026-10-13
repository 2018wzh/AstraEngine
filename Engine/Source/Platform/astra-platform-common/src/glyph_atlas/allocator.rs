use super::*;
use astra_platform::PlatformError;

pub(crate) fn release_pending_atlas_slot(
    allocator: &mut AtlasAllocatorState<'_>,
    placement: AtlasPlacement,
) -> Result<(), PlatformError> {
    let slot = placement_to_slot(placement)?;
    allocator.freed_area = allocator
        .freed_area
        .checked_add(atlas_slot_area(slot))
        .ok_or_else(|| invalid("glyph atlas freed area overflowed"))?;
    insert_pending_free_slot(allocator, slot);
    Ok(())
}

pub(crate) fn insert_pending_free_slot(
    allocator: &mut AtlasAllocatorState<'_>,
    mut slot: AtlasSlot,
) {
    loop {
        if let Some((index, merged)) = allocator
            .base_free_slots
            .iter()
            .enumerate()
            .filter(|(index, _)| !allocator.consumed_free_slots.contains(index))
            .find_map(|(index, candidate)| {
                merge_adjacent_atlas_slots(slot, *candidate).map(|merged| (index, merged))
            })
        {
            allocator.consumed_free_slots.insert(index);
            slot = merged;
            continue;
        }
        if let Some((index, merged)) =
            allocator
                .added_free_slots
                .iter()
                .enumerate()
                .find_map(|(index, candidate)| {
                    merge_adjacent_atlas_slots(slot, *candidate).map(|merged| (index, merged))
                })
        {
            allocator.added_free_slots.remove(index);
            slot = merged;
            continue;
        }
        break;
    }
    allocator.added_free_slots.push(slot);
    allocator.added_free_slots.sort_by_key(atlas_slot_sort_key);
}

pub(crate) fn merge_adjacent_atlas_slots(left: AtlasSlot, right: AtlasSlot) -> Option<AtlasSlot> {
    if left.y == right.y && left.height == right.height {
        if left.x.checked_add(left.width) == Some(right.x) {
            return Some(AtlasSlot {
                x: left.x,
                y: left.y,
                width: left.width.checked_add(right.width)?,
                height: left.height,
            });
        }
        if right.x.checked_add(right.width) == Some(left.x) {
            return Some(AtlasSlot {
                x: right.x,
                y: right.y,
                width: right.width.checked_add(left.width)?,
                height: right.height,
            });
        }
    }
    if left.x == right.x && left.width == right.width {
        if left.y.checked_add(left.height) == Some(right.y) {
            return Some(AtlasSlot {
                x: left.x,
                y: left.y,
                width: left.width,
                height: left.height.checked_add(right.height)?,
            });
        }
        if right.y.checked_add(right.height) == Some(left.y) {
            return Some(AtlasSlot {
                x: right.x,
                y: right.y,
                width: right.width,
                height: right.height.checked_add(left.height)?,
            });
        }
    }
    None
}

pub(crate) fn allocate_pending_atlas_slot(
    allocator: &mut AtlasAllocatorState<'_>,
    width: u32,
    height: u32,
) -> Result<AtlasPlacement, PlatformError> {
    let required_width = width + ATLAS_PADDING * 2;
    let required_height = height + ATLAS_PADDING * 2;
    let base = allocator
        .base_free_slots
        .iter()
        .enumerate()
        .filter(|(index, slot)| {
            !allocator.consumed_free_slots.contains(index)
                && slot.width >= required_width
                && slot.height >= required_height
        })
        .min_by_key(|(_, slot)| atlas_slot_sort_key(slot))
        .map(|(index, slot)| (index, *slot));
    let added = allocator
        .added_free_slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.width >= required_width && slot.height >= required_height)
        .min_by_key(|(_, slot)| atlas_slot_sort_key(slot))
        .map(|(index, slot)| (index, *slot));
    let selected = match (base, added) {
        (Some((index, slot)), Some((added_index, added_slot))) => {
            if atlas_slot_sort_key(&slot) <= atlas_slot_sort_key(&added_slot) {
                allocator.consumed_free_slots.insert(index);
                slot
            } else {
                allocator.added_free_slots.remove(added_index)
            }
        }
        (Some((index, slot)), None) => {
            allocator.consumed_free_slots.insert(index);
            slot
        }
        (None, Some((index, _))) => allocator.added_free_slots.remove(index),
        (None, None) => {
            if allocator.cursor_x + required_width > allocator.width {
                allocator.cursor_x = ATLAS_PADDING * 3;
                allocator.cursor_y = allocator
                    .cursor_y
                    .checked_add(allocator.row_height + ATLAS_PADDING)
                    .ok_or_else(|| invalid("glyph atlas row overflowed"))?;
                allocator.row_height = 0;
            }
            // Wrapping cannot make a resource wider than the atlas fit.
            // Reject it before upload so the caller grows/re-packs the atlas.
            if allocator.cursor_x + required_width > allocator.width
                || allocator.cursor_y + required_height > allocator.height
            {
                return Err(invalid("glyph atlas capacity was exceeded"));
            }
            let placement = AtlasPlacement {
                x: allocator.cursor_x + ATLAS_PADDING,
                y: allocator.cursor_y + ATLAS_PADDING,
                width,
                height,
            };
            allocator.cursor_x += required_width;
            allocator.row_height = allocator.row_height.max(required_height);
            return Ok(placement);
        }
    };
    let used_area = u64::from(required_width) * u64::from(required_height);
    allocator.freed_area = allocator
        .freed_area
        .checked_sub(used_area)
        .ok_or_else(|| invalid("glyph atlas free-slot area accounting underflowed"))?;
    if selected.width > required_width {
        insert_pending_free_slot(
            allocator,
            AtlasSlot {
                x: selected.x + required_width,
                y: selected.y,
                width: selected.width - required_width,
                height: required_height,
            },
        );
    }
    if selected.height > required_height {
        insert_pending_free_slot(
            allocator,
            AtlasSlot {
                x: selected.x,
                y: selected.y + required_height,
                width: selected.width,
                height: selected.height - required_height,
            },
        );
    }
    Ok(AtlasPlacement {
        x: selected.x + ATLAS_PADDING,
        y: selected.y + ATLAS_PADDING,
        width,
        height,
    })
}

pub(crate) fn placement_to_slot(placement: AtlasPlacement) -> Result<AtlasSlot, PlatformError> {
    Ok(AtlasSlot {
        x: placement
            .x
            .checked_sub(ATLAS_PADDING)
            .ok_or_else(|| invalid("glyph atlas placement padding underflowed"))?,
        y: placement
            .y
            .checked_sub(ATLAS_PADDING)
            .ok_or_else(|| invalid("glyph atlas placement padding underflowed"))?,
        width: placement.width + ATLAS_PADDING * 2,
        height: placement.height + ATLAS_PADDING * 2,
    })
}

pub(crate) fn atlas_slot_area(slot: AtlasSlot) -> u64 {
    u64::from(slot.width) * u64::from(slot.height)
}

pub(crate) fn atlas_slot_sort_key(slot: &AtlasSlot) -> (u64, u32, u32) {
    (atlas_slot_area(*slot), slot.y, slot.x)
}
