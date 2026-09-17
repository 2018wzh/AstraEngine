use super::*;
use crate::{MusicaAniArchive, MusicaSqzArchive};

impl Scene {
    pub(super) fn texture(&mut self, uri: &str) -> FamilyResult<TextureFrame> {
        if let Some(frame) = self.textures.get(uri) {
            return Ok(frame);
        }
        let bytes = read_asset(&self.archive, uri, MAX_ASSET_BYTES)?;
        let extension = uri.rsplit_once('.').map(|(_, value)| value);
        let decoded = match extension {
            Some(value) if value.eq_ignore_ascii_case("ani") => {
                let archive = MusicaAniArchive::parse(Arc::<[u8]>::from(bytes.as_ref()))
                    .map_err(core_error)?;
                let first = archive.frames().first().ok_or_else(|| {
                    error("ASTRA_EMU_MUSICA_ANI_FRAME_COUNT", "ANI contains no image")
                })?;
                validate_dimensions(first.width, first.height)?;
                Some(archive.decode_frame(0).map_err(core_error)?)
            }
            Some(value) if value.eq_ignore_ascii_case("sqz") => {
                let archive = MusicaSqzArchive::parse(Arc::<[u8]>::from(bytes.as_ref()))
                    .map_err(core_error)?;
                validate_dimensions(archive.width(), archive.height())?;
                Some(archive.decode_frame(0).map_err(core_error)?)
            }
            _ => None,
        };
        if let Some(decoded) = decoded {
            let frame = TextureFrame {
                width: decoded.width(),
                height: decoded.height(),
                rgba8: decoded.into_raw().into(),
            };
            self.textures
                .insert(uri.into(), frame.clone())
                .map_err(|_| {
                    error(
                        "ASTRA_EMU_MUSICA_IMAGE_BOUND",
                        "decoded image exceeds the texture cache budget",
                    )
                })?;
            return Ok(frame);
        }
        self.textures.decode(uri.into(), &bytes).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_IMAGE_DECODE",
                "image could not be decoded within its bounds",
            )
        })
    }
}

// Apply the scene's cache limits before proprietary decoders allocate RGBA.
fn validate_dimensions(width: u32, height: u32) -> FamilyResult<()> {
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) * 4 > MAX_IMAGE_BYTES as u64
    {
        return Err(error(
            "ASTRA_EMU_MUSICA_IMAGE_BOUND",
            "image dimensions exceed the scene budget",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
