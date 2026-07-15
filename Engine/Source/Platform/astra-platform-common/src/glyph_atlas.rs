use std::collections::{BTreeMap, BTreeSet};

use astra_media_core::{
    BlendMode, GlyphBitmap, GlyphBitmapFormat, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand,
    TextureFrame,
};
use astra_platform::{PlatformError, PlatformErrorCode, SceneFrame};
use wgpu::util::DeviceExt;

const ATLAS_SIDE: u32 = 4096;
const ATLAS_PADDING: u32 = 1;
const MAX_GLYPH_RESOURCES: usize = 65_536;
const MAX_GLYPH_BYTES: usize = 64 * 1024 * 1024;
const VERTEX_STRIDE: wgpu::BufferAddress = 32;

pub(super) struct WgpuGlyphAtlasRenderer {
    resources: BTreeMap<String, AtlasResource>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
}

pub(super) struct PreparedGlyphFrame {
    pub(super) texture: wgpu::Texture,
    next_resources: BTreeMap<String, AtlasResource>,
}

#[derive(Clone, PartialEq, Eq)]
enum AtlasResource {
    Glyph(GlyphBitmap),
    Texture(TextureFrame),
}

impl AtlasResource {
    fn width(&self) -> u32 {
        match self {
            Self::Glyph(value) => value.width,
            Self::Texture(value) => value.width,
        }
    }

    fn height(&self) -> u32 {
        match self {
            Self::Glyph(value) => value.height,
            Self::Texture(value) => value.height,
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Glyph(value) => value.pixels.len(),
            Self::Texture(value) => value.rgba8.len(),
        }
    }
}

struct AtlasPlacement {
    x: u32,
    y: u32,
}

struct PackedAtlas {
    placements: BTreeMap<String, AtlasPlacement>,
    width: u32,
    height: u32,
}

enum QuadSource {
    Resource { resource_id: String, source: RectI },
    White,
}

struct DrawQuad {
    source: QuadSource,
    destination: RectI,
}

struct DrawRun {
    quads: Vec<DrawQuad>,
    rgba: [u8; 4],
    opacity: f32,
    clip: RectI,
}

struct MeshRun {
    vertices: Vec<MeshVertex2D>,
    indices: Vec<u32>,
    texture_id: Option<String>,
    opacity: f32,
    clip: RectI,
}

enum DrawPrimitive {
    Quads(DrawRun),
    Mesh(MeshRun),
}

struct DrawBatch {
    first_vertex: u32,
    vertex_count: u32,
    clip: RectI,
}

