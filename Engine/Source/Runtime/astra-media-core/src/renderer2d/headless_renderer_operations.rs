use super::*;

impl Renderer2D for HeadlessRenderer {
    fn capture_frame(&mut self, commands: &[DrawCommand]) -> Result<CpuFrame, MediaError> {
        // Validate the complete transaction before touching retained state.
        // Rasterization then journals only resources mutated by this frame;
        // stable retained maps are never cloned per frame.
        let _validated = self.validate_frame(commands)?;
        let width = self.request.width as usize;
        let height = self.request.height as usize;
        let byte_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| MediaError::message("render target is too large"))?;
        let mut bytes = vec![0; byte_len];
        let mut texture_changes = BTreeMap::<String, Option<TextureFrame>>::new();
        let mut glyph_changes = BTreeMap::<String, Option<GlyphBitmap>>::new();
        let full_clip = RectI::new(0, 0, self.request.width, self.request.height);
        let mut clips = vec![full_clip];
        let mut transforms = vec![Transform2D::IDENTITY];
        let mut camera = Transform2D::IDENTITY;
        let mut opacities = vec![1.0_f32];
        for command in commands {
            match command {
                DrawCommand::UploadTexture { resource_id, frame } => {
                    texture_changes.insert(resource_id.clone(), Some(frame.clone()));
                }
                DrawCommand::UpdateTextureRegion {
                    resource_id,
                    x,
                    y,
                    width,
                    height,
                    rgba8,
                } => {
                    if !texture_changes.contains_key(resource_id) {
                        texture_changes
                            .insert(resource_id.clone(), self.textures.get(resource_id).cloned());
                    }
                    let frame = texture_changes
                        .get_mut(resource_id)
                        .and_then(Option::as_mut)
                        .expect("validated texture update target must exist");
                    apply_texture_region(frame, *x, *y, *width, *height, rgba8)?;
                }
                DrawCommand::UploadGlyph { resource_id, glyph } => {
                    glyph_changes.insert(resource_id.clone(), Some(glyph.clone()));
                }
                DrawCommand::ReleaseResource { resource_id } => {
                    if capture_texture(&self.textures, &texture_changes, resource_id).is_some() {
                        texture_changes.insert(resource_id.clone(), None);
                    } else {
                        debug_assert!(
                            capture_glyph(&self.glyphs, &glyph_changes, resource_id).is_some()
                        );
                        glyph_changes.insert(resource_id.clone(), None);
                    }
                }
                DrawCommand::Sprite {
                    texture_id,
                    source,
                    destination,
                    opacity,
                    blend,
                    ..
                } => {
                    let frame = capture_texture(&self.textures, &texture_changes, texture_id)
                        .ok_or_else(|| {
                            MediaError::message(
                                "ASTRA_MEDIA_RESOURCE_UNKNOWN: sprite texture is not uploaded",
                            )
                        })?;
                    let cropped;
                    let frame = if let Some(source) = source {
                        cropped = crop_texture(frame, *source)?;
                        &cropped
                    } else {
                        frame
                    };
                    draw_texture(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        *destination,
                        frame,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::GlyphRun {
                    glyphs,
                    rgba,
                    opacity,
                    blend,
                    ..
                } => {
                    for instance in glyphs.iter() {
                        let glyph =
                            capture_glyph(&self.glyphs, &glyph_changes, &instance.resource_id)
                                .ok_or_else(|| {
                                    MediaError::message(
                                    "ASTRA_MEDIA_RESOURCE_UNKNOWN: glyph resource is not uploaded",
                                )
                                })?;
                        draw_glyph(
                            &mut bytes,
                            width,
                            height,
                            *clips.last().expect("clip stack is initialized"),
                            current_transform(camera, &transforms),
                            instance.x,
                            instance.y,
                            glyph,
                            instance.rotation_quadrants,
                            *rgba,
                            *opacity * opacities.last().copied().unwrap_or(1.0),
                            *blend,
                        )?;
                    }
                }
                DrawCommand::Mesh2D {
                    vertices,
                    indices,
                    material,
                    texture_id,
                    texture_filter,
                    opacity,
                    blend,
                    ..
                } => {
                    let texture = match (material, texture_id) {
                        (MeshMaterial2D::Solid, None) => None,
                        (MeshMaterial2D::ColorTexture | MeshMaterial2D::GlyphMask, Some(id)) => {
                            Some(
                                capture_texture(&self.textures, &texture_changes, id).ok_or_else(
                                    || {
                                        MediaError::message(
                                    "ASTRA_MEDIA_RESOURCE_UNKNOWN: mesh texture is not uploaded",
                                )
                                    },
                                )?,
                            )
                        }
                        _ => {
                            return Err(MediaError::message(
                                "ASTRA_MEDIA_MESH_MATERIAL: mesh material and texture mismatch",
                            ));
                        }
                    };
                    draw_mesh(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        vertices,
                        indices,
                        *material,
                        texture,
                        *texture_filter,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::MeshBatch2D {
                    vertices,
                    indices,
                    draws,
                    ..
                } => {
                    for draw in draws.iter() {
                        let vertex_start = draw.vertex_start as usize;
                        let vertex_end = vertex_start + draw.vertex_count as usize;
                        let index_start = draw.index_start as usize;
                        let index_end = index_start + draw.index_count as usize;
                        let texture = match (&draw.material, &draw.texture_id) {
                            (MeshMaterial2D::Solid, None) => None,
                            (
                                MeshMaterial2D::ColorTexture | MeshMaterial2D::GlyphMask,
                                Some(id),
                            ) => Some(capture_texture(&self.textures, &texture_changes, id).ok_or_else(|| {
                                MediaError::message(
                                    "ASTRA_MEDIA_RESOURCE_UNKNOWN: mesh batch texture is not uploaded",
                                )
                            })?),
                            _ => {
                                return Err(MediaError::message(
                                    "ASTRA_MEDIA_MESH_MATERIAL: mesh batch material and texture mismatch",
                                ));
                            }
                        };
                        let clip = draw.scissor.map_or_else(
                            || *clips.last().expect("clip stack is initialized"),
                            |scissor| {
                                intersection(
                                    *clips.last().expect("clip stack is initialized"),
                                    scissor,
                                )
                            },
                        );
                        draw_mesh(
                            &mut bytes,
                            width,
                            height,
                            clip,
                            current_transform(camera, &transforms),
                            &vertices[vertex_start..vertex_end],
                            &indices[index_start..index_end],
                            draw.material,
                            texture,
                            draw.texture_filter,
                            draw.opacity * opacities.last().copied().unwrap_or(1.0),
                            draw.blend,
                        )?;
                    }
                }
                DrawCommand::Clear { rgba } => {
                    for pixel in bytes.as_chunks_mut::<4>().0.iter_mut() {
                        pixel.copy_from_slice(rgba);
                    }
                }
                DrawCommand::Rect {
                    x,
                    y,
                    width: rect_width,
                    height: rect_height,
                    rgba,
                    ..
                } => {
                    let transform = current_transform(camera, &transforms);
                    draw_solid(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        transform,
                        RectI::new(*x as i32, *y as i32, *rect_width, *rect_height),
                        *rgba,
                        *opacities.last().expect("opacity stack is initialized"),
                        BlendMode::Alpha,
                    )?;
                }
                DrawCommand::Texture {
                    frame,
                    destination,
                    opacity,
                    blend,
                    ..
                }
                | DrawCommand::VideoFrame {
                    frame,
                    destination,
                    opacity,
                    blend,
                    ..
                } => {
                    validate_texture(frame)?;
                    draw_texture(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        *destination,
                        frame,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::Glyph {
                    glyph,
                    x,
                    y,
                    rgba,
                    opacity,
                    blend,
                    ..
                } => {
                    validate_glyph(glyph)?;
                    draw_glyph(
                        &mut bytes,
                        width,
                        height,
                        *clips.last().expect("clip stack is initialized"),
                        current_transform(camera, &transforms),
                        *x,
                        *y,
                        glyph,
                        0,
                        *rgba,
                        *opacity * opacities.last().copied().unwrap_or(1.0),
                        *blend,
                    )?;
                }
                DrawCommand::PushPixelMask { .. } | DrawCommand::PopPixelMask => {
                    return Err(MediaError::message("ASTRA_MEDIA_PIXEL_MASK_GPU_REQUIRED: CPU renderer does not support pixel masks"));
                }
                DrawCommand::PushClip { rect } => {
                    let transformed =
                        transformed_bounds(current_transform(camera, &transforms), *rect)?;
                    clips.push(intersection(
                        *clips.last().expect("clip stack is initialized"),
                        transformed,
                    ));
                }
                DrawCommand::PopClip => {
                    if clips.len() == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_CLIP_STACK: clip stack underflow",
                        ));
                    }
                    clips.pop();
                }
                DrawCommand::PushTransform { transform } => {
                    validate_transform(*transform)?;
                    let current = *transforms.last().expect("transform stack is initialized");
                    transforms.push(current.then(*transform));
                }
                DrawCommand::PopTransform => {
                    if transforms.len() == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_TRANSFORM_STACK: transform stack underflow",
                        ));
                    }
                    transforms.pop();
                }
                DrawCommand::SetCamera { transform } => {
                    validate_transform(*transform)?;
                    camera = *transform;
                }
                DrawCommand::PushOpacity { opacity } => {
                    validate_opacity(*opacity)?;
                    opacities.push(opacities.last().copied().unwrap_or(1.0) * *opacity);
                }
                DrawCommand::PopOpacity => {
                    if opacities.len() == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_OPACITY_STACK: opacity stack underflow",
                        ));
                    }
                    opacities.pop();
                }
                DrawCommand::FilterGraph { graph } => {
                    let frame = CpuFrame {
                        width: self.request.width,
                        height: self.request.height,
                        format: self.request.format,
                        bytes,
                    };
                    bytes = crate::CpuFilterExecutor.execute(graph, frame)?.0.bytes;
                }
            }
        }
        if clips.len() != 1 || transforms.len() != 1 || opacities.len() != 1 {
            return Err(MediaError::message(
                "ASTRA_MEDIA_SCENE_STACK: scene command stacks are unbalanced",
            ));
        }
        let frame = CpuFrame {
            width: self.request.width,
            height: self.request.height,
            format: self.request.format,
            bytes,
        };
        commit_capture_changes(&mut self.textures, &mut self.texture_ids, texture_changes);
        commit_capture_changes(&mut self.glyphs, &mut self.glyph_ids, glyph_changes);
        tracing::trace!(
            target: "astra_media_core::renderer2d",
            event = "renderer2d.frame.committed",
            command_count = commands.len(),
            texture_count = self.textures.len(),
            glyph_count = self.glyphs.len(),
        );
        Ok(frame)
    }
}

