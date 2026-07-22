use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_media_core::{BlendMode, MeshMaterial2D, MeshVertex2D, SceneCommand, TextureFrame};
use astra_ui_core::{
    UiMaterialKind, UiMeshPrimitive, UiPoint, UiRect, UiRenderFrame, UiTextureDelta,
    UiTextureFormat, UiTextureId, UiTextureRelease, UiTextureUpload, UiValidationError, UiVertex,
    UiViewport, ValidateUi,
};
use yakui_core::paint::{PaintDom, Pipeline, TextureChange, TextureFormat};
use yakui_core::TextureId as YakuiTextureId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TextureContentKey {
    content_hash: Hash256,
    width: u32,
    height: u32,
    format: u8,
}

#[derive(Debug, Clone, Copy)]
struct ManagedTextureBinding {
    content: TextureContentKey,
    id: UiTextureId,
    generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct SharedTexture {
    id: UiTextureId,
    generation: u64,
    references: u32,
}

/// Converts Yakui's ephemeral managed texture IDs into stable Scene2D
/// resources. Yakui may retire and recreate a glyph texture while rebuilding a
/// tree even if its pixels did not change. Retaining by verified content keeps
/// the atlas placement and avoids a release/upload transaction for that case.
#[derive(Debug, Default)]
pub struct YakuiPaintConverter {
    next_texture_id: u64,
    managed_textures: BTreeMap<String, ManagedTextureBinding>,
    shared_textures: BTreeMap<TextureContentKey, SharedTexture>,
    user_texture_generations: BTreeMap<UiTextureId, u64>,
    force_full_resync: bool,
}

impl YakuiPaintConverter {
    pub fn new() -> Self {
        Self {
            next_texture_id: 1,
            force_full_resync: true,
            ..Self::default()
        }
    }

    pub fn request_full_resync(&mut self) {
        self.force_full_resync = true;
    }

