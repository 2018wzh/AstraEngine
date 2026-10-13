use super::*;
use astra_media_core::{BlendMode, RectI, SceneCompositing2D, TextureFilter2D, Transform2D};
use astra_platform::{PlatformError, SceneFrame};

pub(crate) fn build_vertices(
    frame: &SceneFrame,
    quad_runs: &[DrawRun],
    runs: &[DrawPrimitive],
    atlas: &AtlasPlacementView<'_>,
    resources: &AtlasResourceView<'_>,
    bytes: &mut Vec<u8>,
    batches: &mut Vec<DrawBatch>,
) -> Result<(), PlatformError> {
    let mut vertex_count = 0u32;
    for primitive in runs {
        let clip = match primitive {
            DrawPrimitive::Quads(index) => quad_runs[*index].clip,
            DrawPrimitive::Mesh(run) => run.clip,
        };
        let blend = match primitive {
            DrawPrimitive::Quads(index) => quad_runs[*index].blend,
            DrawPrimitive::Mesh(run) => run.blend,
        };
        let texture_filter = match primitive {
            DrawPrimitive::Quads(_) => TextureFilter2D::Linear,
            DrawPrimitive::Mesh(run) => run.texture_filter,
        };
        let compositing = match primitive {
            DrawPrimitive::Quads(index) => quad_runs[*index].compositing,
            DrawPrimitive::Mesh(run) => run.compositing,
        };
        if clip.width == 0 || clip.height == 0 {
            continue;
        }
        let pixel_mask = match primitive {
            DrawPrimitive::Quads(index) => quad_runs[*index].pixel_mask,
            DrawPrimitive::Mesh(run) => run.pixel_mask,
        };
        let first_vertex = vertex_count;
        match primitive {
            DrawPrimitive::Quads(index) => {
                let run = &quad_runs[*index];
                let color = straight_to_premultiplied(run.rgba, run.opacity, run.compositing);
                for quad in &run.quads {
                    let left = quad.destination.x as f32;
                    let top = quad.destination.y as f32;
                    let rotated = quad.rotation_quadrants % 4;
                    let (draw_width, draw_height) = if rotated % 2 == 0 {
                        (quad.destination.width, quad.destination.height)
                    } else {
                        (quad.destination.height, quad.destination.width)
                    };
                    let right = left + draw_width as f32;
                    let bottom = top + draw_height as f32;
                    let (u0, v0, u1, v1) = match &quad.source {
                        QuadSource::Resource {
                            resource_id,
                            source,
                        } => {
                            let placement = atlas
                                .get(resource_id.as_ref())
                                .ok_or_else(|| invalid("draw resource has no atlas placement"))?;
                            let source_right = source.x as u32 + source.width;
                            let source_bottom = source.y as u32 + source.height;
                            (
                                (placement.x + source.x as u32) as f32 / atlas.width() as f32,
                                (placement.y + source.y as u32) as f32 / atlas.height() as f32,
                                (placement.x + source_right) as f32 / atlas.width() as f32,
                                (placement.y + source_bottom) as f32 / atlas.height() as f32,
                            )
                        }
                        QuadSource::White => {
                            let u = 0.5 / atlas.width() as f32;
                            let v = 0.5 / atlas.height() as f32;
                            (u, v, u, v)
                        }
                    };
                    let uv = match rotated {
                        0 => [(u0, v0), (u1, v0), (u1, v1), (u0, v1)],
                        1 => [(u0, v1), (u0, v0), (u1, v0), (u1, v1)],
                        2 => [(u1, v1), (u0, v1), (u0, v0), (u1, v0)],
                        3 => [(u1, v0), (u1, v1), (u0, v1), (u0, v0)],
                        _ => unreachable!(),
                    };
                    let positions = [(left, top), (right, top), (right, bottom), (left, bottom)];
                    for corner in [0, 1, 2, 0, 2, 3] {
                        let (x, y) = transform_point(
                            run.transform,
                            positions[corner].0,
                            positions[corner].1,
                        );
                        let (u, v) = uv[corner];
                        push_vertex(
                            bytes,
                            x,
                            y,
                            u,
                            v,
                            color,
                            frame.width,
                            frame.height,
                            pixel_mask,
                        );
                        vertex_count = vertex_count
                            .checked_add(1)
                            .ok_or_else(|| invalid("scene vertex count overflowed"))?;
                    }
                }
            }
            DrawPrimitive::Mesh(run) => {
                let texture_mapping = run
                    .texture_id
                    .as_ref()
                    .map(|resource_id| {
                        let resource = resources
                            .get(resource_id)
                            .ok_or_else(|| invalid("mesh texture resource is missing"))?;
                        let placement = atlas
                            .get(resource_id)
                            .ok_or_else(|| invalid("mesh texture has no atlas placement"))?;
                        Ok::<_, PlatformError>((placement, resource.width(), resource.height()))
                    })
                    .transpose()?;
                for index in run.indices {
                    let vertex = &run.vertices[*index as usize];
                    let (x, y) =
                        transform_point(run.transform, vertex.position[0], vertex.position[1]);
                    let (u, v) = if let Some((placement, width, height)) = texture_mapping {
                        (
                            (placement.x as f32 + vertex.uv[0] * width as f32)
                                / atlas.width() as f32,
                            (placement.y as f32 + vertex.uv[1] * height as f32)
                                / atlas.height() as f32,
                        )
                    } else {
                        (0.5 / atlas.width() as f32, 0.5 / atlas.height() as f32)
                    };
                    let color = match run.compositing {
                        SceneCompositing2D::LinearSrgb => {
                            premultiplied_linear(vertex.premultiplied_rgba, run.opacity)
                        }
                        SceneCompositing2D::EncodedSrgb => {
                            premultiplied_encoded(vertex.premultiplied_rgba, run.opacity)
                        }
                    };
                    push_vertex(
                        bytes,
                        x,
                        y,
                        u,
                        v,
                        color,
                        frame.width,
                        frame.height,
                        pixel_mask,
                    );
                    vertex_count = vertex_count
                        .checked_add(1)
                        .ok_or_else(|| invalid("scene vertex count overflowed"))?;
                }
            }
        }
        if vertex_count > first_vertex {
            batches.push(DrawBatch {
                first_vertex,
                vertex_count: vertex_count - first_vertex,
                clip,
                blend,
                texture_filter,
                compositing,
            });
        }
    }
    Ok(())
}

