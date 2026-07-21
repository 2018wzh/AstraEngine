use std::{collections::BTreeSet, sync::Arc};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_id, UiRect, UiValidationError, UiViewport, ValidateUi, MAX_DRAW_CALLS,
    MAX_INDICES_PER_FRAME, MAX_TEXTURE_BYTES, MAX_VERTICES_PER_FRAME,
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct UiTextureId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiTextureFormat {
    R8Unorm,
    Rgba8SrgbPremultiplied,
}

impl UiTextureFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::R8Unorm => 1,
            Self::Rgba8SrgbPremultiplied => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiTextureUpload {
    pub id: UiTextureId,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub format: UiTextureFormat,
    pub pixels: Arc<[u8]>,
    pub content_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiTextureRelease {
    pub id: UiTextureId,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiTextureDelta {
    pub uploads: Vec<UiTextureUpload>,
    pub releases: Vec<UiTextureRelease>,
    pub full_resync: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiVertex {
    pub position_points: [f32; 2],
    pub uv: [f32; 2],
    pub premultiplied_rgba: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiMaterialKind {
    SolidColor,
    ColorTexture,
    GlyphMask,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiMeshPrimitive {
    pub id: String,
    pub layer: i32,
    pub clip_rect_points: Option<UiRect>,
    pub material: UiMaterialKind,
    pub texture: Option<UiTextureId>,
    pub texture_generation: Option<u64>,
    pub vertices: Vec<UiVertex>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiRenderFrame {
    pub schema: String,
    pub session_id: String,
    pub generation: u64,
    pub viewport: UiViewport,
    pub textures: UiTextureDelta,
    pub primitives: Vec<UiMeshPrimitive>,
}

impl ValidateUi for UiRenderFrame {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_render_frame.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_RENDER_SCHEMA",
                "render frame schema must be astra.ui_render_frame.v1",
            ));
        }
        validate_id("render.session_id", &self.session_id)?;
        self.viewport.validate()?;
        if self.primitives.len() > MAX_DRAW_CALLS {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_DRAW_CALL_LIMIT",
                format!("draw call count exceeds {MAX_DRAW_CALLS}"),
            ));
        }
        let mut texture_ops = BTreeSet::new();
        let mut uploaded = BTreeSet::new();
        let mut texture_bytes = 0usize;
        for upload in &self.textures.uploads {
            if upload.width == 0 || upload.height == 0 {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXTURE_DIMENSIONS",
                    "texture dimensions must be positive",
                ));
            }
            let expected =
                upload.width as usize * upload.height as usize * upload.format.bytes_per_pixel();
            if expected != upload.pixels.len() {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXTURE_LENGTH",
                    "texture byte length does not match dimensions and format",
                ));
            }
            if Hash256::from_sha256(&upload.pixels) != upload.content_hash {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXTURE_HASH",
                    "texture content hash mismatch",
                ));
            }
            if upload.format == UiTextureFormat::Rgba8SrgbPremultiplied
                && upload
                    .pixels
                    .chunks_exact(4)
                    .any(|pixel| pixel[0] > pixel[3] || pixel[1] > pixel[3] || pixel[2] > pixel[3])
            {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXTURE_ALPHA",
                    "RGBA texture is not premultiplied",
                ));
            }
            let key = (upload.id, upload.generation);
            if !texture_ops.insert(key) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXTURE_DUPLICATE",
                    "duplicate texture operation in frame",
                ));
            }
            uploaded.insert(key);
            texture_bytes = texture_bytes.saturating_add(upload.pixels.len());
        }
        for release in &self.textures.releases {
            if !texture_ops.insert((release.id, release.generation)) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXTURE_CONFLICT",
                    "texture cannot be uploaded and released in one transaction",
                ));
            }
        }
        if texture_bytes > MAX_TEXTURE_BYTES {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_TEXTURE_BUDGET",
                format!("texture updates exceed {MAX_TEXTURE_BYTES} bytes"),
            ));
        }
        let mut vertices = 0usize;
        let mut indices = 0usize;
        let mut primitive_ids = BTreeSet::new();
        for primitive in &self.primitives {
            validate_id("render.primitive.id", &primitive.id)?;
            if !primitive_ids.insert(primitive.id.as_str()) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_PRIMITIVE_DUPLICATE",
                    format!("duplicate primitive id {}", primitive.id),
                ));
            }
            if primitive.vertices.is_empty()
                || primitive.indices.is_empty()
                || primitive.indices.len() % 3 != 0
            {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_MESH_EMPTY",
                    "mesh must contain vertices and triangle indices",
                ));
            }
            if primitive
                .indices
                .iter()
                .any(|index| *index as usize >= primitive.vertices.len())
            {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_MESH_INDEX",
                    "mesh index is outside the vertex buffer",
                ));
            }
            if primitive.vertices.iter().any(|vertex| {
                vertex
                    .position_points
                    .iter()
                    .any(|value| !value.is_finite())
                    || vertex.uv.iter().any(|value| !value.is_finite())
            }) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_MESH_NON_FINITE",
                    "mesh contains non-finite coordinates",
                ));
            }
            if primitive
                .clip_rect_points
                .is_some_and(|rect| !rect.is_finite_and_ordered())
            {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_MESH_CLIP",
                    "mesh clip rectangle is invalid",
                ));
            }
            match (
                primitive.material,
                primitive.texture,
                primitive.texture_generation,
            ) {
                (UiMaterialKind::SolidColor, None, None) => {}
                (UiMaterialKind::ColorTexture | UiMaterialKind::GlyphMask, Some(_), Some(_)) => {}
                _ => {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_MESH_MATERIAL",
                        "material and texture reference do not match",
                    ));
                }
            }
            vertices += primitive.vertices.len();
            indices += primitive.indices.len();
        }
        if vertices > MAX_VERTICES_PER_FRAME || indices > MAX_INDICES_PER_FRAME {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_MESH_BUDGET",
                "frame mesh budget exceeded",
            ));
        }
        crate::validate_serialized_size(self)
    }
}
