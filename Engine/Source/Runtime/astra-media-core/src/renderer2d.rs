use std::{
    collections::{BTreeMap, HashSet},
    ops::Deref,
    sync::Arc,
};

use astra_core::Hash256;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use thiserror::Error;

#[cfg(test)]
mod tests;

mod media_error;
pub use media_error::BlendMode;
pub use media_error::CpuFrame;
pub use media_error::GlyphBitmap;
pub use media_error::GlyphBitmapFormat;
pub use media_error::MediaError;
pub use media_error::MeshDraw2D;
pub use media_error::MeshMaterial2D;
pub use media_error::MeshVertex2D;
pub use media_error::OwnedPixelBuffer;
pub use media_error::RectI;
pub use media_error::RenderTargetFormat;
pub use media_error::RendererCreateRequest;
pub use media_error::RendererDescriptor;
pub use media_error::SceneCompositing2D;
pub use media_error::TextureFilter2D;
pub use media_error::TextureFrame;
pub use media_error::Transform2D;
mod scene_command;
pub use scene_command::CpuRendererProvider;
pub use scene_command::DrawCommand;
pub use scene_command::GlyphInstance;
pub use scene_command::HeadlessRenderer;
pub use scene_command::Renderer2D;
pub use scene_command::Renderer2DProvider;
pub use scene_command::SceneCommand;
mod headless_renderer;
pub use headless_renderer::RetainedResourceJournal;
mod pending_resource;
use pending_resource::apply_texture_region;
use pending_resource::retained_glyph_exists;
use pending_resource::retained_resource_exists_indexed;
use pending_resource::retained_texture_dimensions;
use pending_resource::validate_source_rect_dimensions;
use pending_resource::validate_texture_region;
use pending_resource::PendingResource;
mod headless_renderer_operations;
use headless_renderer_operations::validate_opacity;
use headless_renderer_operations::validate_resource_payloads;
use headless_renderer_operations::validate_transform;
mod validate_texture_metadata;
use validate_texture_metadata::crop_texture;
use validate_texture_metadata::draw_glyph;
use validate_texture_metadata::draw_mesh;
use validate_texture_metadata::draw_solid;
use validate_texture_metadata::draw_texture;
use validate_texture_metadata::validate_glyph;
use validate_texture_metadata::validate_glyph_dimensions;
use validate_texture_metadata::validate_glyph_metadata;
use validate_texture_metadata::validate_texture;
use validate_texture_metadata::validate_texture_dimensions;
use validate_texture_metadata::validate_texture_metadata;
mod blend_premultiplied_pixel;
use blend_premultiplied_pixel::blend_pixel;
use blend_premultiplied_pixel::blend_premultiplied_pixel;
pub use blend_premultiplied_pixel::frame_hash;
use blend_premultiplied_pixel::intersection;
use blend_premultiplied_pixel::transformed_bounds;
