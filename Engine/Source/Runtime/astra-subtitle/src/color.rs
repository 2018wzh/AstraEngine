use crate::{ErrorKind, Stage, SubtitleError};

/// Parsed ASS `YCbCr Matrix`, in the exact official 0.17.4 enum order.
/// libass returns unconverted RGB. Geometry never determines video colorspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum YcbcrMatrix {
    Default = 0,
    Unknown,
    None,
    Bt601Tv,
    Bt601Pc,
    Bt709Tv,
    Bt709Pc,
    Smpte240mTv,
    Smpte240mPc,
    FccTv,
    FccPc,
}

/// Actual decoded SDR video matrix/range, supplied by the media consumer.
/// `Rgb` means the video was natively RGB, not that YCbCr video was decoded
/// into an RGB texture. Native RGB video bypasses ASS color mangling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoColorSpace {
    Rgb,
    Bt601Tv,
    Bt601Pc,
    Bt709Tv,
    Bt709Pc,
    Smpte240mTv,
    Smpte240mPc,
    FccTv,
    FccPc,
}

impl YcbcrMatrix {
    pub(crate) fn from_native(value: i32) -> Result<Self, SubtitleError> {
        Ok(match value {
            0 => Self::Default,
            1 => Self::Unknown,
            2 => Self::None,
            3 => Self::Bt601Tv,
            4 => Self::Bt601Pc,
            5 => Self::Bt709Tv,
            6 => Self::Bt709Pc,
            7 => Self::Smpte240mTv,
            8 => Self::Smpte240mPc,
            9 => Self::FccTv,
            10 => Self::FccPc,
            _ => return Err(SubtitleError::new(ErrorKind::ColorSpace, Stage::Parse)),
        })
    }

    /// Apply the XySubFilter-compatible SDR mapping documented by ass_types.h:
    /// video_ycbcr_to_rgb(ass_rgb_to_header_ycbcr(rgb)). No gamma/primaries
    /// conversion is implied. HDR presentation requires an explicit consumer
    /// policy; neither method geometry nor this API supplies HDR metadata.
    /// An absent header means TV.601; an unrecognized header is an error.
    pub fn rgb_for_video(
        self,
        rgb: [u8; 3],
        video: VideoColorSpace,
    ) -> Result<[u8; 3], SubtitleError> {
        if self == Self::None || video == VideoColorSpace::Rgb {
            return Ok(rgb);
        }
        let source = match self {
            Self::Default | Self::Bt601Tv => VideoColorSpace::Bt601Tv,
            Self::Bt601Pc => VideoColorSpace::Bt601Pc,
            Self::Bt709Tv => VideoColorSpace::Bt709Tv,
            Self::Bt709Pc => VideoColorSpace::Bt709Pc,
            Self::Smpte240mTv => VideoColorSpace::Smpte240mTv,
            Self::Smpte240mPc => VideoColorSpace::Smpte240mPc,
            Self::FccTv => VideoColorSpace::FccTv,
            Self::FccPc => VideoColorSpace::FccPc,
            _ => return Err(SubtitleError::new(ErrorKind::ColorSpace, Stage::Color)),
        };
        if source == video {
            return Ok(rgb);
        }
        let (kr, kb, yscale, cscale) = source.coefficients();
        let [r, g, b] = rgb.map(|v| f64::from(v) / 255.0);
        let y = kr * r + (1.0 - kr - kb) * g + kb * b;
        let cb = (b - y) / (2.0 * (1.0 - kb));
        let cr = (r - y) / (2.0 * (1.0 - kr));
        let ycode = y * yscale + if yscale == 219.0 { 16.0 } else { 0.0 };
        let cbcode = cb * cscale;
        let crcode = cr * cscale;
        let (kr, kb, yscale, cscale) = video.coefficients();
        let y = (ycode - if yscale == 219.0 { 16.0 } else { 0.0 }) / yscale;
        let cb = cbcode / cscale;
        let cr = crcode / cscale;
        let r = y + 2.0 * (1.0 - kr) * cr;
        let b = y + 2.0 * (1.0 - kb) * cb;
        let g = (y - kr * r - kb * b) / (1.0 - kr - kb);
        Ok([r, g, b].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
    }
}

impl VideoColorSpace {
    fn coefficients(self) -> (f64, f64, f64, f64) {
        let (kr, kb, full) = match self {
            Self::Bt601Tv => (0.299, 0.114, false),
            Self::Bt601Pc => (0.299, 0.114, true),
            Self::Bt709Tv => (0.2126, 0.0722, false),
            Self::Bt709Pc => (0.2126, 0.0722, true),
            Self::Smpte240mTv => (0.212, 0.087, false),
            Self::Smpte240mPc => (0.212, 0.087, true),
            Self::FccTv => (0.30, 0.11, false),
            Self::FccPc => (0.30, 0.11, true),
            Self::Rgb => unreachable!("native RGB bypasses matrix conversion"),
        };
        (
            kr,
            kb,
            if full { 255.0 } else { 219.0 },
            if full { 255.0 } else { 224.0 },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_matrix_and_range_not_resolution() {
        assert_eq!(
            YcbcrMatrix::None.rgb_for_video([255, 0, 0], VideoColorSpace::Bt709Tv),
            Ok([255, 0, 0])
        );
        assert_eq!(
            YcbcrMatrix::Bt601Tv.rgb_for_video([255, 0, 0], VideoColorSpace::Bt709Tv),
            Ok([255, 25, 0])
        );
        assert_eq!(
            YcbcrMatrix::Default.rgb_for_video([0, 255, 0], VideoColorSpace::Bt709Tv),
            // Exact BT.601/709 coefficients give green=215.488166..., not 216.
            Ok([0, 215, 0])
        );
        assert_eq!(
            YcbcrMatrix::Bt601Tv.rgb_for_video([0, 0, 0], VideoColorSpace::Bt601Pc),
            Ok([16, 16, 16])
        );
        assert_eq!(
            YcbcrMatrix::Bt601Tv.rgb_for_video([255, 255, 255], VideoColorSpace::Bt601Pc),
            Ok([235, 235, 235])
        );
        assert_eq!(
            YcbcrMatrix::Unknown.rgb_for_video([1, 2, 3], VideoColorSpace::Rgb),
            Ok([1, 2, 3])
        );
        assert!(YcbcrMatrix::Unknown
            .rgb_for_video([1, 2, 3], VideoColorSpace::Bt601Tv)
            .is_err());
        for matrix in [
            YcbcrMatrix::Bt601Tv,
            YcbcrMatrix::Bt709Pc,
            YcbcrMatrix::FccTv,
            YcbcrMatrix::Smpte240mPc,
        ] {
            assert_eq!(
                matrix.rgb_for_video([31, 127, 242], VideoColorSpace::Rgb),
                Ok([31, 127, 242])
            );
        }
    }
}
