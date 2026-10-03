use super::error;
use crate::wire::FinalFrame;
use astra_emu_family_api::{FamilyResult, FrameAlpha, FrameFormat, FrameInfo, FrameView};
use astra_emu_sdk::{StageCanvas, TextureAsset};
use astra_media_core::{Extent2D, TextureFrame};

pub(super) struct Frame {
    asset: TextureAsset,
    stage: StageCanvas,
}
impl Frame {
    pub fn from_native(mut source: FinalFrame) -> FamilyResult<Self> {
        let info = source.info;
        let required = info.required_bytes().map_err(|_| {
            error(
                "ASTRA_EMU_KRKR_FRAME",
                "native frame dimensions are invalid",
            )
        })?;
        if source.pixels.len() != required {
            return Err(error(
                "ASTRA_EMU_KRKR_FRAME",
                "native frame byte length is invalid",
            ));
        }
        let logical = Extent2D::new(info.logical_width, info.logical_height);
        let stage = StageCanvas::new(logical, Extent2D::new(info.width, info.height))
            .map_err(|_| error("ASTRA_EMU_KRKR_FRAME", "native stage geometry is invalid"))?;
        let viewport = stage.viewport();
        let row = info.width as usize * 4;
        let stride = info.stride as usize;
        for y in 0..info.height {
            for (x, pixel) in source.pixels[y as usize * stride..y as usize * stride + row]
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
            {
                let outside = (x as u32) < viewport.x
                    || (x as u32) >= viewport.x + viewport.width
                    || y < viewport.y
                    || y >= viewport.y + viewport.height;
                if pixel[3] != 255 || (outside && *pixel != [0, 0, 0, 255]) {
                    return Err(error(
                        "ASTRA_EMU_KRKR_FRAME",
                        "native final frame violates opaque centered viewport",
                    ));
                }
            }
            if row != stride {
                source.pixels.copy_within(
                    y as usize * stride..y as usize * stride + row,
                    y as usize * row,
                );
            }
        }
        source.pixels.truncate(row * info.height as usize);
        let texture = TextureFrame::from_vec(info.width, info.height, source.pixels)
            .map_err(|_| error("ASTRA_EMU_KRKR_FRAME", "native texture pixels are invalid"))?;
        let asset = TextureAsset::new(texture, logical)
            .map_err(|_| error("ASTRA_EMU_KRKR_FRAME", "native texture geometry is invalid"))?;
        Ok(Self { asset, stage })
    }
    pub fn info(&self) -> FrameInfo {
        FrameInfo {
            width: self.asset.frame.width,
            height: self.asset.frame.height,
            logical_width: self.stage.logical().width,
            logical_height: self.stage.logical().height,
            stride: self.asset.frame.width * 4,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Opaque,
            },
        }
    }
    pub fn view(&self) -> FamilyResult<FrameView<'_>> {
        FrameView::from_slice(self.asset.frame.rgba8.as_slice(), self.info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> FinalFrame {
        let info = crate::wire::FrameInfo {
            width: 8,
            height: 6,
            logical_width: 4,
            logical_height: 4,
            stride: 36,
        };
        let mut pixels = vec![42; 216];
        for y in 0..6 {
            for x in 0..8 {
                pixels[y * 36 + x * 4..y * 36 + x * 4 + 4].copy_from_slice(if x == 0 || x == 7 {
                    &[0, 0, 0, 255]
                } else {
                    &[10, 20, 30, 255]
                });
            }
        }
        FinalFrame {
            identity: crate::wire::Identity {
                session: 1,
                sequence: 1,
            },
            info,
            pixels,
        }
    }
    #[test]
    fn sdk_frame_retains_physical_logical_extent_and_packs_padded_rows() {
        let frame = Frame::from_native(source()).unwrap();
        assert_eq!(frame.info().logical_width, 4);
        assert_eq!(frame.info().width, 8);
        assert_eq!(frame.info().stride, 32);
        let view = frame.view().unwrap();
        assert_eq!(view.as_slice().len(), 192);
        assert_eq!(&view.as_slice()[32..36], &[0, 0, 0, 255]);
        assert_eq!(&view.as_slice()[36..40], &[10, 20, 30, 255]);
    }
    #[test]
    fn malformed_native_opaque_or_black_border_is_rejected() {
        let mut frame = source();
        frame.pixels[0] = 1;
        assert!(Frame::from_native(frame).is_err());
        let mut frame = source();
        frame.pixels[7] = 254;
        assert!(Frame::from_native(frame).is_err());
    }
}