pub(super) fn capture_texture<'a>(
    retained: &'a BTreeMap<String, TextureFrame>,
    changes: &'a BTreeMap<String, Option<TextureFrame>>,
    resource_id: &str,
) -> Option<&'a TextureFrame> {
    changes
        .get(resource_id)
        .map_or_else(|| retained.get(resource_id), Option::as_ref)
}

pub(super) fn capture_glyph<'a>(
    retained: &'a BTreeMap<String, GlyphBitmap>,
    changes: &'a BTreeMap<String, Option<GlyphBitmap>>,
    resource_id: &str,
) -> Option<&'a GlyphBitmap> {
    changes
        .get(resource_id)
        .map_or_else(|| retained.get(resource_id), Option::as_ref)
}

pub(super) fn commit_capture_changes<T>(
    retained: &mut BTreeMap<String, T>,
    ids: &mut HashSet<String>,
    changes: BTreeMap<String, Option<T>>,
) {
    for (resource_id, value) in changes {
        if let Some(value) = value {
            ids.insert(resource_id.clone());
            retained.insert(resource_id, value);
        } else {
            ids.remove(&resource_id);
            retained.remove(&resource_id);
        }
    }
}

pub(super) fn current_transform(camera: Transform2D, transforms: &[Transform2D]) -> Transform2D {
    transforms
        .last()
        .copied()
        .unwrap_or(Transform2D::IDENTITY)
        .then(camera)
}

pub(super) fn validate_transform(transform: Transform2D) -> Result<(), MediaError> {
    if [
        transform.m11,
        transform.m12,
        transform.m21,
        transform.m22,
        transform.tx,
        transform.ty,
    ]
    .into_iter()
    .all(f32::is_finite)
        && transform.inverse().is_some()
    {
        Ok(())
    } else {
        Err(MediaError::message(
            "ASTRA_MEDIA_TRANSFORM: transform is non-finite or singular",
        ))
    }
}

pub(super) fn validate_opacity(opacity: f32) -> Result<(), MediaError> {
    if opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(MediaError::message(
            "ASTRA_MEDIA_OPACITY: opacity must be within 0..=1",
        ))
    }
}

pub(super) fn validate_resource_payloads(commands: &[DrawCommand]) -> Result<(), MediaError> {
    for command in commands {
        match command {
            DrawCommand::UploadTexture { frame, .. } => validate_texture(frame)?,
            DrawCommand::UploadGlyph { glyph, .. } => validate_glyph(glyph)?,
            _ => {}
        }
    }
    Ok(())
}
