use super::*;
use astra_platform::PlatformError;
use std::collections::BTreeMap;

pub(crate) fn update_gpu_atlas(
    mut context: AtlasUpdateContext<'_>,
    current: Option<&GpuAtlas>,
    old_resources: &BTreeMap<String, AtlasResource>,
    new_resources: &AtlasResourceView<'_>,
    texture_region_uploads: &[TextureRegionUpload<'_>],
    upload_pixels: &mut Vec<u8>,
) -> Result<(PreparedAtlasUpdate, u64), PlatformError> {
    let Some(current) = current else {
        let atlas = create_full_gpu_atlas(&mut context, new_resources, upload_pixels)?;
        let upload_bytes = u64::from(atlas.packed.width) * u64::from(atlas.packed.height) * 4;
        return Ok((PreparedAtlasUpdate::Replace(atlas), upload_bytes));
    };
    let mut allocator = AtlasAllocatorState::new(&current.packed);
    let mut placement_mutations = AtlasPlacementJournal::new();
    for stale_id in current
        .packed
        .placements
        .keys()
        .filter(|id| !old_resources.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>()
    {
        let placement = *current
            .packed
            .placements
            .get(&stale_id)
            .ok_or_else(|| invalid("stale resource has no atlas placement"))?;
        release_pending_atlas_slot(&mut allocator, placement)?;
        placement_mutations.insert(stale_id, None);
    }
    for resource_id in old_resources.keys() {
        if !new_resources.contains_key(resource_id) {
            let placement = *current
                .packed
                .placements
                .get(resource_id)
                .ok_or_else(|| invalid("released resource has no atlas placement"))?;
            release_pending_atlas_slot(&mut allocator, placement)?;
            placement_mutations.insert(resource_id.clone(), None);
        }
    }
    let uploads = new_resources
        .iter()
        .filter(|(id, resource)| {
            old_resources.get(*id) != Some(*resource)
                && !texture_region_uploads
                    .iter()
                    .any(|upload| upload.resource_id == id.as_str())
        })
        .collect::<Vec<_>>();
    for (resource_id, resource) in &uploads {
        // Transient ids may be reused by the next frame. Their old placement
        // was released above; consult the pending removal before reusing it.
        if current.packed.placements.contains_key(*resource_id)
            && !matches!(placement_mutations.get(*resource_id), Some(None))
        {
            continue;
        }
        let placement = match allocate_pending_atlas_slot(
            &mut allocator,
            resource.width(),
            resource.height(),
        ) {
            Ok(placement) => placement,
            Err(_error) => {
                tracing::debug!(
                    event = "platform.wgpu.atlas.repacked",
                    resource_count = new_resources.len(),
                    freed_area = allocator.freed_area,
                    previous_width = current.packed.width,
                    previous_height = current.packed.height,
                    "glyph atlas capacity or fragmentation triggered a GPU-side repack"
                );
                return repack_gpu_atlas(
                    &mut context,
                    current,
                    old_resources,
                    new_resources,
                    upload_pixels,
                );
            }
        };
        placement_mutations.insert((*resource_id).clone(), Some(placement));
    }
    let mut upload_encoder = context.timestamp_query.map(|(query_set, begin, _)| {
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra-glyph-atlas-upload-encoder"),
            });
        encoder.write_timestamp(query_set, begin);
        encoder
    });
    let mut upload_bytes = 0u64;
    for (resource_id, resource) in uploads {
        let placement = placement_mutations
            .get(resource_id)
            .and_then(|placement| *placement)
            .or_else(|| current.packed.placements.get(resource_id).copied())
            .ok_or_else(|| invalid("new atlas resource has no planned placement"))?;
        upload_resource_rect(
            context.queue,
            upload_encoder.as_mut(),
            &current.texture,
            resource,
            placement,
            upload_pixels,
            context.staging_belt,
        )?;
        upload_bytes = upload_bytes
            .checked_add(
                u64::from(resource.width() + ATLAS_PADDING * 2)
                    * u64::from(resource.height() + ATLAS_PADDING * 2)
                    * 4,
            )
            .ok_or_else(|| invalid("glyph atlas upload byte count overflowed"))?;
    }
    for upload in texture_region_uploads {
        let placement = current
            .packed
            .placements
            .get(upload.resource_id)
            .copied()
            .ok_or_else(|| invalid("texture region target has no atlas placement"))?;
        let bytes = upload_texture_region(
            context.queue,
            upload_encoder.as_mut(),
            &current.texture,
            placement,
            *upload,
            upload_pixels,
            context.staging_belt,
        )?;
        upload_bytes = upload_bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid("glyph atlas upload byte count overflowed"))?;
    }
    if let (Some((query_set, _, end)), Some(mut encoder)) =
        (context.timestamp_query, upload_encoder)
    {
        encoder.write_timestamp(query_set, end);
        context.staging_belt.finish();
        context.queue.submit([encoder.finish()]);
        context.staging_belt.recall();
    }
    Ok((
        PreparedAtlasUpdate::Mutate(PackedAtlasMutation {
            placements: placement_mutations,
            cursor_x: allocator.cursor_x,
            cursor_y: allocator.cursor_y,
            row_height: allocator.row_height,
            consumed_free_slots: allocator.consumed_free_slots,
            added_free_slots: allocator.added_free_slots,
            freed_area: allocator.freed_area,
        }),
        upload_bytes,
    ))
}

