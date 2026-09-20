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

/// Maps a logical two-dimensional coordinate space to a raster target.
///
/// The mapping is deliberately uniform.  A Family or product may choose any
/// raster scale, but it must not change the logical aspect ratio by silently
/// stretching its scene.  Physical texture dimensions remain asset metadata;
/// this type only describes the scene-to-target mapping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Canvas2D {
    pub logical: Extent2D,
    pub raster: Extent2D,
    scale_x: f32,
    scale_y: f32,
}

impl Canvas2D {
    pub fn new(logical: Extent2D, raster: Extent2D) -> Result<Self, MediaError> {
        logical.validate()?;
        raster.validate()?;
        let left = u64::from(logical.width) * u64::from(raster.height);
        let right = u64::from(logical.height) * u64::from(raster.width);
        if left != right {
            return Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_ASPECT: logical and raster extents must have the same aspect ratio",
            ));
        }
        let scale_x = raster.width as f32 / logical.width as f32;
        let scale_y = raster.height as f32 / logical.height as f32;
        if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
            return Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_SCALE: canvas scale is invalid",
            ));
        }
        Ok(Self {
            logical,
            raster,
            scale_x,
            scale_y,
        })
    }

    pub fn scale_x(self) -> f32 {
        self.scale_x
    }

    pub fn scale_y(self) -> f32 {
        self.scale_y
    }

    pub fn uniform_scale(self) -> Option<f32> {
        (self.scale_x.to_bits() == self.scale_y.to_bits()).then_some(self.scale_x)
    }

    pub fn logical_to_raster_transform(self) -> Transform2D {
        Transform2D {
            m11: self.scale_x,
            m22: self.scale_y,
            ..Transform2D::IDENTITY
        }
    }

    pub fn raster_to_logical_transform(self) -> Transform2D {
        Transform2D {
            m11: 1.0 / self.scale_x,
            m22: 1.0 / self.scale_y,
            ..Transform2D::IDENTITY
        }
    }

    pub fn logical_to_raster_point(self, point: [f32; 2]) -> Result<[f32; 2], MediaError> {
        let mapped = [point[0] * self.scale_x, point[1] * self.scale_y];
        if mapped.iter().all(|value| value.is_finite()) {
            Ok(mapped)
        } else {
            Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_POINT: mapped point is not finite",
            ))
        }
    }

    pub fn raster_to_logical_point(self, point: [f32; 2]) -> Result<[f32; 2], MediaError> {
        let mapped = [point[0] / self.scale_x, point[1] / self.scale_y];
        if mapped.iter().all(|value| value.is_finite()) {
            Ok(mapped)
        } else {
            Err(MediaError::message(
                "ASTRA_MEDIA_CANVAS_POINT: mapped point is not finite",
            ))
        }
    }

    pub fn logical_to_raster_rect(self, rect: RectI) -> Result<RectI, MediaError> {
        let x = scale_signed(rect.x, self.scale_x)?;
        let y = scale_signed(rect.y, self.scale_y)?;
        let width = scale_unsigned(rect.width, self.scale_x)?;
        let height = scale_unsigned(rect.height, self.scale_y)?;
        Ok(RectI::new(x, y, width, height))
    }
}

fn scale_signed(value: i32, scale: f32) -> Result<i32, MediaError> {
    let scaled = (value as f32) * scale;
    if !scaled.is_finite() || scaled < i32::MIN as f32 || scaled > i32::MAX as f32 {
        return Err(MediaError::message(
            "ASTRA_MEDIA_CANVAS_RECT: mapped coordinate overflows",
        ));
    }
    Ok(scaled.round() as i32)
}

fn scale_unsigned(value: u32, scale: f32) -> Result<u32, MediaError> {
    let scaled = (value as f32) * scale;
    if !scaled.is_finite() || scaled <= 0.0 || scaled > u32::MAX as f32 {
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
    fn rejects_empty_or_stretched_canvas() {
        assert!(Canvas2D::new(Extent2D::new(0, 720), Extent2D::new(1920, 1080)).is_err());
        assert!(Canvas2D::new(Extent2D::new(1280, 720), Extent2D::new(1920, 1200)).is_err());
    }
}
