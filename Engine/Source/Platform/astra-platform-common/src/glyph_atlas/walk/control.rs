use super::*;
impl<'a> WalkedScene<'a> {
    pub(super) fn apply_control(
        &mut self,
        command: &'a SceneCommand,
        resources: &BTreeMap<String, AtlasResource>,
        frame: &'a SceneFrame,
        apply_mutations: bool,
    ) -> Result<bool, PlatformError> {
        macro_rules! resource_get {
            ($id:expr) => {
                match self.render_mutations.get($id) {
                    Some(resource) => resource.as_ref(),
                    None => resources.get($id),
                }
            };
        }
        macro_rules! resource_contains {
            ($id:expr) => {
                resource_get!($id).is_some()
            };
        }
        match command {
            SceneCommand::UploadTexture { resource_id, frame } => {
                self.upload_texture_count += 1;
                validate_resource_id(resource_id)?;
                validate_texture_metadata(frame)?;
                if apply_mutations {
                    if let Some(previous) = self.committed_mutations.get(resource_id) {
                        let id_hash =
                            format!("sha256:{:x}", Sha256::digest(resource_id.as_bytes()));
                        tracing::error!(
                            event = "platform.wgpu.scene.texture_mutation_rejected",
                            id_hash,
                            previous_release = previous.is_none(),
                            upload_texture_count = self.upload_texture_count,
                            release_resource_count = self.release_resource_count,
                            "texture resource is mutated repeatedly in one frame"
                        );
                        return Err(invalid(
                            "texture resource id is mutated more than once in a frame",
                        ));
                    }
                    if resource_contains!(resource_id) {
                        return Err(invalid("texture upload repeats a live resource id"));
                    }
                    let resource = AtlasResource::texture(frame.clone());
                    self.render_mutations
                        .insert(resource_id.clone(), Some(resource.clone()));
                    self.committed_mutations
                        .insert(resource_id.clone(), Some(resource));
                    self.resources_changed = true;
                } else if resource_get!(resource_id) != Some(&AtlasResource::texture(frame.clone()))
                {
                    return Err(invalid(
                        "retained texture resource does not match the recovery frame",
                    ));
                }
            }
            SceneCommand::UpdateTextureRegion {
                resource_id,
                x,
                y,
                width,
                height,
                rgba8,
            } => {
                self.upload_texture_count += 1;
                validate_resource_id(resource_id)?;
                let current = resource_get!(resource_id)
                    .ok_or_else(|| invalid("texture update references an unknown resource"))?;
                let AtlasResource::Texture(current) = current else {
                    return Err(invalid("texture update references a non-texture resource"));
                };
                validate_texture_region(
                    current.base.width,
                    current.base.height,
                    *x,
                    *y,
                    *width,
                    *height,
                    rgba8,
                )?;
                self.texture_region_uploads.push(TextureRegionUpload {
                    resource_id,
                    texture_width: current.base.width,
                    texture_height: current.base.height,
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    rgba8,
                });
                if apply_mutations {
                    let texture =
                        updated_retained_texture(current, *x, *y, *width, *height, rgba8.clone())?;
                    let resource = AtlasResource::Texture(Arc::new(texture));
                    self.render_mutations
                        .insert(resource_id.clone(), Some(resource.clone()));
                    self.committed_mutations
                        .insert(resource_id.clone(), Some(resource));
                    self.resources_changed = true;
                } else {
                    return Err(invalid(
                        "texture region updates are not valid in a recovery frame",
                    ));
                }
            }
            SceneCommand::UploadGlyph { resource_id, glyph } => {
                self.upload_glyph_count += 1;
                validate_resource_id(resource_id)?;
                validate_glyph_metadata(glyph)?;
                if apply_mutations {
                    if self.committed_mutations.contains_key(resource_id) {
                        return Err(invalid(
                            "glyph resource id is mutated more than once in a frame",
                        ));
                    }
                    if resource_contains!(resource_id) {
                        return Err(invalid("glyph upload repeats a live resource id"));
                    }
                    let resource = AtlasResource::Glyph(Arc::new(glyph.clone()));
                    self.render_mutations
                        .insert(resource_id.clone(), Some(resource.clone()));
                    self.committed_mutations
                        .insert(resource_id.clone(), Some(resource));
                    self.resources_changed = true;
                } else if resource_get!(resource_id)
                    != Some(&AtlasResource::Glyph(Arc::new(glyph.clone())))
                {
                    return Err(invalid(
                        "retained glyph resource does not match the recovery frame",
                    ));
                }
            }
            SceneCommand::ReleaseResource { resource_id } => {
                self.release_resource_count += 1;
                validate_resource_id(resource_id)?;
                if apply_mutations {
                    if self.drawn_resources.contains(resource_id) {
                        return Err(invalid(
                            "glyph resource cannot be released after use in the same frame",
                        ));
                    }
                    if !resource_contains!(resource_id) {
                        return Err(invalid("glyph release references an unknown resource"));
                    }
                    self.render_mutations.insert(resource_id.clone(), None);
                    self.committed_mutations.insert(resource_id.clone(), None);
                    self.resources_changed = true;
                } else if resource_contains!(resource_id) {
                    return Err(invalid(
                        "retained glyph release was not committed before recovery",
                    ));
                }
            }
            SceneCommand::PushPixelMask { bits } => {
                self.pixel_masks
                    .push(self.pixel_masks.last().copied().unwrap_or(u64::MAX) & bits);
            }
            SceneCommand::PopPixelMask => {
                if self.pixel_masks.pop().is_none() {
                    return Err(invalid("GPU pixel mask stack underflowed"));
                }
            }
            SceneCommand::PushClip { rect } => {
                let rect = transformed_bounds(
                    current_transform(self.camera, &self.transform_stack),
                    *rect,
                )?;
                let clip = intersect_clip(
                    self.clip_stack.last().copied(),
                    rect,
                    frame.width,
                    frame.height,
                )?;
                self.clip_stack.push(clip);
            }
            SceneCommand::PopClip => {
                if self.clip_stack.pop().is_none() {
                    return Err(invalid("glyph clip stack underflowed"));
                }
            }
            SceneCommand::PushTransform { transform } => {
                validate_transform(*transform)?;
                let current = *self
                    .transform_stack
                    .last()
                    .expect("transform stack is initialized");
                self.transform_stack
                    .push(compose_transform(current, *transform));
            }
            SceneCommand::PopTransform => {
                if self.transform_stack.len() == 1 {
                    return Err(invalid("GPU scene transform stack underflowed"));
                }
                self.transform_stack.pop();
            }
            SceneCommand::SetCamera { transform } => {
                validate_transform(*transform)?;
                self.camera = *transform;
            }
            SceneCommand::PushOpacity { opacity } => {
                validate_opacity(*opacity)?;
                let current = self.opacity_stack.last().copied().unwrap_or(1.0);
                self.opacity_stack.push(current * *opacity);
            }
            SceneCommand::PopOpacity => {
                if self.opacity_stack.len() == 1 {
                    return Err(invalid("GPU scene opacity stack underflowed"));
                }
                self.opacity_stack.pop();
            }
            SceneCommand::FilterGraph { .. } => {
                // Filter graphs are validated and executed by the offscreen
                // owner after this scene pass. They carry no atlas mutation.
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