pub(crate) fn repack_gpu_atlas(
    context: &mut AtlasUpdateContext<'_>,
    current: &GpuAtlas,
    old_resources: &BTreeMap<String, AtlasResource>,
    new_resources: &AtlasResourceView<'_>,
    upload_pixels: &mut Vec<u8>,
) -> Result<(PreparedAtlasUpdate, u64), PlatformError> {
    let current_area = u64::from(current.packed.width) * u64::from(current.packed.height);
    let max_dimension = context.device.limits().max_texture_dimension_2d;
    let candidates = [
        (MIN_ATLAS_SIDE, MIN_ATLAS_SIDE),
        (2048, 2048),
        (ATLAS_SIDE, 2048),
        (ATLAS_SIDE, ATLAS_SIDE),
        (MAX_ATLAS_WIDTH, 2048),
        (MAX_ATLAS_WIDTH, ATLAS_SIDE),
    ];
    let mut packed = None;
    for (width, height) in candidates {
        let area = u64::from(width) * u64::from(height);
        if area < current_area || width > max_dimension || height > max_dimension {
            continue;
        }
        if let Some(candidate) = pack_atlas_at_extent(new_resources, width, height)? {
            packed = Some(candidate);
            break;
        }
    }
    let packed = match packed {
        Some(packed) => packed,
        None => {
            let (resource_bytes, padded_area, widest, tallest) = new_resources.values().try_fold(
                (0_u64, 0_u64, 0_u32, 0_u32),
                |(bytes, area, widest, tallest), resource| {
                    let padded_width = resource.width() + ATLAS_PADDING * 2;
                    let padded_height = resource.height() + ATLAS_PADDING * 2;
                    Ok::<_, PlatformError>((
                        bytes
                            .checked_add(resource.byte_len() as u64)
                            .ok_or_else(|| invalid("scene resource byte count overflowed"))?,
                        area.checked_add(u64::from(padded_width) * u64::from(padded_height))
                            .ok_or_else(|| invalid("glyph atlas total area overflowed"))?,
                        widest.max(padded_width),
                        tallest.max(padded_height),
                    ))
                },
            )?;
            tracing::error!(
                event = "platform.wgpu.atlas.capacity_exceeded",
                resource_count = new_resources.len(),
                resource_bytes,
                padded_area,
                widest,
                tallest,
                max_texture_dimension_2d = max_dimension,
                max_atlas_width = MAX_ATLAS_WIDTH,
                max_atlas_height = ATLAS_SIDE,
                "retained scene resources cannot be packed into the GPU atlas"
            );
            return Err(invalid("glyph atlas capacity was exceeded"));
        }
    };
    let atlas = create_empty_gpu_atlas(context, packed);
    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra-glyph-atlas-repack-encoder"),
        });
    if let Some((query_set, begin, _)) = context.timestamp_query {
        encoder.write_timestamp(query_set, begin);
    }
    let mut upload_bytes = 0u64;
    let mut staged_uploads = false;
    for (resource_id, resource) in new_resources.iter() {
        let placement = *atlas
            .packed
            .placements
            .get(resource_id)
            .ok_or_else(|| invalid("repacked atlas resource has no placement"))?;
        let copy_bytes = u64::from(resource.width() + ATLAS_PADDING * 2)
            * u64::from(resource.height() + ATLAS_PADDING * 2)
            * 4;
        if old_resources.get(resource_id) == Some(resource) {
            if let Some(old_placement) = current.packed.placements.get(resource_id) {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &current.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: old_placement.x - ATLAS_PADDING,
                            y: old_placement.y - ATLAS_PADDING,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &atlas.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: placement.x - ATLAS_PADDING,
                            y: placement.y - ATLAS_PADDING,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: resource.width() + ATLAS_PADDING * 2,
                        height: resource.height() + ATLAS_PADDING * 2,
                        depth_or_array_layers: 1,
                    },
                );
                upload_bytes = upload_bytes
                    .checked_add(copy_bytes)
                    .ok_or_else(|| invalid("glyph atlas repack byte count overflowed"))?;
                continue;
            }
        }
        upload_resource_rect(
            context.queue,
            Some(&mut encoder),
            &atlas.texture,
            resource,
            placement,
            upload_pixels,
            context.staging_belt,
        )?;
        staged_uploads = true;
        upload_bytes = upload_bytes
            .checked_add(copy_bytes)
            .ok_or_else(|| invalid("glyph atlas repack byte count overflowed"))?;
    }
    if let Some((query_set, _, end)) = context.timestamp_query {
        encoder.write_timestamp(query_set, end);
    }
    if staged_uploads {
        context.staging_belt.finish();
    }
    context.queue.submit([encoder.finish()]);
    if staged_uploads {
        context.staging_belt.recall();
    }
    Ok((PreparedAtlasUpdate::Replace(atlas), upload_bytes))
}

