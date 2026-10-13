use super::*;

pub(super) fn validate_texture_metadata(frame: &TextureFrame) -> Result<(), MediaError> {
    validate_texture_dimensions(frame.width, frame.height, frame.rgba8.len())
}

pub(super) fn validate_texture_dimensions(
    width: u32,
    height: u32,
    payload_len: usize,
) -> Result<(), MediaError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| MediaError::message("ASTRA_MEDIA_TEXTURE_SIZE: texture size overflows"))?;
    if width == 0 || height == 0 || payload_len != expected {
        return Err(MediaError::message(
            "ASTRA_MEDIA_TEXTURE_SIZE: texture dimensions do not match payload",
        ));
    }
    Ok(())
}

pub(super) fn validate_texture(frame: &TextureFrame) -> Result<(), MediaError> {
    validate_texture_metadata(frame)
}

pub(super) fn validate_glyph_metadata(glyph: &GlyphBitmap) -> Result<(), MediaError> {
    validate_glyph_dimensions(glyph.width, glyph.height, glyph.format, glyph.pixels.len())
}

pub(super) fn validate_glyph_dimensions(
    width: u32,
    height: u32,
    format: GlyphBitmapFormat,
    payload_len: usize,
) -> Result<(), MediaError> {
    let channels = match format {
        GlyphBitmapFormat::Alpha8 => 1,
        GlyphBitmapFormat::Rgba8 => 4,
    };
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| MediaError::message("ASTRA_MEDIA_GLYPH_SIZE: glyph size overflows"))?;
    if width == 0 || height == 0 || payload_len != expected || width > 4096 || height > 4096 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_GLYPH_SIZE: glyph dimensions do not match payload",
        ));
    }
    Ok(())
}

pub(super) fn validate_glyph(glyph: &GlyphBitmap) -> Result<(), MediaError> {
    validate_glyph_metadata(glyph)
}

