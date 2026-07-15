use std::collections::BTreeMap;

use astra_core::Hash256;
use astra_media_core::{BlendMode, MeshMaterial2D, MeshVertex2D, SceneCommand, TextureFrame};
use astra_ui_core::{
    UiMaterialKind, UiMeshPrimitive, UiPoint, UiRect, UiRenderFrame, UiTextureDelta,
    UiTextureFormat, UiTextureId, UiTextureRelease, UiTextureUpload, UiValidationError, UiVertex,
    UiViewport, ValidateUi,
};
use yakui_core::paint::{PaintDom, Pipeline, TextureChange, TextureFormat};
use yakui_core::TextureId as YakuiTextureId;

#[derive(Debug, Default)]
pub struct YakuiPaintConverter {
    next_texture_id: u64,
    texture_ids: BTreeMap<String, UiTextureId>,
    texture_generations: BTreeMap<UiTextureId, u64>,
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
            for (id, texture) in textures.iter() {
                let logical = self.texture_id(YakuiTextureId::Managed(id));
                let texture_generation = self.bump_generation(logical);
                uploads.push(texture_upload(logical, texture_generation, texture)?);
            }
        } else {
            for (id, change) in textures.edits() {
                let key = texture_key(YakuiTextureId::Managed(id));
                match change {
                    TextureChange::Added | TextureChange::Modified => {
                        let logical = self.texture_id(YakuiTextureId::Managed(id));
                        let texture_generation = self.bump_generation(logical);
                        let texture = textures.get(id).ok_or_else(|| {
                            UiValidationError::invalid(
                                "ASTRA_UI_YAKUI_TEXTURE_MISSING",
                                "Yakui reported an added/modified texture without storage",
                            )
                        })?;
                        uploads.push(texture_upload(logical, texture_generation, texture)?);
                    }
                    TextureChange::Removed => {
                        let logical = self.texture_ids.remove(&key).ok_or_else(|| {
                            UiValidationError::invalid(
                                "ASTRA_UI_YAKUI_TEXTURE_UNKNOWN",
                                "Yakui removed an unknown texture",
                            )
                        })?;
                        let texture_generation =
                            self.texture_generations.remove(&logical).ok_or_else(|| {
                                UiValidationError::invalid(
                                    "ASTRA_UI_YAKUI_TEXTURE_GENERATION",
                                    "removed texture has no live generation",
                                )
                            })?;
                        releases.push(UiTextureRelease {
                            id: logical,
                            generation: texture_generation,
                        });
                    }
                }
            }
        }

        let mut primitives = Vec::new();
        let width = viewport.physical_width as f32;
        let height = viewport.physical_height as f32;
        for (layer_index, layer) in paint.layers().iter().enumerate() {
            for (call_index, call) in layer.calls.iter().enumerate() {
                let texture = call.texture.map(|id| self.texture_id(id));
                let texture_generation =
                    texture.map(|id| self.texture_generations.get(&id).copied().unwrap_or(0));
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

    fn texture_id(&mut self, id: YakuiTextureId) -> UiTextureId {
        match id {
            YakuiTextureId::User(value) => UiTextureId(value | (1_u64 << 63)),
            YakuiTextureId::Managed(_) => {
                let key = texture_key(id);
                if let Some(value) = self.texture_ids.get(&key) {
                    *value
                } else {
                    let value = UiTextureId(self.next_texture_id);
                    self.next_texture_id = self.next_texture_id.saturating_add(1);
                    self.texture_ids.insert(key, value);
                    value
                }
            }
        }
    }

    fn bump_generation(&mut self, id: UiTextureId) -> u64 {
        let generation = self.texture_generations.entry(id).or_insert(0);
        *generation = generation.saturating_add(1);
        *generation
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
        pixels,
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
        let rgba8 = match upload.format {
            UiTextureFormat::Rgba8SrgbPremultiplied => upload.pixels.clone(),
            UiTextureFormat::R8Unorm => upload
                .pixels
                .iter()
                .flat_map(|value| [255, 255, 255, *value])
                .collect(),
        };
        commands.push(SceneCommand::UploadTexture {
            resource_id: texture_resource_id(frame, upload.id, upload.generation),
            frame: TextureFrame {
                width: upload.width,
                height: upload.height,
                hash: Hash256::from_sha256(&rgba8),
                rgba8,
            },
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
                .collect(),
            indices: primitive.indices.clone(),
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

fn texture_resource_id(frame: &UiRenderFrame, id: UiTextureId, generation: u64) -> String {
    format!(
        "ui:{}/{}/{}/{}",
        frame.session_id, frame.generation, id.0, generation
    )
}