    pub fn convert(
        &mut self,
        paint: &PaintDom,
        session_id: &str,
        generation: u64,
        viewport: UiViewport,
    ) -> Result<UiRenderFrame, UiValidationError> {
        let textures = paint.textures();
        let mut uploads = Vec::new();
        let mut releases = Vec::new();

        if self.force_full_resync {
            // A full resync is requested only after the caller has reset its
            // rendering context. The Scene2D resource owner may still contain
            // the prior session's resources (for example while restoring a
            // save without recreating the platform surface), so make the
            // transition explicit and ordered: release every live texture
            // before re-uploading the stable ids below.
            releases.extend(self.release_live_textures_for_resync()?);
            for (id, texture) in textures.iter() {
                self.sync_managed_texture(
                    texture_key(YakuiTextureId::Managed(id)),
                    texture,
                    true,
                    &mut uploads,
                )?;
            }
            let mut resynced = BTreeSet::new();
            uploads.retain(|upload| resynced.insert((upload.id, upload.generation)));
        } else {
            // Process removals before additions, but delay actual releases
            // until all edits have been reconciled. A tree rebuild can replace
            // one Yakui managed ID with another for unchanged glyph pixels in
            // the same frame; issuing the release eagerly would defeat reuse.
            let edits: Vec<_> = textures.edits().collect();
            for (id, change) in &edits {
                if *change == TextureChange::Removed {
                    self.remove_managed_texture(&texture_key(YakuiTextureId::Managed(*id)))?;
                }
            }
            for (id, change) in edits {
                if matches!(change, TextureChange::Added | TextureChange::Modified) {
                    let key = texture_key(YakuiTextureId::Managed(id));
                    if change == TextureChange::Modified {
                        self.remove_managed_texture(&key)?;
                    }
                    let texture = textures.get(id).ok_or_else(|| {
                        UiValidationError::invalid(
                            "ASTRA_UI_YAKUI_TEXTURE_MISSING",
                            "Yakui reported an added/modified texture without storage",
                        )
                    })?;
                    self.sync_managed_texture(key, texture, false, &mut uploads)?;
                }
            }
            releases.extend(self.release_unreferenced_textures());
        }

        let mut primitives = Vec::new();
        let width = viewport.physical_width as f32;
        let height = viewport.physical_height as f32;
        for (layer_index, layer) in paint.layers().iter().enumerate() {
            for (call_index, call) in layer.calls.iter().enumerate() {
                let texture = call.texture.map(|id| self.texture_reference(id));
                let texture = texture.transpose()?;
                let texture_generation = texture.map(|(_, generation)| generation);
                let texture = texture.map(|(id, _)| id);
                if texture.is_some() && texture_generation == Some(0) {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_YAKUI_TEXTURE_NOT_LIVE",
                        "paint call references a texture without a live generation",
                    ));
                }
                let material = match (call.pipeline, texture) {
                    (Pipeline::Main, None) => UiMaterialKind::SolidColor,
                    (Pipeline::Main, Some(_)) => UiMaterialKind::ColorTexture,
                    (Pipeline::Text, Some(_)) => UiMaterialKind::GlyphMask,
                    (Pipeline::Text, None) => {
                        return Err(UiValidationError::invalid(
                            "ASTRA_UI_YAKUI_TEXT_TEXTURE",
                            "Yakui text pipeline requires a texture",
                        ));
                    }
                };
                let vertices = call
                    .vertices
                    .iter()
                    .map(|vertex| UiVertex {
                        position_points: [vertex.position.x * width, vertex.position.y * height],
                        uv: [vertex.texcoord.x, vertex.texcoord.y],
                        premultiplied_rgba: linear_color_to_premultiplied(vertex.color.to_array()),
                    })
                    .collect();
                let clip_rect_points = call.clip.map(|clip| UiRect {
                    min: UiPoint {
                        x: clip.pos().x,
                        y: clip.pos().y,
                    },
                    max: UiPoint {
                        x: clip.pos().x + clip.size().x,
                        y: clip.pos().y + clip.size().y,
                    },
                });
                primitives.push(UiMeshPrimitive {
                    id: format!("yakui/{layer_index}/{call_index}"),
                    layer: layer_index as i32,
                    clip_rect_points,
                    material,
                    texture,
                    texture_generation,
                    vertices,
                    indices: call.indices.iter().map(|value| u32::from(*value)).collect(),
                });
            }
        }
        let frame = UiRenderFrame {
            schema: "astra.ui_render_frame.v1".to_string(),
            session_id: session_id.to_string(),
            generation,
            viewport,
            textures: UiTextureDelta {
                uploads,
                releases,
                full_resync: self.force_full_resync,
            },
            primitives,
        };
        frame.validate()?;
        self.force_full_resync = false;
        Ok(frame)
    }

    fn texture_reference(
        &mut self,
        id: YakuiTextureId,
    ) -> Result<(UiTextureId, u64), UiValidationError> {
        match id {
            YakuiTextureId::User(value) => {
                let id = UiTextureId(value | (1_u64 << 63));
                let generation = self.user_texture_generations.entry(id).or_insert(0);
                if *generation == 0 {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_YAKUI_USER_TEXTURE_NOT_LIVE",
                        "paint call references a user texture without an upload",
                    ));
                }
                Ok((id, *generation))
            }
            YakuiTextureId::Managed(_) => {
                let key = texture_key(id);
                let binding = self.managed_textures.get(&key).ok_or_else(|| {
                    UiValidationError::invalid(
                        "ASTRA_UI_YAKUI_TEXTURE_NOT_LIVE",
                        "paint call references a managed texture without a live binding",
                    )
                })?;
                Ok((binding.id, binding.generation))
            }
        }
    }

    fn sync_managed_texture(
        &mut self,
        key: String,
        texture: &yakui_core::paint::Texture,
        force_upload: bool,
        uploads: &mut Vec<UiTextureUpload>,
    ) -> Result<(), UiValidationError> {
        let mut upload = texture_upload(UiTextureId(0), 0, texture)?;
        let content = TextureContentKey {
            content_hash: upload.content_hash,
            width: upload.width,
            height: upload.height,
            format: texture_format_key(upload.format),
        };
        if let Some(existing) = self.managed_textures.get(&key).copied() {
            if existing.content == content {
                if force_upload {
                    upload.id = existing.id;
                    upload.generation = existing.generation;
                    uploads.push(upload);
                }
                return Ok(());
            }
            self.remove_managed_texture(&key)?;
        }
        let (id, generation, requires_upload) = match self.shared_textures.get_mut(&content) {
            Some(shared) => {
                shared.references = shared.references.saturating_add(1);
                (shared.id, shared.generation, force_upload)
            }
            None => {
                let id = UiTextureId(self.next_texture_id);
                self.next_texture_id = self.next_texture_id.saturating_add(1);
                let generation = 1;
                self.shared_textures.insert(
                    content,
                    SharedTexture {
                        id,
                        generation,
                        references: 1,
                    },
                );
                (id, generation, true)
            }
        };
        self.managed_textures.insert(
            key,
            ManagedTextureBinding {
                content,
                id,
                generation,
            },
        );
        if requires_upload {
            upload.id = id;
            upload.generation = generation;
            uploads.push(upload);
        }
        Ok(())
    }

    fn remove_managed_texture(&mut self, key: &str) -> Result<(), UiValidationError> {
        let binding = self.managed_textures.remove(key).ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_YAKUI_TEXTURE_UNKNOWN",
                "Yakui removed an unknown texture",
            )
        })?;
        let shared = self
            .shared_textures
            .get_mut(&binding.content)
            .ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_YAKUI_TEXTURE_GENERATION",
                    "managed texture binding has no shared resource",
                )
            })?;
        if shared.id != binding.id
            || shared.generation != binding.generation
            || shared.references == 0
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_YAKUI_TEXTURE_REFERENCE",
                "managed texture binding disagrees with its shared resource",
            ));
        }
        shared.references -= 1;
        Ok(())
    }

    fn release_unreferenced_textures(&mut self) -> Vec<UiTextureRelease> {
        let released: Vec<_> = self
            .shared_textures
            .iter()
            .filter_map(|(content, shared)| {
                (shared.references == 0).then_some((*content, shared.id, shared.generation))
            })
            .collect();
        for (content, _, _) in &released {
            let _ = self.shared_textures.remove(content);
        }
        released
            .into_iter()
            .map(|(_, id, generation)| UiTextureRelease { id, generation })
            .collect()
    }

    fn release_live_textures_for_resync(
        &mut self,
    ) -> Result<Vec<UiTextureRelease>, UiValidationError> {
        let releases = self
            .shared_textures
            .values()
            .map(|shared| UiTextureRelease {
                id: shared.id,
                generation: shared.generation,
            })
            .collect::<Vec<_>>();
        // Keep only entries with active managed references. A resync advances
        // their generation so the renderer observes a release of the old
        // Scene2D resource followed by a distinct upload. Reusing the old
        // generation would mutate one atlas resource twice in one submission.
        self.shared_textures
            .retain(|_, shared| shared.references > 0);
        for shared in self.shared_textures.values_mut() {
            shared.generation = shared.generation.checked_add(1).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_YAKUI_TEXTURE_GENERATION_OVERFLOW",
                    "texture generation overflowed during full resync",
                )
            })?;
        }
        for binding in self.managed_textures.values_mut() {
            let shared = self.shared_textures.get(&binding.content).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_YAKUI_TEXTURE_REFERENCE",
                    "managed texture binding disappeared during full resync",
                )
            })?;
            binding.id = shared.id;
            binding.generation = shared.generation;
        }
        Ok(releases)
    }
}

