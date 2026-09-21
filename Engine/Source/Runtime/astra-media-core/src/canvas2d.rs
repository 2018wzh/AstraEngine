use crate::{MediaError, RectI, Transform2D};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A non-empty two-dimensional extent in a declared coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

impl Extent2D {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn validate(self) -> Result<Self, MediaError> {
        if self.width == 0 || self.height == 0 {
            return Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_EXTENT: extent must be non-empty",
            ));
        }
        Ok(self)
    }
}

/// The integer content rectangle selected by an aspect-fit canvas.
///
/// The rectangle is centered with the extra pixel, when the remaining space
/// is odd, kept on the right or bottom edge. This convention is shared by
/// scene rendering and input mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Viewport2D {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Maps a logical two-dimensional coordinate space to a raster target.
///
/// The mapping uses one uniform scale and a centered aspect-fit viewport. A
/// raster may have any positive dimensions; pixels outside the viewport are
/// presentation black bars and are never part of the logical stage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Canvas2D {
    pub logical: Extent2D,
    pub raster: Extent2D,
    viewport: Viewport2D,
    scale: f32,
}

impl Canvas2D {
    pub fn new(logical: Extent2D, raster: Extent2D) -> Result<Self, MediaError> {
        logical.validate()?;
        raster.validate()?;

        let width_limited = u64::from(raster.width) * u64::from(logical.height)
            <= u64::from(raster.height) * u64::from(logical.width);
        let (viewport_width, viewport_height, scale) = if width_limited {
            let height = (u64::from(raster.width) * u64::from(logical.height)
                / u64::from(logical.width))
            .max(1);
            (
                raster.width,
                u32::try_from(height).map_err(|_| {
                    MediaError::message("ASTRA_MEDIA_CANVAS_VIEWPORT: viewport height overflows")
                })?,
                raster.width as f64 / logical.width as f64,
            )
        } else {
            let width = (u64::from(raster.height) * u64::from(logical.width)
                / u64::from(logical.height))
            .max(1);
            (
                u32::try_from(width).map_err(|_| {
                    MediaError::message("ASTRA_MEDIA_CANVAS_VIEWPORT: viewport width overflows")
                })?,
                raster.height,
                raster.height as f64 / logical.height as f64,
            )
        };
        let scale = scale as f32;
        if !scale.is_finite() || scale <= 0.0 {
            return Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_SCALE: canvas scale is invalid",
            ));
        }

        Ok(Self {
            logical,
            raster,
            viewport: Viewport2D {
                x: (raster.width - viewport_width) / 2,
                y: (raster.height - viewport_height) / 2,
                width: viewport_width,
                height: viewport_height,
            },
            scale,
        })
    }

    pub fn scale_x(self) -> f32 {
        self.scale
    }

    pub fn scale_y(self) -> f32 {
        self.scale
    }

    pub fn uniform_scale(self) -> Option<f32> {
        Some(self.scale)
    }

    pub fn viewport(self) -> Viewport2D {
        self.viewport
    }

    pub fn viewport_rect(self) -> Result<RectI, MediaError> {
        Ok(RectI::new(
            i32::try_from(self.viewport.x).map_err(|_| {
                MediaError::message("ASTRA_MEDIA_CANVAS_VIEWPORT: viewport x overflows")
            })?,
            i32::try_from(self.viewport.y).map_err(|_| {
                MediaError::message("ASTRA_MEDIA_CANVAS_VIEWPORT: viewport y overflows")
            })?,
            self.viewport.width,
            self.viewport.height,
        ))
    }

    pub fn contains_raster_point(self, point: [f32; 2]) -> bool {
        point[0].is_finite()
            && point[1].is_finite()
            && point[0] >= self.viewport.x as f32
            && point[1] >= self.viewport.y as f32
            && point[0] < (self.viewport.x + self.viewport.width) as f32
            && point[1] < (self.viewport.y + self.viewport.height) as f32
    }

    pub fn logical_to_raster_transform(self) -> Transform2D {
        Transform2D {
            m11: self.scale,
            m22: self.scale,
            tx: self.viewport.x as f32,
            ty: self.viewport.y as f32,
            ..Transform2D::IDENTITY
        }
    }

    pub fn raster_to_logical_transform(self) -> Transform2D {
        Transform2D {
            m11: 1.0 / self.scale,
            m22: 1.0 / self.scale,
            tx: -(self.viewport.x as f32 / self.scale),
            ty: -(self.viewport.y as f32 / self.scale),
            ..Transform2D::IDENTITY
        }
    }

    pub fn logical_to_raster_point(self, point: [f32; 2]) -> Result<[f32; 2], MediaError> {
        let mapped = [
            point[0] * self.scale + self.viewport.x as f32,
            point[1] * self.scale + self.viewport.y as f32,
        ];
        if mapped.iter().all(|value| value.is_finite()) {
            Ok(mapped)
        } else {
            Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_POINT: mapped point is not finite",
            ))
        }
    }

    pub fn raster_to_logical_point(self, point: [f32; 2]) -> Result<[f32; 2], MediaError> {
        if !self.contains_raster_point(point) {
            return Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_POINT: point is outside the content viewport",
            ));
        }
        let mapped = [
            (point[0] - self.viewport.x as f32) / self.scale,
            (point[1] - self.viewport.y as f32) / self.scale,
        ];
        if mapped.iter().all(|value| value.is_finite()) {
            Ok(mapped)
        } else {
            Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_POINT: mapped point is not finite",
            ))
        }
    }

    pub fn logical_to_raster_rect(self, rect: RectI) -> Result<RectI, MediaError> {
        let x = scale_signed_with_offset(rect.x, self.scale, self.viewport.x)?;
        let y = scale_signed_with_offset(rect.y, self.scale, self.viewport.y)?;
        let width = scale_unsigned(rect.width, self.scale)?;
        let height = scale_unsigned(rect.height, self.scale)?;
        Ok(RectI::new(x, y, width, height))
    }
}

