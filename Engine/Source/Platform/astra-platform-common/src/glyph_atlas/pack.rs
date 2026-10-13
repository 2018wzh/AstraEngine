use super::*;
use astra_platform::PlatformError;
use std::collections::BTreeMap;

pub(crate) fn pack_atlas(resources: &AtlasResourceView<'_>) -> Result<PackedAtlas, PlatformError> {
    pack_atlas_with_min_side(resources, MIN_ATLAS_SIDE)
}

pub(crate) fn pack_atlas_with_min_side(
    resources: &AtlasResourceView<'_>,
    minimum_side: u32,
) -> Result<PackedAtlas, PlatformError> {
    let widest = resources
        .values()
        .map(|resource| resource.width() + ATLAS_PADDING * 2)
        .max()
        .unwrap_or(64);
    let total_area = resources.values().try_fold(0_u64, |total, resource| {
        let width = u64::from(resource.width() + ATLAS_PADDING * 2);
        let height = u64::from(resource.height() + ATLAS_PADDING * 2);
        total
            .checked_add(
                width
                    .checked_mul(height)
                    .ok_or_else(|| invalid("glyph atlas resource area overflowed"))?,
            )
            .ok_or_else(|| invalid("glyph atlas total area overflowed"))
    })?;
    if widest > ATLAS_SIDE || total_area > u64::from(ATLAS_SIDE) * u64::from(ATLAS_SIDE) {
        return Err(invalid("glyph is larger than the configured atlas"));
    }
    let mut ordered = resources.iter().collect::<Vec<_>>();
    ordered.sort_by(|(left_id, left), (right_id, right)| {
        right
            .height()
            .cmp(&left.height())
            .then_with(|| right.width().cmp(&left.width()))
            .then_with(|| left_id.cmp(right_id))
    });
    for side in [MIN_ATLAS_SIDE, 2048, ATLAS_SIDE]
        .into_iter()
        .filter(|side| *side >= minimum_side)
    {
        if let Some(packed) = pack_atlas_for_side(&ordered, side)? {
            return Ok(packed);
        }
    }
    Err(invalid("glyph atlas capacity was exceeded"))
}

pub(crate) fn pack_atlas_for_side(
    ordered: &[(&String, &AtlasResource)],
    side: u32,
) -> Result<Option<PackedAtlas>, PlatformError> {
    pack_atlas_for_extent(ordered, side, side)
}

pub(crate) fn pack_atlas_at_extent(
    resources: &AtlasResourceView<'_>,
    width: u32,
    height: u32,
) -> Result<Option<PackedAtlas>, PlatformError> {
    if width < MIN_ATLAS_SIDE
        || height < MIN_ATLAS_SIDE
        || width > MAX_ATLAS_WIDTH
        || height > ATLAS_SIDE
        || !width.is_power_of_two()
        || !height.is_power_of_two()
    {
        return Err(invalid("glyph atlas extent is outside the supported range"));
    }
    let mut ordered = resources.iter().collect::<Vec<_>>();
    ordered.sort_by(|(left_id, left), (right_id, right)| {
        right
            .height()
            .cmp(&left.height())
            .then_with(|| right.width().cmp(&left.width()))
            .then_with(|| left_id.cmp(right_id))
    });
    pack_atlas_for_extent(&ordered, width, height)
}

pub(crate) fn pack_atlas_for_extent(
    ordered: &[(&String, &AtlasResource)],
    width: u32,
    height: u32,
) -> Result<Option<PackedAtlas>, PlatformError> {
    let mut placements = BTreeMap::new();
    let mut x = ATLAS_PADDING * 3;
    let mut y = ATLAS_PADDING;
    let mut row_height = 0;
    for &(resource_id, resource) in ordered {
        if resource.width() + ATLAS_PADDING * 2 > width
            || resource.height() + ATLAS_PADDING * 2 > height
        {
            return Ok(None);
        }
        if x + resource.width() + ATLAS_PADDING > width {
            x = ATLAS_PADDING * 3;
            y = y
                .checked_add(row_height + ATLAS_PADDING)
                .ok_or_else(|| invalid("glyph atlas row overflowed"))?;
            row_height = 0;
        }
        if y + resource.height() + ATLAS_PADDING > height {
            return Ok(None);
        }
        placements.insert(
            (*resource_id).clone(),
            AtlasPlacement {
                x,
                y,
                width: resource.width(),
                height: resource.height(),
            },
        );
        x += resource.width() + ATLAS_PADDING;
        row_height = row_height.max(resource.height());
    }
    Ok(Some(PackedAtlas {
        placements,
        width,
        height,
        cursor_x: x,
        cursor_y: y,
        row_height,
        free_slots: Vec::new(),
        freed_area: 0,
    }))
}
