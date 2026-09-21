use super::*;
use astra_emu_sdk::StageCanvas;
use astra_media_core::{Extent2D, TextureFrame};
use std::io::Cursor;

const THUMBNAIL_WIDTH: u32 = 96;
const THUMBNAIL_HEIGHT: u32 = 54;
const MAX_CAPTURE_BYTES: usize = 128 * 1024 * 1024;

fn expected_rgba_len(width: u32, height: u32) -> Option<usize> {
    usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SaveCard {
    pub timestamp: String,
    pub comment: String,
    pub thumbnail_png: Vec<u8>,
}
impl SaveCard {
    pub fn capture(
        logical_width: u32,
        logical_height: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> FamilyResult<Self> {
        let fail = || {
            error(
                "ASTRA_EMU_MUSICA_SAVE_THUMBNAIL",
                "gameplay thumbnail could not be encoded",
            )
        };
        let expected = expected_rgba_len(width, height).ok_or_else(fail)?;
        if expected > MAX_CAPTURE_BYTES || rgba.len() != expected {
            return Err(fail());
        }
        let canvas = StageCanvas::new(
            Extent2D::new(logical_width, logical_height),
            Extent2D::new(width, height),
        )
        .map_err(|_| fail())?;
        let viewport = canvas.viewport();
        let right = viewport.x.checked_add(viewport.width).ok_or_else(fail)?;
        let bottom = viewport.y.checked_add(viewport.height).ok_or_else(fail)?;
        if right > width || bottom > height {
            return Err(fail());
        }
        let frame = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(fail)?;
        let content = image::imageops::crop_imm(
            &frame,
            viewport.x,
            viewport.y,
            viewport.width,
            viewport.height,
        )
        .to_image();
        let thumbnail = image::imageops::resize(
            &content,
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            image::imageops::FilterType::Triangle,
        );
        let mut png = Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|_| fail())?;
        let now = time::OffsetDateTime::now_local().map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SAVE_TIMESTAMP",
                "local time is unavailable",
            )
        })?;
        let card = Self {
            timestamp: format!(
                "{:04}/{:02}/{:02} {:02}:{:02}",
                now.year(),
                u8::from(now.month()),
                now.day(),
                now.hour(),
                now.minute()
            ),
            comment: String::new(),
            thumbnail_png: png.into_inner(),
        };
        card.validate()?;
        Ok(card)
    }
    pub fn validate(&self) -> FamilyResult<()> {
        let invalid = || {
            error(
                "ASTRA_EMU_MUSICA_SAVE_CARD",
                "save card metadata is invalid",
            )
        };
        let b = self.timestamp.as_bytes();
        if b.len() != 16
            || b[4] != b'/'
            || b[7] != b'/'
            || b[10] != b' '
            || b[13] != b':'
            || b.iter()
                .enumerate()
                .any(|(i, b)| !matches!(i, 4 | 7 | 10 | 13) && !b.is_ascii_digit())
            || self.comment.len() > 256
            || self.comment.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        let n = |a, b| self.timestamp[a..b].parse::<u16>().map_err(|_| invalid());
        let year = n(0, 4)?;
        let month = time::Month::try_from(n(5, 7)? as u8).map_err(|_| invalid())?;
        if year == 0
            || time::Date::from_calendar_date(year as i32, month, n(8, 10)? as u8).is_err()
            || n(11, 13)? > 23
            || n(14, 16)? > 59
        {
            return Err(invalid());
        }
        self.texture()?;
        Ok(())
    }
    pub fn texture(&self) -> FamilyResult<TextureFrame> {
        let invalid = || {
            error(
                "ASTRA_EMU_MUSICA_SAVE_THUMBNAIL",
                "save thumbnail is invalid",
            )
        };
        if self.thumbnail_png.is_empty() || self.thumbnail_png.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let mut reader = image::ImageReader::with_format(
            Cursor::new(&self.thumbnail_png),
            image::ImageFormat::Png,
        );
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(THUMBNAIL_WIDTH);
        limits.max_image_height = Some(THUMBNAIL_HEIGHT);
        limits.max_alloc = Some(1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| invalid())?;
        if (image.width(), image.height()) != (THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT) {
            return Err(invalid());
        }
        Ok(TextureFrame {
            width: THUMBNAIL_WIDTH,
            height: THUMBNAIL_HEIGHT,
            rgba8: image.into_rgba8().into_raw().into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::expected_rgba_len;

    #[test]
    fn rgba_length_calculation_rejects_overflow() {
        assert_eq!(expected_rgba_len(u32::MAX, u32::MAX), None);
    }
}