fn texture_format_key(format: UiTextureFormat) -> u8 {
    match format {
        UiTextureFormat::R8Unorm => 1,
        UiTextureFormat::Rgba8SrgbPremultiplied => 2,
    }
}

fn texture_key(id: YakuiTextureId) -> String {
    format!("{id:?}")
}

fn texture_upload(
    id: UiTextureId,
    generation: u64,
    texture: &yakui_core::paint::Texture,
) -> Result<UiTextureUpload, UiValidationError> {
    let (format, pixels) = match texture.format() {
        TextureFormat::R8 => (UiTextureFormat::R8Unorm, texture.data().to_vec()),
        TextureFormat::Rgba8SrgbPremultiplied => (
            UiTextureFormat::Rgba8SrgbPremultiplied,
            texture.data().to_vec(),
        ),
        TextureFormat::Rgba8Srgb => {
            let mut pixels = texture.data().to_vec();
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[0] = ((pixel[0] as u16 * pixel[3] as u16) / 255) as u8;
                pixel[1] = ((pixel[1] as u16 * pixel[3] as u16) / 255) as u8;
                pixel[2] = ((pixel[2] as u16 * pixel[3] as u16) / 255) as u8;
            }
            (UiTextureFormat::Rgba8SrgbPremultiplied, pixels)
        }
    };
    let size = texture.size();
    let expected = size.x as usize * size.y as usize * format.bytes_per_pixel();
    if pixels.len() != expected {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_YAKUI_TEXTURE_LENGTH",
            "Yakui texture dimensions do not match its payload",
        ));
    }
    Ok(UiTextureUpload {
        id,
        generation,
        width: size.x,
        height: size.y,
        format,
        content_hash: Hash256::from_sha256(&pixels),
        pixels: pixels.into(),
    })
}

