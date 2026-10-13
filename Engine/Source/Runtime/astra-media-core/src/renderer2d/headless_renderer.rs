use super::*;

impl HeadlessRenderer {
    pub fn retained_resource_commands(&self) -> Vec<DrawCommand> {
        self.textures
            .iter()
            .map(|(resource_id, frame)| DrawCommand::UploadTexture {
                resource_id: resource_id.clone(),
                frame: frame.clone(),
            })
            .chain(
                self.glyphs
                    .iter()
                    .map(|(resource_id, glyph)| DrawCommand::UploadGlyph {
                        resource_id: resource_id.clone(),
                        glyph: glyph.clone(),
                    }),
            )
            .collect()
    }

    pub fn capture_frame(&mut self, commands: &[DrawCommand]) -> Result<CpuFrame, MediaError> {
        <Self as Renderer2D>::capture_frame(self, commands)
    }

    pub fn capture_hash(&mut self, commands: &[DrawCommand]) -> Result<Hash256, MediaError> {
        <Self as Renderer2D>::capture_hash(self, commands)
    }

    /// Validates and transactionally commits the retained resource state without
    /// cloning the retained maps or rasterizing a render target.
    pub fn submit_frame(&mut self, commands: &[DrawCommand]) -> Result<(), MediaError> {
        let journal = self.validate_frame(commands)?;
        self.commit_frame(journal);
        Ok(())
    }