pub(crate) fn premultiplied_linear(rgba: [u8; 4], opacity: f32) -> [f32; 4] {
    let alpha = f32::from(rgba[3]) / 255.0;
    let convert = |value: u8| {
        if alpha == 0.0 {
            0.0
        } else {
            srgb_byte_to_linear(((f32::from(value) / alpha).min(255.0)).round() as u8)
                * alpha
                * opacity
        }
    };
    [
        convert(rgba[0]),
        convert(rgba[1]),
        convert(rgba[2]),
        alpha * opacity,
    ]
}

pub(crate) fn premultiplied_encoded(rgba: [u8; 4], opacity: f32) -> [f32; 4] {
    [
        f32::from(rgba[0]) / 255.0 * opacity,
        f32::from(rgba[1]) / 255.0 * opacity,
        f32::from(rgba[2]) / 255.0 * opacity,
        f32::from(rgba[3]) / 255.0 * opacity,
    ]
}

pub(crate) fn straight_to_premultiplied(
    rgba: [u8; 4],
    opacity: f32,
    compositing: SceneCompositing2D,
) -> [f32; 4] {
    let alpha = f32::from(rgba[3]) / 255.0 * opacity;
    let channel = |value| match compositing {
        SceneCompositing2D::LinearSrgb => srgb_byte_to_linear(value),
        SceneCompositing2D::EncodedSrgb => f32::from(value) / 255.0,
    };
    [
        channel(rgba[0]) * alpha,
        channel(rgba[1]) * alpha,
        channel(rgba[2]) * alpha,
        alpha,
    ]
}