pub(crate) fn create_empty_gpu_atlas(
    context: &AtlasUpdateContext<'_>,
    packed: PackedAtlas,
) -> GpuAtlas {
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra-glyph-atlas"),
        size: wgpu::Extent3d {
            width: packed.width,
            height: packed.height,
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
    // Solid-color primitives sample the reserved texel at the atlas origin.
    // Every replacement atlas must establish the same invariant as the
    // initial/full atlas before any draw can observe it.
    context.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255, 255, 255, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let linear_view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ..Default::default()
    });
    let encoded_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let linear_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &linear_view,
        context.linear_sampler,
        "astra-glyph-atlas-linear-bind-group",
    );
    let nearest_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &linear_view,
        context.nearest_sampler,
        "astra-glyph-atlas-nearest-bind-group",
    );
    let encoded_linear_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &encoded_view,
        context.linear_sampler,
        "astra-glyph-atlas-encoded-linear-bind-group",
    );
    let encoded_nearest_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &encoded_view,
        context.nearest_sampler,
        "astra-glyph-atlas-encoded-nearest-bind-group",
    );
    GpuAtlas {
        texture,
        linear_bind_group,
        nearest_bind_group,
        encoded_linear_bind_group,
        encoded_nearest_bind_group,
        packed,
    }
}

pub(crate) fn create_reserved_gpu_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    linear_sampler: &wgpu::Sampler,
    nearest_sampler: &wgpu::Sampler,
    side: u32,
) -> GpuAtlas {
    let packed = PackedAtlas {
        placements: BTreeMap::new(),
        width: side,
        height: side,
        cursor_x: ATLAS_PADDING * 3,
        cursor_y: ATLAS_PADDING,
        row_height: 0,
        free_slots: Vec::new(),
        freed_area: 0,
    };
    let mut staging_belt = primed_staging_belt(device);
    let context = AtlasUpdateContext {
        device,
        queue,
        layout,
        linear_sampler,
        nearest_sampler,
        staging_belt: &mut staging_belt,
        timestamp_query: None,
    };
    create_empty_gpu_atlas(&context, packed)
}

pub(crate) fn create_full_gpu_atlas(
    context: &mut AtlasUpdateContext<'_>,
    resources: &AtlasResourceView<'_>,
    upload_pixels: &mut Vec<u8>,
) -> Result<GpuAtlas, PlatformError> {
    create_full_gpu_atlas_with_min_side(context, resources, upload_pixels, MIN_ATLAS_SIDE)
}

pub(crate) fn create_full_gpu_atlas_with_min_side(
    context: &mut AtlasUpdateContext<'_>,
    resources: &AtlasResourceView<'_>,
    upload_pixels: &mut Vec<u8>,
    minimum_side: u32,
) -> Result<GpuAtlas, PlatformError> {
    let packed = if minimum_side == MIN_ATLAS_SIDE {
        pack_atlas(resources)?
    } else {
        pack_atlas_with_min_side(resources, minimum_side)?
    };
    build_atlas_pixels(resources, &packed, upload_pixels)?;
    let texture = upload_atlas(
        context.device,
        context.queue,
        upload_pixels,
        packed.width,
        packed.height,
        context.timestamp_query,
    )?;
    upload_pixels.clear();
    upload_pixels.shrink_to(ATLAS_STAGING_CHUNK_BYTES as usize);
    let linear_view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ..Default::default()
    });
    let encoded_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let linear_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &linear_view,
        context.linear_sampler,
        "astra-glyph-atlas-linear-bind-group",
    );
    let nearest_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &linear_view,
        context.nearest_sampler,
        "astra-glyph-atlas-nearest-bind-group",
    );
    let encoded_linear_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &encoded_view,
        context.linear_sampler,
        "astra-glyph-atlas-encoded-linear-bind-group",
    );
    let encoded_nearest_bind_group = create_atlas_bind_group(
        context.device,
        context.layout,
        &encoded_view,
        context.nearest_sampler,
        "astra-glyph-atlas-encoded-nearest-bind-group",
    );
    Ok(GpuAtlas {
        texture,
        linear_bind_group,
        nearest_bind_group,
        encoded_linear_bind_group,
        encoded_nearest_bind_group,
        packed,
    })
}

pub(crate) fn create_atlas_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
