mod walk;

mod id_set;

mod validate;

mod pack;

mod gpu;

mod allocator;

mod upload;

mod vertices;

mod pipelines;

pub(crate) use allocator::*;

pub(crate) use gpu::*;

pub(crate) use pack::*;

pub(crate) use pipelines::*;

pub(crate) use upload::*;

pub(crate) use validate::*;

pub(crate) use vertices::*;

use id_set::SceneIdSet;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use astra_media_core::{
    BlendMode, GlyphBitmap, MeshMaterial2D, MeshVertex2D, OwnedPixelBuffer, RectI, SceneCommand,
    SceneCompositing2D, TextureFilter2D, TextureFrame, Transform2D,
};

use astra_platform::{PlatformError, SceneFrame};

use sha2::{Digest, Sha256};

use smallvec::{smallvec, SmallVec};

#[cfg(test)]
mod tests;

mod atlas_state;
pub(crate) use atlas_state::AtlasAllocatorState;
pub(crate) use atlas_state::AtlasPlacement;
use atlas_state::AtlasPlacementJournal;
pub(crate) use atlas_state::AtlasPlacementView;
pub(crate) use atlas_state::AtlasResource;
pub(crate) use atlas_state::AtlasResourceView;
pub(crate) use atlas_state::AtlasSlot;
pub(crate) use atlas_state::AtlasUpdateContext;
pub(crate) use atlas_state::BlendPipelineSet;
pub(crate) use atlas_state::BlendPipelines;
pub(crate) use atlas_state::CachedOutput;
pub(crate) use atlas_state::DrawPrimitive;
pub(crate) use atlas_state::DrawQuad;
pub(crate) use atlas_state::DrawRun;
pub(crate) use atlas_state::GlyphCpuProfile;
pub(crate) use atlas_state::GlyphProfileQueries;
pub(crate) use atlas_state::GpuAtlas;
pub(crate) use atlas_state::MeshRun;
pub(crate) use atlas_state::PackedAtlas;
pub(crate) use atlas_state::PackedAtlasMutation;
pub(crate) use atlas_state::PreparedAtlasUpdate;
pub(crate) use atlas_state::PreparedGlyphFrame;
use atlas_state::PreparedRender;
pub(crate) use atlas_state::QuadSource;
use atlas_state::ResourceMutationJournal;
pub(crate) use atlas_state::RetainedTexture;
pub(crate) use atlas_state::RetainedTexturePatch;
pub(crate) use atlas_state::TextureRegionUpload;
pub(crate) use atlas_state::WgpuGlyphAtlasRenderer;
use atlas_state::ATLAS_PADDING;
use atlas_state::ATLAS_SIDE;
use atlas_state::ATLAS_STAGING_CHUNK_BYTES;
use atlas_state::MAX_ATLAS_UPLOAD_BYTES;
use atlas_state::MAX_ATLAS_WIDTH;
use atlas_state::MAX_SCENE_RESOURCES;
use atlas_state::MAX_SCENE_RESOURCE_BYTES;
use atlas_state::MIN_ATLAS_SIDE;
use atlas_state::VERTEX_STRIDE;
mod push_quad_run;
use push_quad_run::push_quad_run;
pub(crate) use push_quad_run::DrawBatch;
mod render_internal;