impl WgpuGlyphAtlasRenderer {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let (layout, sampler, pipeline) = create_pipeline(device);
        Self {
            resources: BTreeMap::new(),
            layout,
            sampler,
            pipeline,
        }
    }

    pub(super) fn recover(&mut self, device: &wgpu::Device) {
        let (layout, sampler, pipeline) = create_pipeline(device);
        self.layout = layout;
        self.sampler = sampler;
        self.pipeline = pipeline;
    }

    pub(super) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
    ) -> Result<PreparedGlyphFrame, PlatformError> {
        let (texture, next_resources) = self.render_internal(device, queue, frame, true)?;
        Ok(PreparedGlyphFrame {
            texture,
            next_resources,
        })
    }

    pub(super) fn render_retained(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
    ) -> Result<wgpu::Texture, PlatformError> {
        self.render_internal(device, queue, frame, false)
            .map(|(texture, _)| texture)
    }

    pub(super) fn commit(&mut self, prepared: PreparedGlyphFrame) -> wgpu::Texture {
        self.resources = prepared.next_resources;
        prepared.texture
    }

    fn render_internal(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        apply_mutations: bool,
    ) -> Result<(wgpu::Texture, BTreeMap<String, AtlasResource>), PlatformError> {
        let mut resources = self.resources.clone();
        let mut draw_runs = Vec::new();
        let mut clip_stack = Vec::new();
        let mut run_ids = BTreeSet::new();
        let mut drawn_resources = BTreeSet::new();
        for command in &frame.commands {
            match command {
                SceneCommand::UploadTexture { resource_id, frame } => {
                    validate_resource_id(resource_id)?;
                    validate_texture(frame)?;
                    if apply_mutations {
                        if resources.contains_key(resource_id) {
                            return Err(invalid("texture upload repeats a live resource id"));
                        }
                        resources
                            .insert(resource_id.clone(), AtlasResource::Texture(frame.clone()));
                    } else if resources.get(resource_id)
                        != Some(&AtlasResource::Texture(frame.clone()))
                    {
                        return Err(invalid(
                            "retained texture resource does not match the recovery frame",
                        ));
                    }
                }
                SceneCommand::UploadGlyph { resource_id, glyph } => {
                    validate_resource_id(resource_id)?;
                    validate_glyph(glyph)?;
                    if apply_mutations {
                        if resources.contains_key(resource_id) {
                            return Err(invalid("glyph upload repeats a live resource id"));
                        }
                        resources.insert(resource_id.clone(), AtlasResource::Glyph(glyph.clone()));
                    } else if resources.get(resource_id)
                        != Some(&AtlasResource::Glyph(glyph.clone()))
                    {
                        return Err(invalid(
                            "retained glyph resource does not match the recovery frame",
                        ));
                    }
                }
                SceneCommand::ReleaseResource { resource_id } => {
                    validate_resource_id(resource_id)?;
                    if apply_mutations {
                        if drawn_resources.contains(resource_id) {
                            return Err(invalid(
                                "glyph resource cannot be released after use in the same frame",
                            ));
                        }
                        if resources.remove(resource_id).is_none() {
                            return Err(invalid("glyph release references an unknown resource"));
                        }
                    } else if resources.contains_key(resource_id) {
                        return Err(invalid(
                            "retained glyph release was not committed before recovery",
                        ));
                    }
                }
                SceneCommand::PushClip { rect } => {
                    let clip = intersect_clip(
                        clip_stack.last().copied(),
                        *rect,
                        frame.width,
                        frame.height,
                    )?;
                    clip_stack.push(clip);
                }
                SceneCommand::PopClip => {
                    if clip_stack.pop().is_none() {
                        return Err(invalid("glyph clip stack underflowed"));
                    }
                }
                SceneCommand::GlyphRun {
                    id,
                    glyphs,
                    rgba,
                    opacity,
                    blend,
                } => {
                    if id.is_empty()
                        || id.len() > 256
                        || !run_ids.insert(id.clone())
                        || !opacity.is_finite()
                        || !(0.0..=1.0).contains(opacity)
                        || *blend != BlendMode::Alpha
                    {
                        return Err(invalid(
                            "glyph run identity, opacity, or blend mode is invalid",
                        ));
                    }
                    let mut quads = Vec::with_capacity(glyphs.len());
                    for glyph in glyphs {
                        let bitmap = resources.get(&glyph.resource_id).ok_or_else(|| {
                            invalid("glyph run references a resource that is not live")
                        })?;
                        let AtlasResource::Glyph(bitmap) = bitmap else {
                            return Err(invalid("glyph run references a non-glyph resource"));
                        };
                        quads.push(DrawQuad {
                            source: QuadSource::Resource {
                                resource_id: glyph.resource_id.clone(),
                                source: RectI::new(0, 0, bitmap.width, bitmap.height),
                            },
                            destination: RectI::new(glyph.x, glyph.y, bitmap.width, bitmap.height),
                        });
                        drawn_resources.insert(glyph.resource_id.clone());
                    }
                    draw_runs.push(DrawPrimitive::Quads(DrawRun {
                        quads,
                        rgba: *rgba,
                        opacity: *opacity,
                        clip: clip_stack.last().copied().unwrap_or(RectI::new(
                            0,
                            0,
                            frame.width,
                            frame.height,
                        )),
                    }));
                }
                SceneCommand::Sprite {
                    id,
                    texture_id,
                    source,
                    destination,
                    opacity,
                    blend,
                } => {
                    if id.is_empty()
                        || id.len() > 256
                        || !run_ids.insert(id.clone())
                        || !opacity.is_finite()
                        || !(0.0..=1.0).contains(opacity)
                        || *blend != BlendMode::Alpha
                    {
                        return Err(invalid(
                            "sprite identity, opacity, or blend mode is invalid",
                        ));
                    }
                    let texture = resources
                        .get(texture_id)
                        .ok_or_else(|| invalid("sprite references a resource that is not live"))?;
                    let AtlasResource::Texture(texture) = texture else {
                        return Err(invalid("sprite references a non-texture resource"));
                    };
                    let source = source.unwrap_or(RectI::new(0, 0, texture.width, texture.height));
                    validate_source_rect(source, texture.width, texture.height)?;
                    validate_destination(*destination)?;
                    drawn_resources.insert(texture_id.clone());
                    draw_runs.push(DrawPrimitive::Quads(DrawRun {
                        quads: vec![DrawQuad {
                            source: QuadSource::Resource {
                                resource_id: texture_id.clone(),
                                source,
                            },
                            destination: *destination,
                        }],
                        rgba: [255; 4],
                        opacity: *opacity,
                        clip: clip_stack.last().copied().unwrap_or(RectI::new(
                            0,
                            0,
                            frame.width,
                            frame.height,
                        )),
                    }));
                }
                SceneCommand::Rect {
                    id,
                    x,
                    y,
                    width,
                    height,
                    rgba,
                } => {
                    if id.is_empty()
                        || id.len() > 256
                        || !run_ids.insert(id.clone())
                        || *width == 0
                        || *height == 0
                    {
                        return Err(invalid("rectangle identity or dimensions are invalid"));
                    }
                    draw_runs.push(DrawPrimitive::Quads(DrawRun {
                        quads: vec![DrawQuad {
                            source: QuadSource::White,
                            destination: RectI::new(*x as i32, *y as i32, *width, *height),
                        }],
                        rgba: *rgba,
                        opacity: 1.0,
                        clip: clip_stack.last().copied().unwrap_or(RectI::new(
                            0,
                            0,
                            frame.width,
                            frame.height,
                        )),
                    }));
                }
                SceneCommand::Mesh2D {
                    id,
                    vertices,
                    indices,
                    material,
                    texture_id,
                    opacity,
                    blend,
                } => {
                    if id.is_empty()
                        || id.len() > 256
                        || !run_ids.insert(id.clone())
                        || !opacity.is_finite()
                        || !(0.0..=1.0).contains(opacity)
                        || *blend != BlendMode::Alpha
                        || vertices.is_empty()
                        || indices.is_empty()
                        || indices.len() % 3 != 0
                    {
                        return Err(invalid(
                            "mesh identity, topology, opacity, or blend is invalid",
                        ));
                    }
                    if vertices.len() > 250_000
                        || indices.len() > 750_000
                        || indices
                            .iter()
                            .any(|index| *index as usize >= vertices.len())
                        || vertices.iter().any(|vertex| {
                            !vertex.position.into_iter().all(f32::is_finite)
                                || !vertex.uv.into_iter().all(f32::is_finite)
                                || vertex.premultiplied_rgba[..3]
                                    .iter()
                                    .any(|channel| *channel > vertex.premultiplied_rgba[3])
                        })
                    {
                        return Err(invalid("mesh vertex or index payload is invalid"));
                    }
                    let resolved_texture = match (material, texture_id) {
                        (MeshMaterial2D::Solid, None) => None,
                        (
                            MeshMaterial2D::ColorTexture | MeshMaterial2D::GlyphMask,
                            Some(resource_id),
                        ) => {
                            match resources.get(resource_id) {
                                Some(AtlasResource::Texture(_)) => {}
                                Some(AtlasResource::Glyph(_)) => {
                                    return Err(invalid(
                                        "mesh references a glyph resource as texture",
                                    ));
                                }
                                None => {
                                    return Err(invalid(
                                        "mesh references a resource that is not live",
                                    ))
                                }
                            }
                            drawn_resources.insert(resource_id.clone());
                            Some(resource_id.clone())
                        }
                        _ => return Err(invalid("mesh material and texture binding mismatch")),
                    };
                    draw_runs.push(DrawPrimitive::Mesh(MeshRun {
                        vertices: vertices.clone(),
                        indices: indices.clone(),
                        texture_id: resolved_texture,
                        opacity: *opacity,
                        clip: clip_stack.last().copied().unwrap_or(RectI::new(
                            0,
                            0,
                            frame.width,
                            frame.height,
                        )),
                    }));
                }
                _ => {
                    return Err(invalid(
                        "GPU scene pass received an unsupported scene command",
                    ))
                }
            }
        }
        if !clip_stack.is_empty() {
            return Err(invalid("glyph clip stack is not balanced"));
        }
        validate_resource_budget(&resources)?;

        let packed_atlas = pack_atlas(&resources)?;
        let atlas_pixels = build_atlas_pixels(&resources, &packed_atlas)?;
        let atlas = upload_atlas(
            device,
            queue,
            &atlas_pixels,
            packed_atlas.width,
            packed_atlas.height,
        );
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-glyph-atlas-bind-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let (vertex_bytes, batches) = build_vertices(frame, &draw_runs, &packed_atlas, &resources)?;
        let vertex_buffer = (!vertex_bytes.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("astra-glyph-vertex-buffer"),
                contents: &vertex_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("astra-glyph-output"),
            size: wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra-glyph-render-encoder"),
        });
        {
            let clear = frame.clear_rgba;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra-glyph-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(srgb_byte_to_linear(clear[0])),
                            g: f64::from(srgb_byte_to_linear(clear[1])),
                            b: f64::from(srgb_byte_to_linear(clear[2])),
                            a: f64::from(clear[3]) / 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vertex_buffer) = &vertex_buffer {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for batch in &batches {
                    pass.set_scissor_rect(
                        batch.clip.x as u32,
                        batch.clip.y as u32,
                        batch.clip.width,
                        batch.clip.height,
                    );
                    pass.draw(
                        batch.first_vertex..batch.first_vertex + batch.vertex_count,
                        0..1,
                    );
                }
            }
        }
        queue.submit([encoder.finish()]);
        Ok((output, resources))
    }
}

