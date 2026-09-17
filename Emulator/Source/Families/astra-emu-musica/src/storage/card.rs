use super::*;
use astra_media_core::TextureFrame;
use std::io::Cursor;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SaveCard {
    pub timestamp: String,
    pub comment: String,
    pub thumbnail_png: Vec<u8>,
}
impl SaveCard {
    pub fn capture(width: u32, height: u32, rgba: &[u8]) -> FamilyResult<Self> {
        let fail = || {
            error(
                "ASTRA_EMU_MUSICA_SAVE_THUMBNAIL",
                "gameplay thumbnail could not be encoded",
            )
        };
        if (width, height) != (1280, 720) {
            return Err(fail());
        }
        let frame = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(fail)?;
        let thumbnail =
            image::imageops::resize(&frame, 96, 54, image::imageops::FilterType::Triangle);
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
        limits.max_image_width = Some(96);
        limits.max_image_height = Some(54);
        limits.max_alloc = Some(1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| invalid())?;
        if (image.width(), image.height()) != (96, 54) {
            return Err(invalid());
        }
        Ok(TextureFrame {
            width: 96,
            height: 54,
            rgba8: image.into_rgba8().into_raw().into(),
        })
    }
}