fn scale_signed_with_offset(value: i32, scale: f32, offset: u32) -> Result<i32, MediaError> {
    let scaled = f64::from(value) * f64::from(scale) + f64::from(offset);
    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_CANVAS_RECT: mapped coordinate overflows",
        ));
    }
    Ok(scaled.round() as i32)
}

fn scale_unsigned(value: u32, scale: f32) -> Result<u32, MediaError> {
    let scaled = f64::from(value) * f64::from(scale);
    if !scaled.is_finite() || scaled <= 0.0 || scaled > u32::MAX as f64 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_CANVAS_RECT: mapped extent overflows",
        ));
    }
    Ok(scaled.round().max(1.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_logical_geometry_to_a_uniform_raster() {
        let canvas = Canvas2D::new(Extent2D::new(1280, 720), Extent2D::new(1920, 1080))
            .expect("16:9 canvas");
        assert_eq!(canvas.uniform_scale(), Some(1.5));
        assert_eq!(
            canvas
                .logical_to_raster_rect(RectI::new(160, 568, 960, 112))
                .expect("rect fits the raster"),
            RectI::new(240, 852, 1440, 168)
        );
        assert_eq!(
            canvas
                .logical_to_raster_point([640.0, 360.0])
                .expect("point maps to the raster"),
            [960.0, 540.0]
        );
        assert_eq!(
            canvas
                .raster_to_logical_point([960.0, 540.0])
                .expect("point maps back to the stage"),
            [640.0, 360.0]
        );
    }

    #[test]
    fn supported_raster_scales_preserve_logical_geometry() {
        let logical = Extent2D::new(1280, 720);
        for (raster, scale) in [
            (Extent2D::new(1280, 720), 1.0),
            (Extent2D::new(1920, 1080), 1.5),
            (Extent2D::new(2560, 1440), 2.0),
            (Extent2D::new(3840, 2160), 3.0),
        ] {
            let canvas = Canvas2D::new(logical, raster).expect("supported raster scale");
            assert_eq!(canvas.uniform_scale(), Some(scale));
            assert_eq!(
                canvas
                    .logical_to_raster_point([640.0, 360.0])
                    .expect("center maps to raster"),
                [640.0 * scale, 360.0 * scale]
            );
            assert_eq!(
                canvas
                    .raster_to_logical_point([640.0 * scale, 360.0 * scale])
                    .expect("center maps back to logical stage"),
                [640.0, 360.0]
            );
        }
    }

    #[test]
    fn rejects_empty_canvas_and_aspect_fits_stretched_canvas() {
        assert!(Canvas2D::new(Extent2D::new(0, 720), Extent2D::new(1920, 1080)).is_err());
        let canvas = Canvas2D::new(Extent2D::new(1280, 720), Extent2D::new(1920, 1200))
            .expect("aspect-fit canvas");
        assert_eq!(
            canvas.viewport(),
            Viewport2D {
                x: 0,
                y: 60,
                width: 1920,
                height: 1080,
            }
        );
        assert!(canvas.raster_to_logical_point([0.0, 20.0]).is_err());
    }

    #[test]
    fn aspect_fit_rounding_is_centered_for_odd_raster_dimensions() {
        let canvas = Canvas2D::new(Extent2D::new(1280, 720), Extent2D::new(721, 1280))
            .expect("portrait canvas");
        assert_eq!(
            canvas.viewport(),
            Viewport2D {
                x: 0,
                y: 437,
                width: 721,
                height: 405,
            }
        );
        assert_eq!(
            canvas
                .logical_to_raster_point([0.0, 0.0])
                .expect("logical origin maps"),
            [0.0, 437.0]
        );
        assert!(canvas.raster_to_logical_point([0.0, 200.0]).is_err());
    }

    #[test]
    fn tiny_positive_raster_keeps_positive_logical_rects_addressable() {
        let canvas = Canvas2D::new(Extent2D::new(1280, 720), Extent2D::new(1, 1))
            .expect("one pixel raster is a valid extent");
        assert_eq!(
            canvas.viewport(),
            Viewport2D {
                x: 0,
                y: 0,
                width: 1,
                height: 1
            }
        );
        assert_eq!(
            canvas
                .logical_to_raster_rect(RectI::new(0, 0, 1, 1))
                .unwrap(),
            RectI::new(0, 0, 1, 1)
        );
    }
}