pub(crate) fn srgb_byte_to_linear(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn push_vertex(
    bytes: &mut Vec<u8>,
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    color: [f32; 4],
    width: u32,
    height: u32,
    pixel_mask: u64,
) {
    let ndc_x = x / width as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - y / height as f32 * 2.0;
    for value in [ndc_x, ndc_y, u, v, color[0], color[1], color[2], color[3]] {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes.extend_from_slice(&(pixel_mask as u32).to_ne_bytes());
    bytes.extend_from_slice(&((pixel_mask >> 32) as u32).to_ne_bytes());
}

pub(crate) fn intersect_clip(
    current: Option<RectI>,
    next: RectI,
    width: u32,
    height: u32,
) -> Result<RectI, PlatformError> {
    let bounds = RectI::new(0, 0, width, height);
    let current = current.unwrap_or(bounds);
    let left = i64::from(current.x).max(i64::from(next.x)).max(0);
    let top = i64::from(current.y).max(i64::from(next.y)).max(0);
    let right = (i64::from(current.x) + i64::from(current.width))
        .min(i64::from(next.x) + i64::from(next.width))
        .min(i64::from(width));
    let bottom = (i64::from(current.y) + i64::from(current.height))
        .min(i64::from(next.y) + i64::from(next.height))
        .min(i64::from(height));
    let clipped_width = (right - left).max(0) as u32;
    let clipped_height = (bottom - top).max(0) as u32;
    Ok(RectI::new(
        left as i32,
        top as i32,
        clipped_width,
        clipped_height,
    ))
}

pub(crate) fn validate_draw_identity<'a>(
    id: &'a str,
    opacity: &f32,
    blend: BlendMode,
    run_ids: &mut SceneIdSet<'a>,
) -> Result<(), PlatformError> {
    if id.is_empty()
        || id.len() > 256
        || !run_ids.insert(id)
        || !opacity.is_finite()
        || !(0.0..=1.0).contains(opacity)
        || !matches!(blend, BlendMode::Alpha | BlendMode::Screen)
    {
        return Err(invalid("draw identity, opacity, or blend mode is invalid"));
    }
    Ok(())
}

pub(crate) fn transient_resource_id(
    resources: &AtlasResourceView<'_>,
    sequence: &mut u64,
    kind: &str,
) -> Result<String, PlatformError> {
    loop {
        *sequence = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("transient scene resource sequence overflowed"))?;
        let id = format!("astra.transient.{kind}.{sequence}");
        if !resources.contains_key(&id) {
            return Ok(id);
        }
    }
}

pub(crate) fn validate_opacity(opacity: f32) -> Result<(), PlatformError> {
    if opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(invalid("scene opacity is invalid"))
    }
}

pub(crate) fn validate_transform(transform: Transform2D) -> Result<(), PlatformError> {
    let values = [
        transform.m11,
        transform.m12,
        transform.m21,
        transform.m22,
        transform.tx,
        transform.ty,
    ];
    let determinant = transform.m11 * transform.m22 - transform.m12 * transform.m21;
    if values.into_iter().all(f32::is_finite)
        && determinant.is_finite()
        && determinant.abs() >= f32::EPSILON
    {
        Ok(())
    } else {
        Err(invalid("scene transform is invalid or singular"))
    }
}

pub(crate) fn compose_transform(current: Transform2D, next: Transform2D) -> Transform2D {
    Transform2D {
        m11: next.m11 * current.m11 + next.m21 * current.m12,
        m12: next.m12 * current.m11 + next.m22 * current.m12,
        m21: next.m11 * current.m21 + next.m21 * current.m22,
        m22: next.m12 * current.m21 + next.m22 * current.m22,
        tx: next.m11 * current.tx + next.m21 * current.ty + next.tx,
        ty: next.m12 * current.tx + next.m22 * current.ty + next.ty,
    }
}

pub(crate) fn current_transform(camera: Transform2D, stack: &[Transform2D]) -> Transform2D {
    compose_transform(
        stack.last().copied().unwrap_or(Transform2D::IDENTITY),
        camera,
    )
}

pub(crate) fn transform_point(transform: Transform2D, x: f32, y: f32) -> (f32, f32) {
    (
        transform.m11 * x + transform.m21 * y + transform.tx,
        transform.m12 * x + transform.m22 * y + transform.ty,
    )
}

pub(crate) fn transformed_bounds(
    transform: Transform2D,
    rect: RectI,
) -> Result<RectI, PlatformError> {
    let right = rect.x as f32 + rect.width as f32;
    let bottom = rect.y as f32 + rect.height as f32;
    let points = [
        transform_point(transform, rect.x as f32, rect.y as f32),
        transform_point(transform, right, rect.y as f32),
        transform_point(transform, right, bottom),
        transform_point(transform, rect.x as f32, bottom),
    ];
    let min_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min);
    let min_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max);
    if ![min_x, min_y, max_x, max_y].into_iter().all(f32::is_finite)
        || min_x < i32::MIN as f32
        || min_y < i32::MIN as f32
        || max_x > i32::MAX as f32
        || max_y > i32::MAX as f32
    {
        return Err(invalid(
            "transformed scene clip is outside supported coordinates",
        ));
    }
    let left = min_x.floor() as i32;
    let top = min_y.floor() as i32;
    let right = max_x.ceil() as i64;
    let bottom = max_y.ceil() as i64;
    let width = u32::try_from(right - i64::from(left))
        .map_err(|_| invalid("transformed scene clip width overflowed"))?;
    let height = u32::try_from(bottom - i64::from(top))
        .map_err(|_| invalid("transformed scene clip height overflowed"))?;
    Ok(RectI::new(left, top, width, height))
}
