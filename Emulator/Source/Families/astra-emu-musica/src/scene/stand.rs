use super::*;
use crate::MusicaStandLayer;
use std::io::Cursor;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Offsets {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

fn invalid_metadata() -> FamilyError {
    error(
        "ASTRA_EMU_MUSICA_PNG_METADATA",
        "invalid native PNG geometry metadata",
    )
}

impl Offsets {
    fn read(bytes: &[u8]) -> FamilyResult<Self> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_limits(png::Limits { bytes: 1024 * 1024 });
        let reader = decoder.read_info().map_err(|_| invalid_metadata())?;
        let info = reader.info();
        let mut values = [None; 4];
        let index = |key: &str| match key {
            "ol" => Some(0),
            "ot" => Some(1),
            "or" => Some(2),
            "ob" => Some(3),
            _ => None,
        };
        let mut insert = |i: usize, value: &str| -> FamilyResult<()> {
            if value.len() > 32 || values[i].is_some() {
                return Err(invalid_metadata());
            }
            values[i] = Some(
                value
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid_metadata())?,
            );
            Ok(())
        };
        for chunk in &info.uncompressed_latin1_text {
            if let Some(i) = index(&chunk.keyword) {
                insert(i, &chunk.text)?;
            }
        }
        for chunk in &info.compressed_latin1_text {
            if let Some(i) = index(&chunk.keyword) {
                let mut chunk = chunk.clone();
                chunk
                    .decompress_text_with_limit(32)
                    .map_err(|_| invalid_metadata())?;
                insert(i, &chunk.get_text().map_err(|_| invalid_metadata())?)?;
            }
        }
        for chunk in &info.utf8_text {
            if let Some(i) = index(&chunk.keyword) {
                let mut chunk = chunk.clone();
                chunk
                    .decompress_text_with_limit(32)
                    .map_err(|_| invalid_metadata())?;
                insert(i, &chunk.get_text().map_err(|_| invalid_metadata())?)?;
            }
        }
        Ok(Self {
            left: values[0].unwrap_or(0),
            top: values[1].unwrap_or(0),
            right: values[2].unwrap_or(0),
            bottom: values[3].unwrap_or(0),
        })
    }

    // Native default origin: center horizontally and align the visible image
    // bottom to the viewport. `ot` is retained metadata, not a screen offset.
    fn placement(
        self,
        width: u32,
        height: u32,
        viewport_height: u32,
        position: i32,
        parameter: i32,
    ) -> FamilyResult<(RectI, RectI)> {
        let invalid = || {
            error(
                "ASTRA_EMU_MUSICA_STAND_GEOMETRY",
                "native stand geometry is out of bounds",
            )
        };
        let full_width = i64::from(width) + i64::from(self.left) + i64::from(self.right);
        let removed = (i64::from(parameter) - i64::from(self.bottom)).max(0);
        if width == 0 || height == 0 || full_width <= 0 || removed >= i64::from(height) {
            return Err(invalid());
        }
        let visible_height = u32::try_from(i64::from(height) - removed).map_err(|_| invalid())?;
        let x = i32::try_from(i64::from(position) - full_width / 2 + i64::from(self.left))
            .map_err(|_| invalid())?;
        let y = i32::try_from(i64::from(viewport_height) - i64::from(visible_height))
            .map_err(|_| invalid())?;
        Ok((
            RectI {
                x: 0,
                y: 0,
                width,
                height: visible_height,
            },
            RectI {
                x,
                y,
                width,
                height: visible_height,
            },
        ))
    }
}

impl Scene {
    pub(super) fn stand(
        &mut self,
        commands: &mut Vec<SceneCommand>,
        stand: &MusicaStandLayer,
    ) -> FamilyResult<()> {
        let uri = &stand.resource_uri;
        let offsets = if let Some(offsets) = self.stand_offsets.get(uri) {
            *offsets
        } else {
            let (stem, extension) = uri.rsplit_once('.').ok_or_else(invalid_metadata)?;
            if !extension.eq_ignore_ascii_case("png") {
                return Err(invalid_metadata());
            }
            match self.archive.stat(&format!("{stem}.sqz")) {
                Ok(_) => {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_STAND_ANIMATION",
                        "native SQZ stand animation is not implemented",
                    ))
                }
                Err(cause) if cause.code() == "ASTRA_EMU_VFS_NOT_FOUND" => {}
                Err(cause) => return Err(core_error(cause)),
            }
            let bytes = read_asset(&self.archive, uri, MAX_ASSET_BYTES)?;
            let offsets = Offsets::read(&bytes)?;
            self.stand_offsets.put(uri.clone(), offsets);
            offsets
        };
        let asset = self.texture_asset(uri)?;
        let (_, destination) = offsets.placement(
            asset.logical_extent.width,
            asset.logical_extent.height,
            self.height,
            stand.position,
            stand.resource_parameter,
        )?;
        commands.push(SceneCommand::PushClip { rect: destination });
        commands.push(SceneCommand::Texture {
            id: format!("stand:{}", commands.len()),
            destination: RectI {
                height: asset.logical_extent.height,
                ..destination
            },
            frame: asset.frame,
            opacity: 1.0,
            blend: BlendMode::Alpha,
        });
        commands.push(SceneCommand::PopClip);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
