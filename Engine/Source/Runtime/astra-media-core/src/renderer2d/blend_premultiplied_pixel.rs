use super::*;

pub(super) fn blend_premultiplied_pixel(
    target: &mut [u8],
    source: [u8; 4],
    opacity: f32,
    blend: BlendMode,
) {
    let source_alpha = source[3] as f32 / 255.0 * opacity;
    let target_alpha = target[3] as f32 / 255.0;
    let output_alpha = source_alpha + target_alpha * (1.0 - source_alpha);
    match blend {
        BlendMode::Alpha => {
            for channel in 0..3 {
                let source_premultiplied = source[channel] as f32 / 255.0 * opacity;
                let target_premultiplied = target[channel] as f32 / 255.0 * target_alpha;
                let output_premultiplied =
                    source_premultiplied + target_premultiplied * (1.0 - source_alpha);
                target[channel] = if output_alpha > 0.0 {
                    (output_premultiplied / output_alpha * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                } else {
                    0
                };
            }
        }
        BlendMode::Add => {
            for channel in 0..3 {
                target[channel] = target[channel]
                    .saturating_add((source[channel] as f32 * opacity).round() as u8);
            }
        }
        BlendMode::Opaque => {
            for channel in 0..3 {
                target[channel] =
                    (source[channel] as f32 * opacity).round().clamp(0.0, 255.0) as u8;
            }
        }
        BlendMode::Multiply => {
            let straight = if source_alpha > 0.0 {
                [
                    (source[0] as f32 * opacity / source_alpha).clamp(0.0, 255.0),
                    (source[1] as f32 * opacity / source_alpha).clamp(0.0, 255.0),
                    (source[2] as f32 * opacity / source_alpha).clamp(0.0, 255.0),
                ]
            } else {
                [0.0; 3]
            };
            for channel in 0..3 {
                let multiplied = target[channel] as f32 * straight[channel] / 255.0;
                target[channel] = (target[channel] as f32 * (1.0 - source_alpha)
                    + multiplied * source_alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        BlendMode::Screen => {
            let straight = if source_alpha > 0.0 {
                [
                    (source[0] as f32 * opacity / source_alpha).clamp(0.0, 255.0),
                    (source[1] as f32 * opacity / source_alpha).clamp(0.0, 255.0),
                    (source[2] as f32 * opacity / source_alpha).clamp(0.0, 255.0),
                ]
            } else {
                [0.0; 3]
            };
            for channel in 0..3 {
                let screened =
                    255.0 - (255.0 - target[channel] as f32) * (255.0 - straight[channel]) / 255.0;
                target[channel] = (target[channel] as f32 * (1.0 - source_alpha)
                    + screened * source_alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    target[3] = if matches!(blend, BlendMode::Opaque) {
        (source_alpha * 255.0).round().clamp(0.0, 255.0) as u8
    } else {
        (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8
    };
}

pub(super) fn transformed_bounds(transform: Transform2D, rect: RectI) -> Result<RectI, MediaError> {
    validate_transform(transform)?;
    let x1 = rect.x as f32 + rect.width as f32;
    let y1 = rect.y as f32 + rect.height as f32;
    let points = [
        transform.point(rect.x as f32, rect.y as f32),
        transform.point(x1, rect.y as f32),
        transform.point(rect.x as f32, y1),
        transform.point(x1, y1),
    ];
    let min_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let min_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let max_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    let max_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    Ok(RectI::new(
        min_x,
        min_y,
        max_x.saturating_sub(min_x) as u32,
        max_y.saturating_sub(min_y) as u32,
    ))
}

pub(super) fn intersection(left: RectI, right: RectI) -> RectI {
    let x0 = left.x.max(right.x);
    let y0 = left.y.max(right.y);
    let x1 = (left.x + left.width as i32).min(right.x + right.width as i32);
    let y1 = (left.y + left.height as i32).min(right.y + right.height as i32);
    RectI::new(
        x0,
        y0,
        x1.saturating_sub(x0) as u32,
        y1.saturating_sub(y0) as u32,
    )
}

pub(super) fn blend_pixel(target: &mut [u8], source: &[u8], opacity: f32, blend: BlendMode) {
    let alpha = source[3] as f32 / 255.0 * opacity;
    let target_alpha = target[3] as f32 / 255.0;
    let output_alpha = alpha + target_alpha * (1.0 - alpha);
    match blend {
        BlendMode::Alpha => {
            for channel in 0..3 {
                let premultiplied = source[channel] as f32 * alpha
                    + target[channel] as f32 * target_alpha * (1.0 - alpha);
                target[channel] = if output_alpha > 0.0 {
                    (premultiplied / output_alpha).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                };
            }
        }
        BlendMode::Add => {
            for channel in 0..3 {
                target[channel] =
                    target[channel].saturating_add((source[channel] as f32 * alpha).round() as u8);
            }
        }
        BlendMode::Opaque => {
            for channel in 0..3 {
                target[channel] =
                    (source[channel] as f32 * opacity).round().clamp(0.0, 255.0) as u8;
            }
        }
        BlendMode::Multiply => {
            for channel in 0..3 {
                let multiplied = target[channel] as f32 * source[channel] as f32 / 255.0;
                target[channel] = (target[channel] as f32 * (1.0 - alpha) + multiplied * alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        BlendMode::Screen => {
            for channel in 0..3 {
                let screened = 255.0
                    - (255.0 - target[channel] as f32) * (255.0 - source[channel] as f32) / 255.0;
                target[channel] = (target[channel] as f32 * (1.0 - alpha) + screened * alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    target[3] = if matches!(blend, BlendMode::Opaque) {
        (alpha * 255.0).round().clamp(0.0, 255.0) as u8
    } else {
        (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8
    };
}

pub fn frame_hash(width: u32, height: u32, format: RenderTargetFormat, bytes: &[u8]) -> Hash256 {
    let mut payload = Vec::with_capacity(12 + bytes.len());
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend_from_slice(&(format as u32).to_le_bytes());
    payload.extend_from_slice(bytes);
    Hash256::from_sha256(&payload)
}
