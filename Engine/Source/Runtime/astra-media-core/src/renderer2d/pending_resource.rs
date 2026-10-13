use super::*;

pub(super) enum PendingResource<'a> {
    Texture(&'a TextureFrame),
    Glyph(&'a GlyphBitmap),
    Released,
}

pub(super) fn retained_resource_exists_indexed(
    texture_ids: &HashSet<String>,
    glyph_ids: &HashSet<String>,
    overlay: &BTreeMap<&str, PendingResource<'_>>,
    resource_id: &str,
) -> bool {
    match overlay.get(resource_id) {
        Some(PendingResource::Texture(_) | PendingResource::Glyph(_)) => true,
        Some(PendingResource::Released) => false,
        None => texture_ids.contains(resource_id) || glyph_ids.contains(resource_id),
    }
}

pub(super) fn retained_texture_dimensions(
    textures: &BTreeMap<String, TextureFrame>,
    overlay: &BTreeMap<&str, PendingResource<'_>>,
    resource_id: &str,
) -> Option<(u32, u32)> {
    match overlay.get(resource_id) {
        Some(PendingResource::Texture(frame)) => Some((frame.width, frame.height)),
        Some(PendingResource::Glyph(_) | PendingResource::Released) => None,
        None => textures
            .get(resource_id)
            .map(|frame| (frame.width, frame.height)),
    }
}

pub(super) fn retained_glyph_exists(
    glyph_ids: &HashSet<String>,
    overlay: &BTreeMap<&str, PendingResource<'_>>,
    resource_id: &str,
) -> bool {
    match overlay.get(resource_id) {
        Some(PendingResource::Glyph(glyph)) => {
            let _ = glyph;
            true
        }
        Some(PendingResource::Texture(_) | PendingResource::Released) => false,
        None => glyph_ids.contains(resource_id),
    }
}

pub(super) fn validate_source_rect_dimensions(
    dimensions: (u32, u32),
    source: RectI,
) -> Result<(), MediaError> {
    let right = source.x.checked_add_unsigned(source.width);
    let bottom = source.y.checked_add_unsigned(source.height);
    if source.x < 0
        || source.y < 0
        || source.width == 0
        || source.height == 0
        || right.is_none_or(|value| value > dimensions.0 as i32)
        || bottom.is_none_or(|value| value > dimensions.1 as i32)
    {
        return Err(MediaError::message(
            "ASTRA_MEDIA_TEXTURE_SOURCE: source rectangle is outside the texture",
        ));
    }
    Ok(())
}

pub(super) fn validate_texture_region(
    texture_dimensions: (u32, u32),
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    rgba8: &OwnedPixelBuffer,
) -> Result<(), MediaError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| MediaError::message("ASTRA_MEDIA_TEXTURE_REGION: byte count overflow"))?;
    if width == 0
        || height == 0
        || x.checked_add(width)
            .is_none_or(|right| right > texture_dimensions.0)
        || y.checked_add(height)
            .is_none_or(|bottom| bottom > texture_dimensions.1)
        || rgba8.len() != expected
    {
        return Err(MediaError::message(
            "ASTRA_MEDIA_TEXTURE_REGION: update is outside the retained texture",
        ));
    }
    Ok(())
}

pub(super) fn apply_texture_region(
    frame: &mut TextureFrame,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    rgba8: &[u8],
) -> Result<(), MediaError> {
    let pixels = frame.rgba8.make_mut_for_update();
    for row in 0..height as usize {
        let destination = ((y as usize + row) * frame.width as usize + x as usize) * 4;
        let source = row * width as usize * 4;
        let byte_len = width as usize * 4;
        pixels[destination..destination + byte_len]
            .copy_from_slice(&rgba8[source..source + byte_len]);
    }
    Ok(())
}