fn linear_color_to_premultiplied(color: [f32; 4]) -> [u8; 4] {
    let alpha = color[3].clamp(0.0, 1.0);
    let a = (alpha * 255.0).round() as u8;
    let convert = |value: f32| {
        ((fast_srgb8::f32_to_srgb8(value.clamp(0.0, 1.0)) as u16 * a as u16) / 255) as u8
    };
    [convert(color[0]), convert(color[1]), convert(color[2]), a]
}

pub fn ui_frame_to_scene_commands(
    frame: &UiRenderFrame,
) -> Result<Vec<SceneCommand>, UiValidationError> {
    frame.validate()?;
    let mut commands = Vec::new();
    for release in &frame.textures.releases {
        commands.push(SceneCommand::ReleaseResource {
            resource_id: texture_resource_id(frame, release.id, release.generation),
        });
    }
    for upload in &frame.textures.uploads {
        let rgba8 = scene_texture_rgba8(upload);
        commands.push(SceneCommand::UploadTexture {
            resource_id: texture_resource_id(frame, upload.id, upload.generation),
            frame: TextureFrame::from_rgba8(upload.width, upload.height, rgba8.into()).map_err(
                |error| UiValidationError::invalid("ASTRA_UI_TEXTURE_PAYLOAD", error.to_string()),
            )?,
        });
    }
    for primitive in &frame.primitives {
        let texture_id = primitive
            .texture
            .zip(primitive.texture_generation)
            .map(|(id, generation)| texture_resource_id(frame, id, generation));
        commands.push(SceneCommand::Mesh2D {
            id: primitive.id.clone(),
            vertices: primitive
                .vertices
                .iter()
                .map(|vertex| MeshVertex2D {
                    position: vertex.position_points,
                    uv: vertex.uv,
                    premultiplied_rgba: vertex.premultiplied_rgba,
                })
                .collect::<Vec<_>>()
                .into(),
            indices: primitive.indices.clone().into(),
            material: match primitive.material {
                UiMaterialKind::SolidColor => MeshMaterial2D::Solid,
                UiMaterialKind::ColorTexture => MeshMaterial2D::ColorTexture,
                UiMaterialKind::GlyphMask => MeshMaterial2D::GlyphMask,
            },
            texture_id,
            opacity: 1.0,
            blend: BlendMode::Alpha,
        });
    }
    Ok(commands)
}

/// Scene2D `TextureFrame` pixels are straight-alpha RGBA. The UI transport is
/// premultiplied, so restore straight color before sharing the stage renderer.
fn scene_texture_rgba8(upload: &UiTextureUpload) -> Vec<u8> {
    match upload.format {
        UiTextureFormat::Rgba8SrgbPremultiplied => upload
            .pixels
            .chunks_exact(4)
            .flat_map(|pixel| {
                let alpha = u16::from(pixel[3]);
                if alpha == 0 {
                    [0, 0, 0, 0]
                } else {
                    let restore =
                        |value: u8| ((u16::from(value) * 255 + alpha / 2) / alpha).min(255) as u8;
                    [
                        restore(pixel[0]),
                        restore(pixel[1]),
                        restore(pixel[2]),
                        pixel[3],
                    ]
                }
            })
            .collect(),
        UiTextureFormat::R8Unorm => upload
            .pixels
            .iter()
            .flat_map(|value| [255, 255, 255, *value])
            .collect(),
    }
}

fn texture_resource_id(frame: &UiRenderFrame, id: UiTextureId, generation: u64) -> String {
    texture_resource_id_for_session(&frame.session_id, id, generation)
}

fn texture_resource_id_for_session(session_id: &str, id: UiTextureId, generation: u64) -> String {
    // `UiRenderFrame::generation` changes for every submitted UI frame. It is
    // intentionally excluded: a texture's own id and generation are its
    // lifetime identity, so unchanged Yakui resources keep their atlas
    // placement across input-only UI generations.
    format!("ui:{session_id}/{}/{generation}", id.0)
}

#[cfg(test)]
mod tests {
    use super::{scene_texture_rgba8, texture_resource_id_for_session, YakuiPaintConverter};
    use astra_core::Hash256;
    use astra_ui_core::{UiTextureFormat, UiTextureId, UiTextureUpload};
    use yakui_core::geometry::UVec2;
    use yakui_core::paint::{Texture, TextureFormat};

    #[astra_headless_test::test]
    fn glyph_mask_upload_becomes_straight_alpha_scene_texture() {
        let upload = UiTextureUpload {
            id: UiTextureId(1),
            generation: 1,
            width: 2,
            height: 1,
            format: UiTextureFormat::R8Unorm,
            content_hash: Hash256::from_sha256(&[64, 255]),
            pixels: vec![64, 255].into(),
        };

        assert_eq!(
            scene_texture_rgba8(&upload),
            vec![255, 255, 255, 64, 255, 255, 255, 255]
        );
    }