pub(super) fn crop_texture(
    frame: &TextureFrame,
    source: RectI,
) -> Result<TextureFrame, MediaError> {
    if source.x < 0
        || source.y < 0
        || source.width == 0
        || source.height == 0
        || source.x as u32 + source.width > frame.width
        || source.y as u32 + source.height > frame.height
    {
        return Err(MediaError::message(
            "ASTRA_MEDIA_SOURCE_RECT: sprite source rectangle is outside the texture",
        ));
    }
    let mut rgba8 = Vec::with_capacity(source.width as usize * source.height as usize * 4);
    for y in source.y as u32..source.y as u32 + source.height {
        let start = ((y * frame.width + source.x as u32) * 4) as usize;
        let end = start + source.width as usize * 4;
        rgba8.extend_from_slice(&frame.rgba8[start..end]);
    }
    TextureFrame::from_vec(source.width, source.height, rgba8)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_solid(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    destination: RectI,
    rgba: [u8; 4],
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    let frame = TextureFrame::from_vec(1, 1, rgba.to_vec())?;
    draw_texture(
        target,
        target_width,
        target_height,
        clip,
        transform,
        destination,
        &frame,
        opacity,
        blend,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_texture(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    destination: RectI,
    frame: &TextureFrame,
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    validate_opacity(opacity)?;
    if destination.width == 0 || destination.height == 0 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_DESTINATION_SIZE: destination must be non-empty",
        ));
    }
    let inverse = transform
        .inverse()
        .ok_or_else(|| MediaError::message("ASTRA_MEDIA_TRANSFORM: transform is singular"))?;
    let bounds = intersection(clip, transformed_bounds(transform, destination)?);
    for y in bounds.y.max(0) as usize
        ..(bounds.y + bounds.height as i32)
            .max(0)
            .min(target_height as i32) as usize
    {
        for x in bounds.x.max(0) as usize
            ..(bounds.x + bounds.width as i32)
                .max(0)
                .min(target_width as i32) as usize
        {
            let (local_x, local_y) = inverse.point(x as f32 + 0.5, y as f32 + 0.5);
            let dx = local_x - destination.x as f32;
            let dy = local_y - destination.y as f32;
            if dx < 0.0
                || dy < 0.0
                || dx >= destination.width as f32
                || dy >= destination.height as f32
            {
                continue;
            }
            let sx = ((dx / destination.width as f32) * frame.width as f32).floor() as usize;
            let sy = ((dy / destination.height as f32) * frame.height as f32).floor() as usize;
            let source_offset = (sy.min(frame.height as usize - 1) * frame.width as usize
                + sx.min(frame.width as usize - 1))
                * 4;
            let target_offset = (y * target_width + x) * 4;
            blend_pixel(
                &mut target[target_offset..target_offset + 4],
                &frame.rgba8[source_offset..source_offset + 4],
                opacity,
                blend,
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_glyph(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    x: i32,
    y: i32,
    glyph: &GlyphBitmap,
    rotation_quadrants: u8,
    rgba: [u8; 4],
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    let mut rgba8 = Vec::with_capacity(glyph.width as usize * glyph.height as usize * 4);
    match glyph.format {
        GlyphBitmapFormat::Alpha8 => {
            for alpha in glyph.pixels.iter() {
                rgba8.extend_from_slice(&[
                    rgba[0],
                    rgba[1],
                    rgba[2],
                    ((*alpha as u16 * rgba[3] as u16) / 255) as u8,
                ]);
            }
        }
        GlyphBitmapFormat::Rgba8 => {
            for source in glyph.pixels.as_chunks::<4>().0.iter() {
                rgba8.extend_from_slice(&[
                    ((source[0] as u16 * rgba[0] as u16) / 255) as u8,
                    ((source[1] as u16 * rgba[1] as u16) / 255) as u8,
                    ((source[2] as u16 * rgba[2] as u16) / 255) as u8,
                    ((source[3] as u16 * rgba[3] as u16) / 255) as u8,
                ]);
            }
        }
    }
    let frame = TextureFrame::from_vec(glyph.width, glyph.height, rgba8)?;
    let frame = rotate_texture_clockwise(frame, rotation_quadrants % 4);
    draw_texture(
        target,
        target_width,
        target_height,
        clip,
        transform,
        RectI::new(x, y, frame.width, frame.height),
        &frame,
        opacity,
        blend,
    )
}

pub(super) fn rotate_texture_clockwise(mut frame: TextureFrame, quadrants: u8) -> TextureFrame {
    for _ in 0..quadrants {
        let source_width = frame.width as usize;
        let source_height = frame.height as usize;
        let mut rotated = vec![0_u8; frame.rgba8.len()];
        for source_y in 0..source_height {
            for source_x in 0..source_width {
                let target_x = source_height - source_y - 1;
                let target_y = source_x;
                let source = (source_y * source_width + source_x) * 4;
                let target = (target_y * source_height + target_x) * 4;
                rotated[target..target + 4].copy_from_slice(&frame.rgba8[source..source + 4]);
            }
        }
        frame = TextureFrame::from_vec(frame.height, frame.width, rotated)
            .expect("rotating a validated texture preserves its byte dimensions");
    }
    frame
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_mesh(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    clip: RectI,
    transform: Transform2D,
    vertices: &[MeshVertex2D],
    indices: &[u32],
    material: MeshMaterial2D,
    texture: Option<&TextureFrame>,
    texture_filter: TextureFilter2D,
    opacity: f32,
    blend: BlendMode,
) -> Result<(), MediaError> {
    validate_opacity(opacity)?;
    if vertices.is_empty() || indices.is_empty() || !indices.len().is_multiple_of(3) {
        return Err(MediaError::message(
            "ASTRA_MEDIA_MESH_TOPOLOGY: mesh requires triangle-list vertices and indices",
        ));
    }
    if vertices.len() > 250_000 || indices.len() > 750_000 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_MESH_BUDGET: mesh exceeds vertex or index budget",
        ));
    }
    if indices
        .iter()
        .any(|index| *index as usize >= vertices.len())
    {
        return Err(MediaError::message(
            "ASTRA_MEDIA_MESH_INDEX: mesh index is outside the vertex buffer",
        ));
    }
    if let Some(texture) = texture {
        validate_texture(texture)?;
    }
    let transformed = vertices
        .iter()
        .map(|vertex| {
            if vertex.position.iter().any(|value| !value.is_finite())
                || vertex.uv.iter().any(|value| !value.is_finite())
                || vertex.premultiplied_rgba[..3]
                    .iter()
                    .any(|channel| *channel > vertex.premultiplied_rgba[3])
            {
                return Err(MediaError::message(
                    "ASTRA_MEDIA_MESH_VERTEX: mesh vertex is invalid or not premultiplied",
                ));
            }
            let (x, y) = transform.point(vertex.position[0], vertex.position[1]);
            Ok(MeshVertex2D {
                position: [x, y],
                ..*vertex
            })
        })
        .collect::<Result<Vec<_>, MediaError>>()?;

    for triangle in indices.as_chunks::<3>().0.iter() {
        let a = transformed[triangle[0] as usize];
        let b = transformed[triangle[1] as usize];
        let c = transformed[triangle[2] as usize];
        let area = edge(a.position, b.position, c.position);
        if area.abs() < f32::EPSILON {
            continue;
        }
        let min_x = a.position[0].min(b.position[0]).min(c.position[0]).floor() as i32;
        let min_y = a.position[1].min(b.position[1]).min(c.position[1]).floor() as i32;
        let max_x = a.position[0].max(b.position[0]).max(c.position[0]).ceil() as i32;
        let max_y = a.position[1].max(b.position[1]).max(c.position[1]).ceil() as i32;
        let clip_x0 = clip.x.max(0).max(min_x);
        let clip_y0 = clip.y.max(0).max(min_y);
        let clip_x1 = (clip.x + clip.width as i32)
            .min(target_width as i32)
            .min(max_x);
        let clip_y1 = (clip.y + clip.height as i32)
            .min(target_height as i32)
            .min(max_y);
        for y in clip_y0..clip_y1 {
            for x in clip_x0..clip_x1 {
                let point = [x as f32 + 0.5, y as f32 + 0.5];
                let wa = edge(b.position, c.position, point) / area;
                let wb = edge(c.position, a.position, point) / area;
                let wc = 1.0 - wa - wb;
                let epsilon = -1.0e-5;
                if wa < epsilon || wb < epsilon || wc < epsilon {
                    continue;
                }
                let uv = [
                    wa * a.uv[0] + wb * b.uv[0] + wc * c.uv[0],
                    wa * a.uv[1] + wb * b.uv[1] + wc * c.uv[1],
                ];
                let mut color = [0u8; 4];
                for (channel, output) in color.iter_mut().enumerate() {
                    *output = (wa * a.premultiplied_rgba[channel] as f32
                        + wb * b.premultiplied_rgba[channel] as f32
                        + wc * c.premultiplied_rgba[channel] as f32)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
                if let Some(texture) = texture {
                    let texel = sample_texture(texture, uv, texture_filter);
                    match material {
                        MeshMaterial2D::ColorTexture => {
                            for (channel, texel_channel) in color.iter_mut().zip(texel) {
                                *channel = ((*channel as u16 * texel_channel as u16) / 255) as u8;
                            }
                        }
                        MeshMaterial2D::GlyphMask => {
                            let mask = texel[3];
                            for channel in &mut color {
                                *channel = ((*channel as u16 * mask as u16) / 255) as u8;
                            }
                        }
                        MeshMaterial2D::Solid => unreachable!("solid mesh has no texture"),
                    }
                }
                let offset = (y as usize * target_width + x as usize) * 4;
                blend_premultiplied_pixel(&mut target[offset..offset + 4], color, opacity, blend);
            }
        }
    }
    Ok(())
}

pub(super) fn edge(a: [f32; 2], b: [f32; 2], point: [f32; 2]) -> f32 {
    (point[0] - a[0]) * (b[1] - a[1]) - (point[1] - a[1]) * (b[0] - a[0])
}

pub(super) fn sample_texture(
    frame: &TextureFrame,
    uv: [f32; 2],
    texture_filter: TextureFilter2D,
) -> [u8; 4] {
    let u = uv[0].clamp(0.0, 1.0) * frame.width.saturating_sub(1) as f32;
    let v = uv[1].clamp(0.0, 1.0) * frame.height.saturating_sub(1) as f32;
    if texture_filter == TextureFilter2D::Nearest {
        return texture_pixel(frame, u.round() as usize, v.round() as usize);
    }
    let x0 = u.floor() as usize;
    let y0 = v.floor() as usize;
    let x1 = (x0 + 1).min(frame.width as usize - 1);
    let y1 = (y0 + 1).min(frame.height as usize - 1);
    let tx = u - x0 as f32;
    let ty = v - y0 as f32;
    let c00 = texture_pixel(frame, x0, y0);
    let c10 = texture_pixel(frame, x1, y0);
    let c01 = texture_pixel(frame, x0, y1);
    let c11 = texture_pixel(frame, x1, y1);
    std::array::from_fn(|channel| {
        let top = c00[channel] as f32 + (c10[channel] as f32 - c00[channel] as f32) * tx;
        let bottom = c01[channel] as f32 + (c11[channel] as f32 - c01[channel] as f32) * tx;
        (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8
    })
}

pub(super) fn texture_pixel(frame: &TextureFrame, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * frame.width as usize + x) * 4;
    frame.rgba8[offset..offset + 4]
        .try_into()
        .expect("validated RGBA texture has complete texels")
}
