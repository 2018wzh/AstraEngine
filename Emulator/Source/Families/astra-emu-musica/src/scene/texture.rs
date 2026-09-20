use super::*;
use crate::{profile::MusicaTextureOverrides, MusicaAniArchive, MusicaSqzArchive};
use astra_emu_sdk::ArchiveNodeKind;
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct TextureSourceGeometry {
    pub(super) logical_extent: Extent2D,
    pub(super) logical_origin: [i32; 2],
    kind: TextureSourceKind,
}

#[derive(Clone, Copy, Debug)]
enum TextureSourceKind {
    Png,
    Ani { frame_count: usize },
    Sqz,
    Other,
}

struct NativeTexture {
    frame: TextureFrame,
    geometry: TextureSourceGeometry,
}

pub(super) fn validate_texture_overrides(
    archive: &MusicaMountedVfs,
    overrides: &MusicaTextureOverrides,
) -> FamilyResult<()> {
    for (source, replacement) in overrides {
        let stat = archive.stat(source).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE",
                "texture override source is missing or invalid",
            )
        })?;
        if stat.kind != ArchiveNodeKind::File {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_SOURCE",
                "texture override source is not a file",
            ));
        }
        let source_kind = source_kind(source);
        match source_kind {
            TextureSourceKind::Png => {}
            TextureSourceKind::Ani { .. } => {
                let bytes = read_asset(archive, source, MAX_ASSET_BYTES)?;
                let ani = MusicaAniArchive::parse(Arc::<[u8]>::from(bytes.as_ref()))
                    .map_err(core_error)?;
                if ani.frames().len() != 1 {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_ANI_FRAMES",
                        "only single-frame ANI sources support PNG replacement",
                    ));
                }
                if let Some(frame) = ani.frames().first() {
                    validate_dimensions(frame.width, frame.height)?;
                }
            }
            TextureSourceKind::Sqz | TextureSourceKind::Other => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT",
                    "texture override source format is unsupported",
                ));
            }
        }
        validate_replacement_file(replacement)?;
    }
    Ok(())
}

fn validate_replacement_file(path: &Path) -> FamilyResult<()> {
    let metadata = fs::metadata(path).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
            "texture replacement path is missing or unsafe",
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ASSET_BYTES {
        return Err(error(
            "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_BOUND",
            "texture replacement exceeds the supported size",
        ));
    }
    Ok(())
}

impl Scene {
    fn native_texture(&mut self, uri: &str) -> FamilyResult<NativeTexture> {
        let cache_key = native_cache_key(uri);
        if let (Some(frame), Some(geometry)) = (
            self.textures.get(&cache_key),
            self.texture_sources.get(uri).copied(),
        ) {
            return Ok(NativeTexture { frame, geometry });
        }
        let bytes = read_asset(&self.archive, uri, MAX_ASSET_BYTES)?;
        let extension = uri.rsplit_once('.').map(|(_, value)| value);
        let (frame, kind, origin) = match extension {
            Some(value) if value.eq_ignore_ascii_case("ani") => {
                let archive = MusicaAniArchive::parse(Arc::<[u8]>::from(bytes.as_ref()))
                    .map_err(core_error)?;
                let first = archive.frames().first().ok_or_else(|| {
                    error("ASTRA_EMU_MUSICA_ANI_FRAME_COUNT", "ANI contains no image")
                })?;
                validate_dimensions(first.width, first.height)?;
                (
                    frame_from_image(archive.decode_frame(0).map_err(core_error)?),
                    TextureSourceKind::Ani {
                        frame_count: archive.frames().len(),
                    },
                    [i32::from(first.offset_x), i32::from(first.offset_y)],
                )
            }
            Some(value) if value.eq_ignore_ascii_case("sqz") => {
                let archive = MusicaSqzArchive::parse(Arc::<[u8]>::from(bytes.as_ref()))
                    .map_err(core_error)?;
                validate_dimensions(archive.width(), archive.height())?;
                (
                    frame_from_image(archive.decode_frame(0).map_err(core_error)?),
                    TextureSourceKind::Sqz,
                    [0, 0],
                )
            }
            _ => (
                self.textures
                    .decode(cache_key.clone(), &bytes)
                    .map_err(|_| {
                        error(
                            "ASTRA_EMU_MUSICA_IMAGE_DECODE",
                            "image could not be decoded within its bounds",
                        )
                    })?,
                if extension.is_some_and(|value| value.eq_ignore_ascii_case("png")) {
                    TextureSourceKind::Png
                } else {
                    TextureSourceKind::Other
                },
                [0, 0],
            ),
        };
        if !matches!(kind, TextureSourceKind::Other) {
            self.textures
                .insert(cache_key, frame.clone())
                .map_err(|_| {
                    error(
                        "ASTRA_EMU_MUSICA_IMAGE_BOUND",
                        "decoded image exceeds the texture cache budget",
                    )
                })?;
        }
        let geometry = TextureSourceGeometry {
            logical_extent: Extent2D::new(frame.width, frame.height),
            logical_origin: origin,
            kind,
        };
        self.texture_sources.put(uri.to_owned(), geometry);
        Ok(NativeTexture { frame, geometry })
    }

    fn replacement_texture(&mut self, uri: &str, path: &Path) -> FamilyResult<TextureFrame> {
        let cache_key = replacement_cache_key(uri);
        if let Some(frame) = self.textures.get(&cache_key) {
            return Ok(frame);
        }
        validate_replacement_file(path)?;
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_PATH",
                    "texture replacement path is missing or unsafe",
                )
            })?
            .take(MAX_ASSET_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_READ",
                    "texture replacement could not be read",
                )
            })?;
        if bytes.len() as u64 > MAX_ASSET_BYTES {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_BOUND",
                "texture replacement exceeds the supported size",
            ));
        }
        self.textures.decode(cache_key, &bytes).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_DECODE",
                "texture replacement PNG could not be decoded",
            )
        })
    }

    pub(super) fn texture_asset(&mut self, uri: &str) -> FamilyResult<TextureAsset> {
        let native = self.native_texture(uri)?;
        let frame = if let Some(path) = self.texture_overrides.get(uri).cloned() {
            if !matches!(
                native.geometry.kind,
                TextureSourceKind::Png | TextureSourceKind::Ani { frame_count: 1 }
            ) {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TEXTURE_OVERRIDE_FORMAT",
                    "texture override source format is unsupported",
                ));
            }
            self.replacement_texture(uri, &path)?
        } else {
            native.frame
        };
        TextureAsset::new(frame, native.geometry.logical_extent)
            .map(|asset| asset.with_origin(native.geometry.logical_origin))
            .map_err(core_error)
    }
}

fn source_kind(uri: &str) -> TextureSourceKind {
    match uri.rsplit_once('.').map(|(_, value)| value) {
        Some(value) if value.eq_ignore_ascii_case("png") => TextureSourceKind::Png,
        Some(value) if value.eq_ignore_ascii_case("ani") => {
            TextureSourceKind::Ani { frame_count: 0 }
        }
        Some(value) if value.eq_ignore_ascii_case("sqz") => TextureSourceKind::Sqz,
        _ => TextureSourceKind::Other,
    }
}

fn native_cache_key(uri: &str) -> String {
    format!("native:{uri}")
}

fn replacement_cache_key(uri: &str) -> String {
    format!("override:{uri}")
}

fn frame_from_image(image: image::RgbaImage) -> TextureFrame {
    TextureFrame {
        width: image.width(),
        height: image.height(),
        rgba8: image.into_raw().into(),
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