    #[astra_headless_test::test]
    fn premultiplied_ui_upload_is_unpremultiplied_at_scene_boundary() {
        let upload = UiTextureUpload {
            id: UiTextureId(2),
            generation: 1,
            width: 2,
            height: 1,
            format: UiTextureFormat::Rgba8SrgbPremultiplied,
            content_hash: Hash256::from_sha256(&[100, 50, 25, 128, 0, 0, 0, 0]),
            pixels: vec![100, 50, 25, 128, 0, 0, 0, 0].into(),
        };

        assert_eq!(
            scene_texture_rgba8(&upload),
            vec![199, 100, 50, 128, 0, 0, 0, 0]
        );
    }

    #[astra_headless_test::test]
    fn texture_resource_identity_is_stable_across_ui_render_generations() {
        let first = texture_resource_id_for_session("vn.ui.demo:0", UiTextureId(7), 3);
        let repeated = texture_resource_id_for_session("vn.ui.demo:0", UiTextureId(7), 3);
        let updated = texture_resource_id_for_session("vn.ui.demo:0", UiTextureId(7), 4);

        assert_eq!(first, repeated);
        assert_ne!(first, updated);
    }

    #[astra_headless_test::test]
    fn identical_recreated_managed_texture_reuses_the_live_scene_resource() {
        let mut converter = YakuiPaintConverter::new();
        let texture = Texture::new(TextureFormat::R8, UVec2::new(2, 1), vec![42, 84]);
        let mut initial = Vec::new();
        converter
            .sync_managed_texture("ManagedTextureId(1)".into(), &texture, false, &mut initial)
            .unwrap();
        let first = converter
            .managed_textures
            .get("ManagedTextureId(1)")
            .copied()
            .unwrap();
        assert_eq!(initial.len(), 1);

        converter
            .remove_managed_texture("ManagedTextureId(1)")
            .unwrap();
        let mut replacement = Vec::new();
        converter
            .sync_managed_texture(
                "ManagedTextureId(2)".into(),
                &texture,
                false,
                &mut replacement,
            )
            .unwrap();
        let second = converter
            .managed_textures
            .get("ManagedTextureId(2)")
            .copied()
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(first.generation, second.generation);
        assert!(replacement.is_empty());
        assert!(converter.release_unreferenced_textures().is_empty());
    }

    #[astra_headless_test::test]
    fn changed_managed_texture_uploads_new_content_and_releases_old_resource() {
        let mut converter = YakuiPaintConverter::new();
        let first_texture = Texture::new(TextureFormat::R8, UVec2::new(1, 1), vec![42]);
        let second_texture = Texture::new(TextureFormat::R8, UVec2::new(1, 1), vec![84]);
        let mut initial = Vec::new();
        converter
            .sync_managed_texture(
                "ManagedTextureId(1)".into(),
                &first_texture,
                false,
                &mut initial,
            )
            .unwrap();
        let old = initial[0].id;
        converter
            .remove_managed_texture("ManagedTextureId(1)")
            .unwrap();
        let mut replacement = Vec::new();
        converter
            .sync_managed_texture(
                "ManagedTextureId(1)".into(),
                &second_texture,
                false,
                &mut replacement,
            )
            .unwrap();

        assert_eq!(replacement.len(), 1);
        assert_ne!(replacement[0].id, old);
        let releases = converter.release_unreferenced_textures();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].id, old);
    }

    #[astra_headless_test::test]
    fn full_resync_releases_live_resources_before_reusing_their_identity() {
        let mut converter = YakuiPaintConverter::new();
        let texture = Texture::new(TextureFormat::R8, UVec2::new(1, 1), vec![42]);
        let mut initial = Vec::new();
        converter
            .sync_managed_texture(
                "ManagedTextureId(1)".into(),
                &texture,
                false,
                &mut initial,
            )
            .unwrap();
        let binding = converter
            .managed_textures
            .get("ManagedTextureId(1)")
            .copied()
            .unwrap();

        let releases = converter.release_live_textures_for_resync().unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].id, binding.id);
        assert_eq!(releases[0].generation, binding.generation);

        let mut replay = Vec::new();
        converter
            .sync_managed_texture(
                "ManagedTextureId(1)".into(),
                &texture,
                true,
                &mut replay,
            )
            .unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].id, binding.id);
        assert_eq!(replay[0].generation, binding.generation + 1);
    }
}
