use super::*;
use astra_media_core::{
    GlyphBitmap, GlyphBitmapFormat, OwnedPixelBuffer, RectI, SceneCommand, TextureFrame,
};
use astra_platform::{PlatformError, PlatformErrorCode};
use std::{sync::Arc, time::Instant};

pub(crate) fn profiled_elapsed_ns(enabled: bool, started: Instant) -> Result<u64, PlatformError> {
    if !enabled {
        return Ok(0);
    }
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| invalid("glyph CPU profile duration overflowed"))
}

pub(crate) fn vertex_upload_required(
    current: &[u8],
    uploaded: &[u8],
    buffer_recreated: bool,
) -> bool {
    buffer_recreated || current != uploaded
}

pub(crate) fn validate_resource_budget(
    resources: &AtlasResourceView<'_>,
) -> Result<(), PlatformError> {
    let bytes = resources.values().try_fold(0usize, |total, resource| {
        total
            .checked_add(resource.byte_len())
            .ok_or_else(|| invalid("scene resource byte count overflowed"))
    })?;
    if resources.len() > MAX_SCENE_RESOURCES || bytes > MAX_SCENE_RESOURCE_BYTES {
        let (glyph_count, texture_count) =
            resources
                .values()
                .fold((0_u64, 0_u64), |counts, resource| match resource {
                    AtlasResource::Glyph(_) => (counts.0 + 1, counts.1),
                    AtlasResource::Texture(_) => (counts.0, counts.1 + 1),
                });
        tracing::error!(
            event = "platform.wgpu.scene.resource_budget_exceeded",
            resource_count = resources.len(),
            resource_bytes = bytes,
            glyph_count,
            texture_count,
            max_resource_count = MAX_SCENE_RESOURCES,
            max_resource_bytes = MAX_SCENE_RESOURCE_BYTES,
            "retained scene resources exceeded the renderer budget"
        );
        return Err(invalid("scene resource budget was exceeded"));
    }
    Ok(())
}

pub(crate) fn validate_resource_id(resource_id: &str) -> Result<(), PlatformError> {
    if resource_id.is_empty()
        || resource_id.len() > 256
        || resource_id.contains("..")
        || !resource_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-')
        })
    {
        return Err(invalid("glyph resource id is unsafe"));
    }
    Ok(())
}

pub(crate) fn validate_scene_resource_payloads(
    commands: &[SceneCommand],
) -> Result<(), PlatformError> {
    for command in commands {
        match command {
            SceneCommand::UploadTexture { frame, .. }
            | SceneCommand::Texture { frame, .. }
            | SceneCommand::VideoFrame { frame, .. } => validate_texture(frame)?,
            SceneCommand::UploadGlyph { glyph, .. } | SceneCommand::Glyph { glyph, .. } => {
                validate_glyph(glyph)?
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn validate_glyph_metadata(glyph: &GlyphBitmap) -> Result<(), PlatformError> {
    let channels = match glyph.format {
        GlyphBitmapFormat::Alpha8 => 1usize,
        GlyphBitmapFormat::Rgba8 => 4usize,
    };
    let expected = (glyph.width as usize)
        .checked_mul(glyph.height as usize)
        .and_then(|pixels| pixels.checked_mul(channels));
    if glyph.width == 0 || glyph.height == 0 || expected != Some(glyph.pixels.len()) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "glyph dimensions or format is invalid",
        ));
    }
    Ok(())
}

pub(crate) fn validate_glyph(glyph: &GlyphBitmap) -> Result<(), PlatformError> {
    validate_glyph_metadata(glyph)?;
    glyph.validate_integrity().map_err(|_| {
        PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "glyph dimensions or format are invalid",
        )
    })
}

pub(crate) fn validate_texture_metadata(texture: &TextureFrame) -> Result<(), PlatformError> {
    let expected = (texture.width as usize)
        .checked_mul(texture.height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if texture.width == 0 || texture.height == 0 || expected != Some(texture.rgba8.len()) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "texture dimensions are invalid",
        ));
    }
    Ok(())
}

pub(crate) fn validate_texture(texture: &TextureFrame) -> Result<(), PlatformError> {
    validate_texture_metadata(texture)?;
    texture.validate_integrity().map_err(|_| {
        PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "texture dimensions or byte length are invalid",
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_texture_region(
    texture_width: u32,
    texture_height: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    rgba8: &OwnedPixelBuffer,
) -> Result<(), PlatformError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0
        || height == 0
        || x.checked_add(width)
            .is_none_or(|right| right > texture_width)
        || y.checked_add(height)
            .is_none_or(|bottom| bottom > texture_height)
        || expected != Some(rgba8.len())
    {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "texture region bounds or byte length are invalid",
        ));
    }
    Ok(())
}

pub(crate) fn updated_retained_texture(
    current: &RetainedTexture,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    rgba8: OwnedPixelBuffer,
) -> Result<RetainedTexture, PlatformError> {
    if x == 0 && y == 0 && width == current.base.width && height == current.base.height {
        let base = Arc::new(
            TextureFrame::from_buffer(current.base.width, current.base.height, rgba8).map_err(
                |error| {
                    PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        error.to_string(),
                    )
                },
            )?,
        );
        return Ok(RetainedTexture {
            base,
            patches: Vec::new(),
            patch_bytes: 0,
        });
    }
    let mut patches = current.patches.clone();
    patches.retain(|patch| {
        !(x <= patch.x
            && y <= patch.y
            && x + width >= patch.x + patch.width
            && y + height >= patch.y + patch.height)
    });
    patches.push(RetainedTexturePatch {
        x,
        y,
        width,
        height,
        rgba8,
    });
    let patch_bytes = patches.iter().try_fold(0_usize, |total, patch| {
        total
            .checked_add(patch.rgba8.len())
            .ok_or_else(|| invalid("retained texture patch byte count overflowed"))
    })?;
    if patch_bytes > current.base.rgba8.len() {
        return Err(PlatformError::new(
            PlatformErrorCode::QueueOverflow,
            "surface.present_scene",
            "retained texture patch budget requires a full texture refresh",
        ));
    }
    Ok(RetainedTexture {
        base: current.base.clone(),
        patches,
        patch_bytes,
    })
}

pub(crate) fn validate_source_rect(
    source: RectI,
    width: u32,
    height: u32,
) -> Result<(), PlatformError> {
    let right = i64::from(source.x) + i64::from(source.width);
    let bottom = i64::from(source.y) + i64::from(source.height);
    if source.x < 0
        || source.y < 0
        || source.width == 0
        || source.height == 0
        || right > i64::from(width)
        || bottom > i64::from(height)
    {
        return Err(invalid("sprite source rectangle is outside its texture"));
    }
    Ok(())
}

pub(crate) fn validate_destination(destination: RectI) -> Result<(), PlatformError> {
    if destination.width == 0 || destination.height == 0 {
        return Err(invalid("sprite destination rectangle is empty"));
    }
    Ok(())
}