fn validate_resource_budget(
    resources: &BTreeMap<String, AtlasResource>,
) -> Result<(), PlatformError> {
    let bytes = resources.values().try_fold(0usize, |total, resource| {
        total
            .checked_add(resource.byte_len())
            .ok_or_else(|| invalid("scene resource byte count overflowed"))
    })?;
    if resources.len() > MAX_GLYPH_RESOURCES || bytes > MAX_GLYPH_BYTES {
        return Err(invalid("glyph atlas resource budget was exceeded"));
    }
    Ok(())
}

fn validate_resource_id(resource_id: &str) -> Result<(), PlatformError> {
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

fn validate_glyph(glyph: &GlyphBitmap) -> Result<(), PlatformError> {
    let channels = match glyph.format {
        GlyphBitmapFormat::Alpha8 => 1usize,
        GlyphBitmapFormat::Rgba8 => 4usize,
    };
    let expected = (glyph.width as usize)
        .checked_mul(glyph.height as usize)
        .and_then(|pixels| pixels.checked_mul(channels));
    if glyph.width == 0
        || glyph.height == 0
        || expected != Some(glyph.pixels.len())
        || astra_core::Hash256::from_sha256(&glyph.pixels) != glyph.hash
    {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "glyph dimensions, format, or content hash is invalid",
        ));
    }
    Ok(())
}

