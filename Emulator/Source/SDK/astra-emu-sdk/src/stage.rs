use astra_media_core::{Canvas2D, Extent2D, TextureFrame};

use crate::CoreError;

/// EMU semantic wrapper around the engine's product-neutral canvas mapping.
/// The logical extent is the legacy game's coordinate space; raster is the
/// Family's actual render target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageCanvas {
    inner: Canvas2D,
}

impl StageCanvas {
    pub fn new(logical: Extent2D, raster: Extent2D) -> Result<Self, CoreError> {
        Canvas2D::new(logical, raster)
            .map(|inner| Self { inner })
            .map_err(|error| CoreError::invalid("ASTRA_EMU_SDK_STAGE_CANVAS", error.to_string()))
    }

    pub fn logical(self) -> Extent2D {
        self.inner.logical
    }

    pub fn raster(self) -> Extent2D {
        self.inner.raster
    }

    pub fn scale(self) -> f32 {
        self.inner.scale_x()
    }

    pub fn logical_to_raster_transform(self) -> astra_media_core::Transform2D {
        self.inner.logical_to_raster_transform()
    }

    pub fn raster_to_logical_transform(self) -> astra_media_core::Transform2D {
        self.inner.raster_to_logical_transform()
    }

    pub fn logical_to_raster_point(self, point: [f32; 2]) -> Result<[f32; 2], CoreError> {
        self.inner
            .logical_to_raster_point(point)
            .map_err(|error| CoreError::invalid("ASTRA_EMU_SDK_STAGE_POINT", error.to_string()))
    }

    pub fn raster_to_logical_point(self, point: [f32; 2]) -> Result<[f32; 2], CoreError> {
        self.inner
            .raster_to_logical_point(point)
            .map_err(|error| CoreError::invalid("ASTRA_EMU_SDK_STAGE_POINT", error.to_string()))
    }
}

/// Decoded pixels plus the geometry they occupy in legacy logical coordinates.
/// `TextureFrame` remains the physical pixel contract; this metadata must be
/// supplied explicitly when a replacement has a different density.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureAsset {
    pub frame: TextureFrame,
    pub logical_extent: Extent2D,
    /// Logical destination origin carried by formats such as ANI. It is kept
    /// separate from physical pixels so a replacement can preserve geometry.
    pub logical_origin: [i32; 2],
}

impl TextureAsset {
    pub fn new(frame: TextureFrame, logical_extent: Extent2D) -> Result<Self, CoreError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(CoreError::invalid(
                "ASTRA_EMU_SDK_TEXTURE_ASSET",
                "physical texture extent must be non-empty",
            ));
        }
        logical_extent.validate().map_err(|error| {
            CoreError::invalid("ASTRA_EMU_SDK_TEXTURE_ASSET", error.to_string())
        })?;
        Ok(Self {
            frame,
            logical_extent,
            logical_origin: [0, 0],
        })
    }

    pub fn from_native_frame(frame: TextureFrame) -> Result<Self, CoreError> {
        let logical_extent = Extent2D::new(frame.width, frame.height);
        Self::new(frame, logical_extent)
    }

    pub fn with_origin(mut self, logical_origin: [i32; 2]) -> Self {
        self.logical_origin = logical_origin;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_asset_keeps_logical_extent_separate_from_physical_pixels() {
        let frame = TextureFrame::from_vec(192, 108, vec![255; 192 * 108 * 4])
            .expect("physical texture is valid");
        let asset = TextureAsset::new(frame, Extent2D::new(128, 72))
            .expect("logical asset extent is valid");
        assert_eq!(asset.frame.width, 192);
        assert_eq!(asset.frame.height, 108);
        assert_eq!(asset.logical_extent, Extent2D::new(128, 72));
        assert_eq!(asset.logical_origin, [0, 0]);
    }

    #[test]
    fn stage_canvas_round_trips_letterbox_stage_points() {
        let canvas = StageCanvas::new(Extent2D::new(1280, 720), Extent2D::new(1920, 1080))
            .expect("16:9 stage canvas");
        assert_eq!(
            canvas
                .logical_to_raster_point([640.0, 360.0])
                .expect("logical point maps to raster"),
            [960.0, 540.0]
        );
        assert_eq!(
            canvas
                .raster_to_logical_point([960.0, 540.0])
                .expect("raster point maps to logical"),
            [640.0, 360.0]
        );
        assert!(StageCanvas::new(Extent2D::new(1280, 720), Extent2D::new(1920, 1200)).is_err());
    }
}