    /// Builds an immutable mutation journal after validating the complete scene.
    /// Pixel payloads remain shared through `Arc`; retained state is untouched.
    pub fn validate_frame(
        &self,
        commands: &[DrawCommand],
    ) -> Result<RetainedResourceJournal, MediaError> {
        validate_resource_payloads(commands)?;
        let mut mutations = Vec::new();
        let mut resource_overlay = BTreeMap::new();
        let mut clip_depth = 1_usize;
        let mut pixel_mask_depth = 0_usize;
        let mut transform_depth = 1_usize;
        let mut opacity_depth = 1_usize;
        for (command_index, command) in commands.iter().enumerate() {
            match command {
                DrawCommand::UploadTexture { resource_id, frame } => {
                    validate_texture_metadata(frame)?;
                    if retained_resource_exists_indexed(
                        &self.texture_ids,
                        &self.glyph_ids,
                        &resource_overlay,
                        resource_id,
                    ) {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_DUPLICATE: texture resource is already uploaded",
                        ));
                    }
                    resource_overlay.insert(resource_id.as_str(), PendingResource::Texture(frame));
                    mutations.push(RetainedResourceMutation::UploadTexture { resource_id, frame });
                }
                DrawCommand::UpdateTextureRegion {
                    resource_id,
                    x,
                    y,
                    width,
                    height,
                    rgba8,
                } => {
                    let dimensions = retained_texture_dimensions(
                        &self.textures,
                        &resource_overlay,
                        resource_id,
                    )
                    .ok_or_else(|| {
                        MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_UNKNOWN: texture update references an unknown texture",
                        )
                    })?;
                    validate_texture_region(dimensions, *x, *y, *width, *height, rgba8)?;
                    mutations.push(RetainedResourceMutation::UpdateTextureRegion {
                        resource_id,
                        x: *x,
                        y: *y,
                        width: *width,
                        height: *height,
                        rgba8,
                    });
                }
                DrawCommand::UploadGlyph { resource_id, glyph } => {
                    validate_glyph_metadata(glyph)?;
                    if retained_resource_exists_indexed(
                        &self.texture_ids,
                        &self.glyph_ids,
                        &resource_overlay,
                        resource_id,
                    ) {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_DUPLICATE: glyph resource is already uploaded",
                        ));
                    }
                    resource_overlay.insert(resource_id.as_str(), PendingResource::Glyph(glyph));
                    mutations.push(RetainedResourceMutation::UploadGlyph { resource_id, glyph });
                }
                DrawCommand::ReleaseResource { resource_id } => {
                    if !retained_resource_exists_indexed(
                        &self.texture_ids,
                        &self.glyph_ids,
                        &resource_overlay,
                        resource_id,
                    ) {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_RESOURCE_UNKNOWN: released resource is not uploaded",
                        ));
                    }
                    resource_overlay.insert(resource_id.as_str(), PendingResource::Released);
                    mutations.push(RetainedResourceMutation::Release { resource_id });
                }
                DrawCommand::Sprite {
                    texture_id,
                    source,
                    opacity,
                    ..
                } => {
                    validate_opacity(*opacity)?;
                    let dimensions =
                        retained_texture_dimensions(&self.textures, &resource_overlay, texture_id)
                            .ok_or_else(|| {
                                MediaError::message(
                                    "ASTRA_MEDIA_RESOURCE_UNKNOWN: sprite texture is not uploaded",
                                )
                            })?;
                    if let Some(source) = source {
                        validate_source_rect_dimensions(dimensions, *source)?;
                    }
                }
                DrawCommand::GlyphRun {
                    glyphs: instances,
                    opacity,
                    ..
                } => {
                    validate_opacity(*opacity)?;
                    for instance in instances.iter() {
                        if !retained_glyph_exists(
                            &self.glyph_ids,
                            &resource_overlay,
                            &instance.resource_id,
                        ) {
                            let pending_state =
                                match resource_overlay.get(instance.resource_id.as_str()) {
                                    Some(PendingResource::Released) => "released",
                                    Some(PendingResource::Texture(_)) => "texture",
                                    Some(PendingResource::Glyph(_)) => "glyph",
                                    None => "absent",
                                };
                            tracing::error!(
                                event = "media.scene.glyph_resource_missing",
                                command_index,
                                resource_hash = %astra_core::Hash256::from_sha256(instance.resource_id.as_bytes()),
                                previously_resident = self.glyph_ids.contains(&instance.resource_id),
                                pending_state,
                                resident_glyph_count = self.glyph_ids.len(),
                                pending_mutation_count = mutations.len(),
                                "glyph draw references a resource outside its lifetime"
                            );
                            return Err(MediaError::message(
                                "ASTRA_MEDIA_RESOURCE_UNKNOWN: glyph resource is not uploaded",
                            ));
                        }
                    }
                }
                DrawCommand::Mesh2D {
                    vertices,
                    indices,
                    material,
                    texture_id,
                    opacity,
                    ..
                } => {
                    validate_opacity(*opacity)?;
                    if vertices.is_empty()
                        || indices.is_empty()
                        || indices.len() % 3 != 0
                        || indices
                            .iter()
                            .any(|index| *index as usize >= vertices.len())
                        || vertices.iter().any(|vertex| {
                            !vertex.position.into_iter().all(f32::is_finite)
                                || !vertex.uv.into_iter().all(f32::is_finite)
                        })
                    {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_MESH_TOPOLOGY: mesh payload is invalid",
                        ));
                    }
                    match (material, texture_id) {
                        (MeshMaterial2D::Solid, None) => {}
                        (MeshMaterial2D::ColorTexture | MeshMaterial2D::GlyphMask, Some(id))
                            if retained_texture_dimensions(
                                &self.textures,
                                &resource_overlay,
                                id,
                            )
                            .is_some() => {}
                        _ => {
                            return Err(MediaError::message(
                                "ASTRA_MEDIA_MESH_MATERIAL: mesh material and texture mismatch",
                            ))
                        }
                    }
                }
                DrawCommand::MeshBatch2D {
                    vertices,
                    indices,
                    draws,
                    ..
                } => {
                    if vertices.is_empty() || draws.is_empty() {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_MESH_TOPOLOGY: mesh batch is empty",
                        ));
                    }
                    for draw in draws.iter() {
                        validate_opacity(draw.opacity)?;
                        let vertex_start = draw.vertex_start as usize;
                        let vertex_end = vertex_start
                            .checked_add(draw.vertex_count as usize)
                            .ok_or_else(|| MediaError::message("ASTRA_MEDIA_MESH_TOPOLOGY"))?;
                        let index_start = draw.index_start as usize;
                        let index_end = index_start
                            .checked_add(draw.index_count as usize)
                            .ok_or_else(|| MediaError::message("ASTRA_MEDIA_MESH_TOPOLOGY"))?;
                        if vertex_end > vertices.len()
                            || index_end > indices.len()
                            || draw.index_count == 0
                            || !draw.index_count.is_multiple_of(3)
                            || indices[index_start..index_end]
                                .iter()
                                .any(|index| *index as usize >= draw.vertex_count as usize)
                            || vertices[vertex_start..vertex_end].iter().any(|vertex| {
                                !vertex.position.into_iter().all(f32::is_finite)
                                    || !vertex.uv.into_iter().all(f32::is_finite)
                            })
                        {
                            return Err(MediaError::message(
                                "ASTRA_MEDIA_MESH_TOPOLOGY: mesh batch draw is invalid",
                            ));
                        }
                        match (&draw.material, &draw.texture_id) {
                            (MeshMaterial2D::Solid, None) => {}
                            (
                                MeshMaterial2D::ColorTexture | MeshMaterial2D::GlyphMask,
                                Some(id),
                            ) if retained_texture_dimensions(
                                &self.textures,
                                &resource_overlay,
                                id,
                            )
                            .is_some() => {}
                            _ => {
                                return Err(MediaError::message(
                                    "ASTRA_MEDIA_MESH_MATERIAL: mesh batch material and texture mismatch",
                                ));
                            }
                        }
                    }
                }
                DrawCommand::Texture { frame, opacity, .. }
                | DrawCommand::VideoFrame { frame, opacity, .. } => {
                    validate_texture(frame)?;
                    validate_opacity(*opacity)?;
                }
                DrawCommand::Glyph { glyph, opacity, .. } => {
                    validate_glyph(glyph)?;
                    validate_opacity(*opacity)?;
                }
                DrawCommand::PushPixelMask { .. } => pixel_mask_depth += 1,
                DrawCommand::PopPixelMask => {
                    pixel_mask_depth = pixel_mask_depth.checked_sub(1).ok_or_else(|| {
                        MediaError::message("ASTRA_MEDIA_PIXEL_MASK_STACK: stack underflow")
                    })?;
                }
                DrawCommand::PushClip { .. } => clip_depth += 1,
                DrawCommand::PopClip => {
                    clip_depth = clip_depth.checked_sub(1).ok_or_else(|| {
                        MediaError::message("ASTRA_MEDIA_CLIP_STACK: clip stack underflow")
                    })?;
                    if clip_depth == 0 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_CLIP_STACK: clip stack underflow",
                        ));
                    }
                }
                DrawCommand::PushTransform { transform } => {
                    validate_transform(*transform)?;
                    transform_depth += 1;
                }
                DrawCommand::PopTransform => {
                    if transform_depth == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_TRANSFORM_STACK: transform stack underflow",
                        ));
                    }
                    transform_depth -= 1;
                }
                DrawCommand::SetCamera { transform } => validate_transform(*transform)?,
                DrawCommand::PushOpacity { opacity } => {
                    validate_opacity(*opacity)?;
                    opacity_depth += 1;
                }
                DrawCommand::PopOpacity => {
                    if opacity_depth == 1 {
                        return Err(MediaError::message(
                            "ASTRA_MEDIA_OPACITY_STACK: opacity stack underflow",
                        ));
                    }
                    opacity_depth -= 1;
                }
                DrawCommand::FilterGraph { graph } => {
                    let validation = crate::FilterValidator.validate(graph);
                    if !validation.blocking_diagnostics().is_empty() {
                        return Err(MediaError::Diagnostics(validation.diagnostics));
                    }
                }
                DrawCommand::Clear { .. } | DrawCommand::Rect { .. } => {}
            }
        }
        if pixel_mask_depth != 0 || clip_depth != 1 || transform_depth != 1 || opacity_depth != 1 {
            return Err(MediaError::message(
                "ASTRA_MEDIA_SCENE_STACK: scene command stacks are unbalanced",
            ));
        }
        Ok(RetainedResourceJournal {
            mutations: mutations
                .into_iter()
                .map(OwnedRetainedResourceMutation::from)
                .collect(),
        })
    }

    /// Commits a journal produced by `validate_frame` without another validation
    /// or retained-map copy.
    pub fn commit_frame(&mut self, journal: RetainedResourceJournal) {
        for mutation in journal.mutations {
            match mutation {
                OwnedRetainedResourceMutation::UploadTexture { resource_id, frame } => {
                    self.texture_ids.insert(resource_id.clone());
                    self.textures.insert(resource_id, frame);
                }
                OwnedRetainedResourceMutation::UploadGlyph { resource_id, glyph } => {
                    self.glyph_ids.insert(resource_id.clone());
                    self.glyphs.insert(resource_id, glyph);
                }
                OwnedRetainedResourceMutation::UpdateTextureRegion {
                    resource_id,
                    x,
                    y,
                    width,
                    height,
                    rgba8,
                } => {
                    let frame = self
                        .textures
                        .get_mut(&resource_id)
                        .expect("validated texture region target must remain live");
                    apply_texture_region(frame, x, y, width, height, &rgba8)
                        .expect("validated texture region must commit");
                }
                OwnedRetainedResourceMutation::Release { resource_id } => {
                    self.textures.remove(&resource_id);
                    self.glyphs.remove(&resource_id);
                    self.texture_ids.remove(&resource_id);
                    self.glyph_ids.remove(&resource_id);
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct RetainedResourceJournal {
    pub(super) mutations: Vec<OwnedRetainedResourceMutation>,
}

#[derive(Debug)]
pub(super) enum OwnedRetainedResourceMutation {
    UploadTexture {
        resource_id: String,
        frame: TextureFrame,
    },
    UploadGlyph {
        resource_id: String,
        glyph: GlyphBitmap,
    },
    UpdateTextureRegion {
        resource_id: String,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba8: OwnedPixelBuffer,
    },
    Release {
        resource_id: String,
    },
}

#[derive(Debug)]
pub(super) enum RetainedResourceMutation<'a> {
    UploadTexture {
        resource_id: &'a str,
        frame: &'a TextureFrame,
    },
    UploadGlyph {
        resource_id: &'a str,
        glyph: &'a GlyphBitmap,
    },
    UpdateTextureRegion {
        resource_id: &'a str,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba8: &'a OwnedPixelBuffer,
    },
    Release {
        resource_id: &'a str,
    },
}

impl From<RetainedResourceMutation<'_>> for OwnedRetainedResourceMutation {
    fn from(value: RetainedResourceMutation<'_>) -> Self {
        match value {
            RetainedResourceMutation::UploadTexture { resource_id, frame } => Self::UploadTexture {
                resource_id: resource_id.to_owned(),
                frame: frame.clone(),
            },
            RetainedResourceMutation::UploadGlyph { resource_id, glyph } => Self::UploadGlyph {
                resource_id: resource_id.to_owned(),
                glyph: glyph.clone(),
            },
            RetainedResourceMutation::UpdateTextureRegion {
                resource_id,
                x,
                y,
                width,
                height,
                rgba8,
            } => Self::UpdateTextureRegion {
                resource_id: resource_id.to_owned(),
                x,
                y,
                width,
                height,
                rgba8: rgba8.clone(),
            },
            RetainedResourceMutation::Release { resource_id } => Self::Release {
                resource_id: resource_id.to_owned(),
            },
        }
    }
}
