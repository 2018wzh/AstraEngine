use super::*;
use astra_media_core::GlyphBitmapFormat;
use astra_platform::PlatformError;

#[allow(clippy::too_many_arguments)]
pub(crate) fn upload_texture_region(
    queue: &wgpu::Queue,
    encoder: Option<&mut wgpu::CommandEncoder>,
    texture: &wgpu::Texture,
    placement: AtlasPlacement,
    upload: TextureRegionUpload<'_>,
    upload_pixels: &mut Vec<u8>,
    staging_belt: &mut wgpu::util::StagingBelt,
) -> Result<u64, PlatformError> {
    let pad_left = u32::from(upload.x == 0);
    let pad_top = u32::from(upload.y == 0);
    let pad_right = u32::from(upload.x + upload.width == upload.texture_width);
    let pad_bottom = u32::from(upload.y + upload.height == upload.texture_height);
    let upload_width = upload.width + pad_left + pad_right;
    let upload_height = upload.height + pad_top + pad_bottom;
    prepare_upload_pixels(upload_pixels, upload_width, upload_height)?;
    for destination_y in 0..upload_height as usize {
        let source_y = destination_y
            .saturating_sub(pad_top as usize)
            .min(upload.height as usize - 1);
        for destination_x in 0..upload_width as usize {
            let source_x = destination_x
                .saturating_sub(pad_left as usize)
                .min(upload.width as usize - 1);
            let source = (source_y * upload.width as usize + source_x) * 4;
            let destination = (destination_y * upload_width as usize + destination_x) * 4;
            upload_pixels[destination..destination + 4]
                .copy_from_slice(&upload.rgba8[source..source + 4]);
        }
    }
    let origin = wgpu::Origin3d {
        x: placement.x + upload.x - pad_left,
        y: placement.y + upload.y - pad_top,
        z: 0,
    };
    if let Some(encoder) = encoder {
        encode_texture_upload_belt(
            encoder,
            texture,
            upload_pixels,
            upload_width,
            upload_height,
            origin,
            staging_belt,
        )?;
    } else {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            upload_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(upload_width * 4),
                rows_per_image: Some(upload_height),
            },
            wgpu::Extent3d {
                width: upload_width,
                height: upload_height,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(u64::from(upload_width) * u64::from(upload_height) * 4)
}

pub(crate) fn upload_resource_rect(
    queue: &wgpu::Queue,
    encoder: Option<&mut wgpu::CommandEncoder>,
    texture: &wgpu::Texture,
    resource: &AtlasResource,
    placement: AtlasPlacement,
    upload_pixels: &mut Vec<u8>,
    staging_belt: &mut wgpu::util::StagingBelt,
) -> Result<(), PlatformError> {
    let width = resource.width() + ATLAS_PADDING * 2;
    let height = resource.height() + ATLAS_PADDING * 2;
    prepare_upload_pixels(upload_pixels, width, height)?;
    write_padded_resource(resource, upload_pixels, width, 0, 0)?;
    let origin = wgpu::Origin3d {
        x: placement.x - ATLAS_PADDING,
        y: placement.y - ATLAS_PADDING,
        z: 0,
    };
    if let Some(encoder) = encoder {
        encode_texture_upload_belt(
            encoder,
            texture,
            upload_pixels,
            width,
            height,
            origin,
            staging_belt,
        )?;
    } else {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            upload_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(())
}

pub(crate) fn build_atlas_pixels(
    resources: &AtlasResourceView<'_>,
    atlas: &PackedAtlas,
    upload_pixels: &mut Vec<u8>,
) -> Result<(), PlatformError> {
    prepare_upload_pixels(upload_pixels, atlas.width, atlas.height)?;
    upload_pixels.fill(0);
    upload_pixels[..4].copy_from_slice(&[255; 4]);
    for (resource_id, resource) in resources.iter() {
        let placement = atlas
            .placements
            .get(resource_id)
            .ok_or_else(|| invalid("glyph atlas placement is missing"))?;
        write_padded_resource(
            resource,
            upload_pixels,
            atlas.width,
            placement.x - ATLAS_PADDING,
            placement.y - ATLAS_PADDING,
        )?;
    }
    Ok(())
}

pub(crate) fn write_padded_resource(
    resource: &AtlasResource,
    destination: &mut [u8],
    destination_width: u32,
    destination_x: u32,
    destination_y: u32,
) -> Result<(), PlatformError> {
    let source_width = resource.width() as usize;
    let source_height = resource.height() as usize;
    let destination_width = destination_width as usize;
    let destination_x = destination_x as usize;
    let destination_y = destination_y as usize;
    let padded_width = source_width + ATLAS_PADDING as usize * 2;
    let padded_height = source_height + ATLAS_PADDING as usize * 2;
    let required_rows = destination_y
        .checked_add(padded_height)
        .ok_or_else(|| invalid("glyph atlas padded row overflowed"))?;
    let required_columns = destination_x
        .checked_add(padded_width)
        .ok_or_else(|| invalid("glyph atlas padded column overflowed"))?;
    let required_bytes = required_rows
        .checked_mul(destination_width)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("glyph atlas padded destination overflowed"))?;
    if source_width == 0
        || source_height == 0
        || required_columns > destination_width
        || required_bytes > destination.len()
    {
        return Err(invalid(
            "glyph atlas padded resource is outside destination",
        ));
    }
    match resource {
        AtlasResource::Glyph(glyph) if glyph.format == GlyphBitmapFormat::Alpha8 => {
            let padding = ATLAS_PADDING as usize;
            for padded_row in 0..padded_height {
                let source_row = padded_row.saturating_sub(padding).min(source_height - 1);
                for padded_column in 0..padded_width {
                    let source_column = padded_column.saturating_sub(padding).min(source_width - 1);
                    let alpha = glyph.pixels[source_row * source_width + source_column];
                    let offset = ((destination_y + padded_row) * destination_width
                        + destination_x
                        + padded_column)
                        * 4;
                    destination[offset..offset + 4].copy_from_slice(&[alpha; 4]);
                }
            }
        }
        AtlasResource::Glyph(glyph) => write_padded_rgba_rows(
            &glyph.pixels,
            source_width,
            source_height,
            destination,
            destination_width,
            destination_x,
            destination_y,
        )?,
        AtlasResource::Texture(texture) => {
            write_padded_rgba_rows(
                &texture.base.rgba8,
                source_width,
                source_height,
                destination,
                destination_width,
                destination_x,
                destination_y,
            )?;
            for patch in &texture.patches {
                write_retained_texture_patch(
                    patch,
                    texture.base.width,
                    texture.base.height,
                    destination,
                    destination_width,
                    destination_x,
                    destination_y,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_retained_texture_patch(
    patch: &RetainedTexturePatch,
    texture_width: u32,
    texture_height: u32,
    destination: &mut [u8],
    destination_width: usize,
    destination_x: usize,
    destination_y: usize,
) -> Result<(), PlatformError> {
    let padding = ATLAS_PADDING as usize;
    let pad_left = usize::from(patch.x == 0);
    let pad_top = usize::from(patch.y == 0);
    let pad_right = usize::from(patch.x + patch.width == texture_width);
    let pad_bottom = usize::from(patch.y + patch.height == texture_height);
    let write_width = patch.width as usize + pad_left + pad_right;
    let write_height = patch.height as usize + pad_top + pad_bottom;
    for row in 0..write_height {
        let source_row = row.saturating_sub(pad_top).min(patch.height as usize - 1);
        let target_row = destination_y + padding + patch.y as usize + row - pad_top;
        for column in 0..write_width {
            let source_column = column
                .saturating_sub(pad_left)
                .min(patch.width as usize - 1);
            let target_column = destination_x + padding + patch.x as usize + column - pad_left;
            let source = (source_row * patch.width as usize + source_column) * 4;
            let target = (target_row * destination_width + target_column)
                .checked_mul(4)
                .ok_or_else(|| invalid("retained texture patch destination overflowed"))?;
            if target + 4 > destination.len() {
                return Err(invalid("retained texture patch is outside destination"));
            }
            destination[target..target + 4]
                .copy_from_slice(&patch.rgba8.as_slice()[source..source + 4]);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_padded_rgba_rows(
    source: &[u8],
    source_width: usize,
    source_height: usize,
    destination: &mut [u8],
    destination_width: usize,
    destination_x: usize,
    destination_y: usize,
) -> Result<(), PlatformError> {
    let padding = ATLAS_PADDING as usize;
    let source_row_bytes = source_width
        .checked_mul(4)
        .ok_or_else(|| invalid("glyph atlas source row overflowed"))?;
    if source.len() != source_row_bytes.saturating_mul(source_height) {
        return Err(invalid("glyph atlas RGBA resource byte count is invalid"));
    }
    for padded_row in 0..source_height + padding * 2 {
        let source_row = padded_row.saturating_sub(padding).min(source_height - 1);
        let source_start = source_row * source_row_bytes;
        let destination_start =
            ((destination_y + padded_row) * destination_width + destination_x) * 4;
        let interior_start = destination_start + padding * 4;
        destination[interior_start..interior_start + source_row_bytes]
            .copy_from_slice(&source[source_start..source_start + source_row_bytes]);
        for column in 0..padding {
            let left = destination_start + column * 4;
            destination[left..left + 4].copy_from_slice(&source[source_start..source_start + 4]);
        }
        let right_source = source_start + source_row_bytes - 4;
        let right_destination = interior_start + source_row_bytes;
        for column in 0..padding {
            let right = right_destination + column * 4;
            destination[right..right + 4].copy_from_slice(&source[right_source..right_source + 4]);
        }
    }
    Ok(())
}

pub(crate) fn prepare_upload_pixels(
    upload_pixels: &mut Vec<u8>,
    width: u32,
    height: u32,
) -> Result<(), PlatformError> {
    let required = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("glyph atlas upload byte count overflowed"))?;
    if required > MAX_ATLAS_UPLOAD_BYTES {
        return Err(invalid(
            "glyph atlas upload exceeds the reserved staging budget",
        ));
    }
    if upload_pixels.capacity() < required {
        upload_pixels
            .try_reserve_exact(required - upload_pixels.capacity())
            .map_err(|_| invalid("glyph atlas upload staging allocation failed"))?;
    }
    upload_pixels.clear();
    upload_pixels.resize(required, 0);
    Ok(())
}

pub(crate) fn upload_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pixels: &[u8],
    width: u32,
    height: u32,
    timestamp_query: Option<(&wgpu::QuerySet, u32, u32)>,
) -> Result<wgpu::Texture, PlatformError> {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra-glyph-atlas"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
    });
    if let Some((query_set, begin, end)) = timestamp_query {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra-glyph-atlas-upload-encoder"),
        });
        encoder.write_timestamp(query_set, begin);
        encode_texture_upload_once(
            device,
            &mut encoder,
            &texture,
            pixels,
            width,
            height,
            wgpu::Origin3d::ZERO,
        )?;
        encoder.write_timestamp(query_set, end);
        queue.submit([encoder.finish()]);
    } else {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(texture)
}

pub(crate) fn primed_staging_belt(device: &wgpu::Device) -> wgpu::util::StagingBelt {
    let mut belt = wgpu::util::StagingBelt::new(device.clone(), ATLAS_STAGING_CHUNK_BYTES);
    {
        let _ = belt.allocate(
            wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT)
                .expect("copy buffer alignment is non-zero"),
            wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT)
                .expect("copy buffer alignment is non-zero"),
        );
    }
    belt
}

pub(crate) fn encode_texture_upload_once(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    pixels: &[u8],
    width: u32,
    height: u32,
    origin: wgpu::Origin3d,
) -> Result<(), PlatformError> {
    let tight_row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("glyph atlas upload row overflowed"))?;
    let padded_row =
        tight_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let size = u64::from(padded_row)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("glyph atlas staging buffer size overflowed"))?;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("astra-glyph-atlas-initial-upload-staging"),
        size,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    {
        let mut mapped = staging.slice(..).get_mapped_range_mut();
        for row in 0..height as usize {
            let source = row * tight_row as usize;
            let destination = row * padded_row as usize;
            mapped
                .slice(destination..destination + tight_row as usize)
                .copy_from_slice(&pixels[source..source + tight_row as usize]);
        }
    }
    staging.unmap();
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

pub(crate) fn encode_texture_upload_belt(
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    pixels: &[u8],
    width: u32,
    height: u32,
    origin: wgpu::Origin3d,
    staging_belt: &mut wgpu::util::StagingBelt,
) -> Result<(), PlatformError> {
    let tight_row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("glyph atlas upload row overflowed"))?;
    let padded_row =
        tight_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let size = u64::from(padded_row)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("glyph atlas staging buffer size overflowed"))?;
    let size = wgpu::BufferSize::new(size)
        .ok_or_else(|| invalid("glyph atlas staging buffer cannot be empty"))?;
    let alignment = wgpu::BufferSize::new(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
        .expect("copy row alignment is non-zero");
    let staging = staging_belt.allocate(size, alignment);
    {
        let mut mapped = staging.get_mapped_range_mut();
        for row in 0..height as usize {
            let source = row * tight_row as usize;
            let destination = row * padded_row as usize;
            mapped
                .slice(destination..destination + tight_row as usize)
                .copy_from_slice(&pixels[source..source + tight_row as usize]);
        }
    }
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: staging.buffer(),
            layout: wgpu::TexelCopyBufferLayout {
                offset: staging.offset(),
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}