fn validate_texture(texture: &TextureFrame) -> Result<(), PlatformError> {
    let expected = (texture.width as usize)
        .checked_mul(texture.height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if texture.width == 0
        || texture.height == 0
        || expected != Some(texture.rgba8.len())
        || astra_core::Hash256::from_sha256(&texture.rgba8) != texture.hash
    {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "texture dimensions or content hash is invalid",
        ));
    }
    Ok(())
}

fn validate_source_rect(source: RectI, width: u32, height: u32) -> Result<(), PlatformError> {
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

fn validate_destination(destination: RectI) -> Result<(), PlatformError> {
    if destination.width == 0 || destination.height == 0 {
        return Err(invalid("sprite destination rectangle is empty"));
    }
    Ok(())
}

fn pack_atlas(resources: &BTreeMap<String, AtlasResource>) -> Result<PackedAtlas, PlatformError> {
    let widest = resources
        .values()
        .map(|resource| resource.width() + ATLAS_PADDING * 2)
        .max()
        .unwrap_or(64);
    let atlas_width = widest
        .max(1024)
        .checked_next_power_of_two()
        .ok_or_else(|| invalid("glyph atlas width overflowed"))?;
    if atlas_width > ATLAS_SIDE {
        return Err(invalid("glyph is larger than the configured atlas"));
    }
    let mut placements = BTreeMap::new();
    let mut x = ATLAS_PADDING * 3;
    let mut y = ATLAS_PADDING;
    let mut row_height = 0;
    for (resource_id, resource) in resources {
        if resource.width() + ATLAS_PADDING * 2 > atlas_width
            || resource.height() + ATLAS_PADDING * 2 > ATLAS_SIDE
        {
            return Err(invalid("glyph is larger than the configured atlas"));
        }
        if x + resource.width() + ATLAS_PADDING > atlas_width {
            x = ATLAS_PADDING * 3;
            y = y
                .checked_add(row_height + ATLAS_PADDING)
                .ok_or_else(|| invalid("glyph atlas row overflowed"))?;
            row_height = 0;
        }
        if y + resource.height() + ATLAS_PADDING > ATLAS_SIDE {
            return Err(invalid("glyph atlas capacity was exceeded"));
        }
        placements.insert(resource_id.clone(), AtlasPlacement { x, y });
        x += resource.width() + ATLAS_PADDING;
        row_height = row_height.max(resource.height());
    }
    let used_height = (y + row_height + ATLAS_PADDING).max(1);
    let atlas_height = used_height
        .checked_next_power_of_two()
        .ok_or_else(|| invalid("glyph atlas height overflowed"))?;
    if atlas_height > ATLAS_SIDE {
        return Err(invalid("glyph atlas capacity was exceeded"));
    }
    Ok(PackedAtlas {
        placements,
        width: atlas_width,
        height: atlas_height,
    })
}

fn build_atlas_pixels(
    resources: &BTreeMap<String, AtlasResource>,
    atlas: &PackedAtlas,
) -> Result<Vec<u8>, PlatformError> {
    let mut pixels = vec![0; atlas.width as usize * atlas.height as usize * 4];
    pixels[..4].copy_from_slice(&[255; 4]);
    for (resource_id, resource) in resources {
        let placement = atlas
            .placements
            .get(resource_id)
            .ok_or_else(|| invalid("glyph atlas placement is missing"))?;
        for padded_row in -1_i32..=resource.height() as i32 {
            for padded_column in -1_i32..=resource.width() as i32 {
                let source_row = padded_row.clamp(0, resource.height() as i32 - 1) as usize;
                let source_column = padded_column.clamp(0, resource.width() as i32 - 1) as usize;
                let destination_row = (placement.y as i32 + padded_row) as usize;
                let destination_column = (placement.x as i32 + padded_column) as usize;
                let destination = (destination_row * atlas.width as usize + destination_column) * 4;
                pixels[destination..destination + 4].copy_from_slice(&resource_pixel(
                    resource,
                    source_column,
                    source_row,
                ));
            }
        }
    }
    Ok(pixels)
}

fn resource_pixel(resource: &AtlasResource, column: usize, row: usize) -> [u8; 4] {
    match resource {
        AtlasResource::Glyph(glyph) if glyph.format == GlyphBitmapFormat::Alpha8 => {
            let alpha = glyph.pixels[row * glyph.width as usize + column];
            [alpha, alpha, alpha, alpha]
        }
        AtlasResource::Glyph(glyph) => {
            let source = (row * glyph.width as usize + column) * 4;
            [
                glyph.pixels[source],
                glyph.pixels[source + 1],
                glyph.pixels[source + 2],
                glyph.pixels[source + 3],
            ]
        }
        AtlasResource::Texture(texture) => {
            let source = (row * texture.width as usize + column) * 4;
            [
                texture.rgba8[source],
                texture.rgba8[source + 1],
                texture.rgba8[source + 2],
                texture.rgba8[source + 3],
            ]
        }
    }
}

fn upload_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> wgpu::Texture {
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
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
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
    texture
}

fn build_vertices(
    frame: &SceneFrame,
    runs: &[DrawPrimitive],
    atlas: &PackedAtlas,
    resources: &BTreeMap<String, AtlasResource>,
) -> Result<(Vec<u8>, Vec<DrawBatch>), PlatformError> {
    let mut bytes = Vec::new();
    let mut batches = Vec::new();
    let mut vertex_count = 0u32;
    for primitive in runs {
        let clip = match primitive {
            DrawPrimitive::Quads(run) => run.clip,
            DrawPrimitive::Mesh(run) => run.clip,
        };
        if clip.width == 0 || clip.height == 0 {
            continue;
        }
        let first_vertex = vertex_count;
        match primitive {
            DrawPrimitive::Quads(run) => {
                let color = straight_to_premultiplied_linear(run.rgba, run.opacity);
                for quad in &run.quads {
                    let left = quad.destination.x as f32;
                    let top = quad.destination.y as f32;
                    let right = left + quad.destination.width as f32;
                    let bottom = top + quad.destination.height as f32;
                    let (u0, v0, u1, v1) = match &quad.source {
                        QuadSource::Resource {
                            resource_id,
                            source,
                        } => {
                            let placement = atlas
                                .placements
                                .get(resource_id)
                                .ok_or_else(|| invalid("draw resource has no atlas placement"))?;
                            let source_right = source.x as u32 + source.width;
                            let source_bottom = source.y as u32 + source.height;
                            (
                                (placement.x + source.x as u32) as f32 / atlas.width as f32,
                                (placement.y + source.y as u32) as f32 / atlas.height as f32,
                                (placement.x + source_right) as f32 / atlas.width as f32,
                                (placement.y + source_bottom) as f32 / atlas.height as f32,
                            )
                        }
                        QuadSource::White => {
                            let u = 0.5 / atlas.width as f32;
                            let v = 0.5 / atlas.height as f32;
                            (u, v, u, v)
                        }
                    };
                    for (x, y, u, v) in [
                        (left, top, u0, v0),
                        (right, top, u1, v0),
                        (right, bottom, u1, v1),
                        (left, top, u0, v0),
                        (right, bottom, u1, v1),
                        (left, bottom, u0, v1),
                    ] {
                        push_vertex(&mut bytes, x, y, u, v, color, frame.width, frame.height);
                        vertex_count = vertex_count
                            .checked_add(1)
                            .ok_or_else(|| invalid("scene vertex count overflowed"))?;
                    }
                }
            }
            DrawPrimitive::Mesh(run) => {
                let texture_mapping = run
                    .texture_id
                    .as_ref()
                    .map(|resource_id| {
                        let resource = resources
                            .get(resource_id)
                            .ok_or_else(|| invalid("mesh texture resource is missing"))?;
                        let placement = atlas
                            .placements
                            .get(resource_id)
                            .ok_or_else(|| invalid("mesh texture has no atlas placement"))?;
                        Ok::<_, PlatformError>((placement, resource.width(), resource.height()))
                    })
                    .transpose()?;
                for index in &run.indices {
                    let vertex = &run.vertices[*index as usize];
                    let (u, v) = if let Some((placement, width, height)) = texture_mapping {
                        (
                            (placement.x as f32 + vertex.uv[0] * width as f32) / atlas.width as f32,
                            (placement.y as f32 + vertex.uv[1] * height as f32)
                                / atlas.height as f32,
                        )
                    } else {
                        (0.5 / atlas.width as f32, 0.5 / atlas.height as f32)
                    };
                    push_vertex(
                        &mut bytes,
                        vertex.position[0],
                        vertex.position[1],
                        u,
                        v,
                        premultiplied_linear(vertex.premultiplied_rgba, run.opacity),
                        frame.width,
                        frame.height,
                    );
                    vertex_count = vertex_count
                        .checked_add(1)
                        .ok_or_else(|| invalid("scene vertex count overflowed"))?;
                }
            }
        }
        if vertex_count > first_vertex {
            batches.push(DrawBatch {
                first_vertex,
                vertex_count: vertex_count - first_vertex,
                clip,
            });
        }
    }
    Ok((bytes, batches))
}

fn premultiplied_linear(rgba: [u8; 4], opacity: f32) -> [f32; 4] {
    let alpha = f32::from(rgba[3]) / 255.0;
    let convert = |value: u8| {
        if alpha == 0.0 {
            0.0
        } else {
            srgb_byte_to_linear(((f32::from(value) / alpha).min(255.0)).round() as u8)
                * alpha
                * opacity
        }
    };
    [
        convert(rgba[0]),
        convert(rgba[1]),
        convert(rgba[2]),
        alpha * opacity,
    ]
}

fn straight_to_premultiplied_linear(rgba: [u8; 4], opacity: f32) -> [f32; 4] {
    let alpha = f32::from(rgba[3]) / 255.0 * opacity;
    [
        srgb_byte_to_linear(rgba[0]) * alpha,
        srgb_byte_to_linear(rgba[1]) * alpha,
        srgb_byte_to_linear(rgba[2]) * alpha,
        alpha,
    ]
}

fn srgb_byte_to_linear(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

#[allow(clippy::too_many_arguments)]
fn push_vertex(
    bytes: &mut Vec<u8>,
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    color: [f32; 4],
    width: u32,
    height: u32,
) {
    let ndc_x = x / width as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - y / height as f32 * 2.0;
    for value in [ndc_x, ndc_y, u, v, color[0], color[1], color[2], color[3]] {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
}

fn intersect_clip(
    current: Option<RectI>,
    next: RectI,
    width: u32,
    height: u32,
) -> Result<RectI, PlatformError> {
    let bounds = RectI::new(0, 0, width, height);
    let current = current.unwrap_or(bounds);
    let left = i64::from(current.x).max(i64::from(next.x)).max(0);
    let top = i64::from(current.y).max(i64::from(next.y)).max(0);
    let right = (i64::from(current.x) + i64::from(current.width))
        .min(i64::from(next.x) + i64::from(next.width))
        .min(i64::from(width));
    let bottom = (i64::from(current.y) + i64::from(current.height))
        .min(i64::from(next.y) + i64::from(next.height))
        .min(i64::from(height));
    let clipped_width = (right - left).max(0) as u32;
    let clipped_height = (bottom - top).max(0) as u32;
    Ok(RectI::new(
        left as i32,
        top as i32,
        clipped_width,
        clipped_height,
    ))
}

fn create_pipeline(
    device: &wgpu::Device,
) -> (wgpu::BindGroupLayout, wgpu::Sampler, wgpu::RenderPipeline) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("astra-glyph-atlas-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("astra-glyph-atlas-sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra-glyph-atlas-shader"),
        source: wgpu::ShaderSource::Wgsl(GLYPH_SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra-glyph-atlas-pipeline-layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("astra-glyph-atlas-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    (layout, sampler, pipeline)
}

fn invalid(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "surface.present_scene",
        message,
    )
}

const GLYPH_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(atlas, atlas_sampler, input.uv);
    return vec4<f32>(sample.rgb * input.color.rgb, sample.a * input.color.a);
}
"#;
