use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use astra_media_core::{
    BlendMode, GlyphBitmap, GlyphBitmapFormat, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand,
    TextureFrame, Transform2D,
};
use astra_platform::{PlatformError, PlatformErrorCode, SceneFrame};
use sha2::{Digest, Sha256};
use smallvec::{smallvec, SmallVec};

const ATLAS_SIDE: u32 = 4096;
const MIN_ATLAS_SIDE: u32 = 1024;
const ATLAS_PADDING: u32 = 1;
const MAX_GLYPH_RESOURCES: usize = 65_536;
const MAX_GLYPH_BYTES: usize = 64 * 1024 * 1024;
const MAX_ATLAS_UPLOAD_BYTES: usize = ATLAS_SIDE as usize * ATLAS_SIDE as usize * 4;
const ATLAS_STAGING_RING_SIZE: usize = 1;
const ATLAS_STAGING_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const VERTEX_STRIDE: wgpu::BufferAddress = 32;

pub(crate) struct WgpuGlyphAtlasRenderer {
    resources: BTreeMap<String, AtlasResource>,
    atlas: Option<GpuAtlas>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    output: Option<CachedOutput>,
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_capacity: u64,
    last_upload_bytes: u64,
    last_draw_calls: u64,
    last_engine_allocation_bytes: u64,
    last_engine_allocation_count: u64,
    last_command_allocation_bytes: u64,
    last_command_allocation_breakdown: Option<[u64; 4]>,
    command_allocation_diagnostics_emitted: u32,
    last_atlas_allocation_bytes: u64,
    last_geometry_allocation_bytes: u64,
    vertex_bytes: Vec<u8>,
    uploaded_vertex_bytes: Vec<u8>,
    draw_batches: Vec<DrawBatch>,
    atlas_upload_pixels: Vec<u8>,
    atlas_staging_belts: [wgpu::util::StagingBelt; ATLAS_STAGING_RING_SIZE],
    atlas_staging_index: usize,
    reserved_side: Option<u32>,
}

struct CachedOutput {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
}

pub(crate) struct PreparedGlyphFrame {
    pub(crate) texture: wgpu::Texture,
    pub(crate) cpu_profile: GlyphCpuProfile,
    resource_mutations: ResourceMutationJournal,
    atlas_update: PreparedAtlasUpdate,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct GlyphCpuProfile {
    pub(crate) command_ns: u64,
    pub(crate) atlas_ns: u64,
    pub(crate) geometry_ns: u64,
    pub(crate) vertex_upload_ns: u64,
    pub(crate) render_encode_ns: u64,
    pub(crate) queue_submit_ns: u64,
    pub(crate) render_submit_ns: u64,
}

pub(crate) struct GlyphProfileQueries<'a> {
    pub(crate) query_set: &'a wgpu::QuerySet,
    pub(crate) atlas_upload: Option<(u32, u32)>,
    pub(crate) scene: (u32, u32),
}

struct AtlasUpdateContext<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    layout: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    staging_belt: &'a mut wgpu::util::StagingBelt,
    timestamp_query: Option<(&'a wgpu::QuerySet, u32, u32)>,
}

type PreparedRender = (
    wgpu::Texture,
    ResourceMutationJournal,
    PreparedAtlasUpdate,
    GlyphCpuProfile,
);

#[derive(Clone, PartialEq, Eq)]
enum AtlasResource {
    Glyph(Arc<GlyphBitmap>),
    Texture(Arc<TextureFrame>),
}

type ResourceMutationJournal = BTreeMap<String, Option<AtlasResource>>;

struct AtlasResourceView<'a> {
    base: &'a BTreeMap<String, AtlasResource>,
    mutations: &'a ResourceMutationJournal,
}

impl<'a> AtlasResourceView<'a> {
    fn new(
        base: &'a BTreeMap<String, AtlasResource>,
        mutations: &'a ResourceMutationJournal,
    ) -> Self {
        Self { base, mutations }
    }

    fn get(&self, resource_id: &str) -> Option<&AtlasResource> {
        match self.mutations.get(resource_id) {
            Some(resource) => resource.as_ref(),
            None => self.base.get(resource_id),
        }
    }

    fn contains_key(&self, resource_id: &str) -> bool {
        self.get(resource_id).is_some()
    }

    fn len(&self) -> usize {
        self.base.len()
            + self
                .mutations
                .iter()
                .filter(|(id, resource)| resource.is_some() && !self.base.contains_key(*id))
                .count()
            - self
                .mutations
                .iter()
                .filter(|(id, resource)| resource.is_none() && self.base.contains_key(*id))
                .count()
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &AtlasResource)> {
        self.base
            .iter()
            .filter(|(id, _)| !self.mutations.contains_key(*id))
            .chain(
                self.mutations
                    .iter()
                    .filter_map(|(id, resource)| resource.as_ref().map(|resource| (id, resource))),
            )
    }

    fn values(&self) -> impl Iterator<Item = &AtlasResource> {
        self.iter().map(|(_, resource)| resource)
    }
}

struct GpuAtlas {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    packed: PackedAtlas,
}

enum PreparedAtlasUpdate {
    None,
    Replace(GpuAtlas),
    Mutate(PackedAtlasMutation),
}

type AtlasPlacementJournal = BTreeMap<String, Option<AtlasPlacement>>;

struct PackedAtlasMutation {
    placements: AtlasPlacementJournal,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    consumed_free_slots: BTreeSet<usize>,
    added_free_slots: Vec<AtlasSlot>,
    freed_area: u64,
}

struct AtlasAllocatorState<'a> {
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    base_free_slots: &'a [AtlasSlot],
    consumed_free_slots: BTreeSet<usize>,
    added_free_slots: Vec<AtlasSlot>,
    freed_area: u64,
}

impl<'a> AtlasAllocatorState<'a> {
    fn new(packed: &'a PackedAtlas) -> Self {
        Self {
            width: packed.width,
            height: packed.height,
            cursor_x: packed.cursor_x,
            cursor_y: packed.cursor_y,
            row_height: packed.row_height,
            base_free_slots: &packed.free_slots,
            consumed_free_slots: BTreeSet::new(),
            added_free_slots: Vec::new(),
            freed_area: packed.freed_area,
        }
    }
}

struct AtlasPlacementView<'a> {
    packed: &'a PackedAtlas,
    mutations: Option<&'a AtlasPlacementJournal>,
}

impl<'a> AtlasPlacementView<'a> {
    fn committed(packed: &'a PackedAtlas) -> Self {
        Self {
            packed,
            mutations: None,
        }
    }

    fn pending(packed: &'a PackedAtlas, mutations: &'a AtlasPlacementJournal) -> Self {
        Self {
            packed,
            mutations: Some(mutations),
        }
    }

    fn get(&self, resource_id: &str) -> Option<&AtlasPlacement> {
        match self
            .mutations
            .and_then(|mutations| mutations.get(resource_id))
        {
            Some(placement) => placement.as_ref(),
            None => self.packed.placements.get(resource_id),
        }
    }

    fn width(&self) -> u32 {
        self.packed.width
    }

    fn height(&self) -> u32 {
        self.packed.height
    }
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

#[derive(Clone, Copy)]
struct AtlasPlacement {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone)]
struct PackedAtlas {
    placements: BTreeMap<String, AtlasPlacement>,
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    free_slots: Vec<AtlasSlot>,
    freed_area: u64,
}

#[derive(Clone, Copy)]
struct AtlasSlot {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

enum QuadSource<'a> {
    Resource {
        resource_id: Cow<'a, str>,
        source: RectI,
    },
    White,
}

struct DrawQuad<'a> {
    source: QuadSource<'a>,
    destination: RectI,
    rotation_quadrants: u8,
}

struct DrawRun<'a> {
    // Classic text runs regularly contain more than eight glyphs. Keeping the
    // expected bounded run inline avoids a per-frame growth allocation while
    // retaining an explicit spill path for authored long text.
    quads: SmallVec<[DrawQuad<'a>; 32]>,
    rgba: [u8; 4],
    opacity: f32,
    clip: RectI,
    transform: Transform2D,
}

struct MeshRun<'a> {
    vertices: &'a [MeshVertex2D],
    indices: &'a [u32],
    texture_id: Option<&'a str>,
    opacity: f32,
    clip: RectI,
    transform: Transform2D,
}

enum DrawPrimitive<'a> {
    Quads(usize),
    Mesh(MeshRun<'a>),
}

fn push_quad_run<'a>(
    quad_runs: &mut SmallVec<[DrawRun<'a>; 64]>,
    primitives: &mut SmallVec<[DrawPrimitive<'a>; 64]>,
    run: DrawRun<'a>,
) {
    let index = quad_runs.len();
    quad_runs.push(run);
    primitives.push(DrawPrimitive::Quads(index));
}

struct DrawBatch {
    first_vertex: u32,
    vertex_count: u32,
    clip: RectI,
}

impl WgpuGlyphAtlasRenderer {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self::new_internal(device, None)
    }

    pub(crate) fn new_reserved(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        side: u32,
    ) -> Result<Self, PlatformError> {
        if !(MIN_ATLAS_SIDE..=ATLAS_SIDE).contains(&side) || !side.is_power_of_two() {
            return Err(invalid(
                "glyph atlas reservation side is outside the supported range",
            ));
        }
        let mut renderer = Self::new_internal(device, Some(side));
        renderer.atlas = Some(create_reserved_gpu_atlas(
            device,
            queue,
            &renderer.layout,
            &renderer.sampler,
            side,
        ));
        Ok(renderer)
    }

    fn new_internal(device: &wgpu::Device, reserved_side: Option<u32>) -> Self {
        let (layout, sampler, pipeline) = create_pipeline(device);
        Self {
            resources: BTreeMap::new(),
            atlas: None,
            layout,
            sampler,
            pipeline,
            output: None,
            vertex_buffer: None,
            vertex_capacity: 0,
            last_upload_bytes: 0,
            last_draw_calls: 0,
            last_engine_allocation_bytes: 0,
            last_engine_allocation_count: 0,
            last_command_allocation_bytes: 0,
            last_command_allocation_breakdown: None,
            command_allocation_diagnostics_emitted: 0,
            last_atlas_allocation_bytes: 0,
            last_geometry_allocation_bytes: 0,
            vertex_bytes: Vec::new(),
            uploaded_vertex_bytes: Vec::new(),
            draw_batches: Vec::new(),
            atlas_upload_pixels: Vec::with_capacity(ATLAS_STAGING_CHUNK_BYTES as usize),
            atlas_staging_belts: std::array::from_fn(|_| primed_staging_belt(device)),
            atlas_staging_index: 0,
            reserved_side,
        }
    }

    pub(super) fn recover(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (layout, sampler, pipeline) = create_pipeline(device);
        self.layout = layout;
        self.sampler = sampler;
        self.pipeline = pipeline;
        self.atlas = self.reserved_side.map(|side| {
            create_reserved_gpu_atlas(device, queue, &self.layout, &self.sampler, side)
        });
        self.output = None;
        self.vertex_buffer = None;
        self.vertex_capacity = 0;
        self.uploaded_vertex_bytes.clear();
        self.atlas_staging_belts = std::array::from_fn(|_| primed_staging_belt(device));
        self.atlas_staging_index = 0;
        self.last_upload_bytes = 0;
        self.last_draw_calls = 0;
        self.last_engine_allocation_bytes = 0;
        self.last_engine_allocation_count = 0;
        self.last_command_allocation_bytes = 0;
        self.last_command_allocation_breakdown = None;
        self.command_allocation_diagnostics_emitted = 0;
        self.last_atlas_allocation_bytes = 0;
        self.last_geometry_allocation_bytes = 0;
    }

    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
    ) -> Result<PreparedGlyphFrame, PlatformError> {
        let (texture, resource_mutations, atlas_update, cpu_profile) =
            self.render_internal(device, queue, frame, true, None, None)?;
        Ok(PreparedGlyphFrame {
            texture,
            cpu_profile,
            resource_mutations,
            atlas_update,
        })
    }

    pub(crate) fn render_profiled(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        queries: GlyphProfileQueries<'_>,
    ) -> Result<PreparedGlyphFrame, PlatformError> {
        let (texture, resource_mutations, atlas_update, cpu_profile) = self.render_internal(
            device,
            queue,
            frame,
            true,
            queries
                .atlas_upload
                .map(|(begin, end)| (queries.query_set, begin, end)),
            Some((queries.query_set, queries.scene.0, queries.scene.1)),
        )?;
        Ok(PreparedGlyphFrame {
            texture,
            cpu_profile,
            resource_mutations,
            atlas_update,
        })
    }

    pub(super) fn render_retained(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
    ) -> Result<wgpu::Texture, PlatformError> {
        self.render_internal(device, queue, frame, false, None, None)
            .map(|(texture, _, _, _)| texture)
    }

    pub(crate) fn commit(&mut self, prepared: PreparedGlyphFrame) -> wgpu::Texture {
        for (resource_id, resource) in prepared.resource_mutations {
            match resource {
                Some(resource) => {
                    self.resources.insert(resource_id, resource);
                }
                None => {
                    self.resources.remove(&resource_id);
                }
            }
        }
        match prepared.atlas_update {
            PreparedAtlasUpdate::None => {}
            PreparedAtlasUpdate::Replace(atlas) => self.atlas = Some(atlas),
            PreparedAtlasUpdate::Mutate(mutation) => {
                let atlas = self
                    .atlas
                    .as_mut()
                    .expect("incremental atlas mutation requires a committed atlas");
                for (resource_id, placement) in mutation.placements {
                    match placement {
                        Some(placement) => {
                            atlas.packed.placements.insert(resource_id, placement);
                        }
                        None => {
                            atlas.packed.placements.remove(&resource_id);
                        }
                    }
                }
                atlas.packed.cursor_x = mutation.cursor_x;
                atlas.packed.cursor_y = mutation.cursor_y;
                atlas.packed.row_height = mutation.row_height;
                for index in mutation.consumed_free_slots.into_iter().rev() {
                    atlas.packed.free_slots.remove(index);
                }
                atlas.packed.free_slots.extend(mutation.added_free_slots);
                atlas.packed.free_slots.sort_by_key(atlas_slot_sort_key);
                atlas.packed.freed_area = mutation.freed_area;
            }
        }
        prepared.texture
    }

    pub(crate) fn resource_bytes(&self) -> u64 {
        let atlas = self.atlas.as_ref().map_or(0, |atlas| {
            u64::from(atlas.packed.width) * u64::from(atlas.packed.height) * 4
        });
        let output = self.output.as_ref().map_or(0, |output| {
            u64::from(output.width) * u64::from(output.height) * 4
        });
        atlas + output + self.vertex_capacity
    }

    pub(crate) fn atlas_bytes(&self) -> u64 {
        self.atlas.as_ref().map_or(0, |atlas| {
            u64::from(atlas.packed.width) * u64::from(atlas.packed.height) * 4
        })
    }

    pub(crate) fn last_upload_bytes(&self) -> u64 {
        self.last_upload_bytes
    }

    pub(crate) fn last_draw_calls(&self) -> u64 {
        self.last_draw_calls
    }

    pub(crate) fn last_engine_allocation_bytes(&self) -> u64 {
        self.last_engine_allocation_bytes
    }

    pub(crate) fn last_engine_allocation_count(&self) -> u64 {
        self.last_engine_allocation_count
    }

    pub(crate) fn last_command_allocation_bytes(&self) -> u64 {
        self.last_command_allocation_bytes
    }

    pub(crate) fn last_atlas_allocation_bytes(&self) -> u64 {
        self.last_atlas_allocation_bytes
    }

    pub(crate) fn last_geometry_allocation_bytes(&self) -> u64 {
        self.last_geometry_allocation_bytes
    }

    pub(crate) fn requires_atlas_update(&self, frame: &SceneFrame) -> bool {
        self.atlas.is_none()
            || frame.commands.iter().any(|command| {
                matches!(
                    command,
                    SceneCommand::UploadTexture { .. }
                        | SceneCommand::UploadGlyph { .. }
                        | SceneCommand::ReleaseResource { .. }
                        | SceneCommand::Texture { .. }
                        | SceneCommand::VideoFrame { .. }
                        | SceneCommand::Glyph { .. }
                )
            })
    }

    fn render_internal(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        apply_mutations: bool,
        atlas_upload_query: Option<(&wgpu::QuerySet, u32, u32)>,
        timestamp_query: Option<(&wgpu::QuerySet, u32, u32)>,
    ) -> Result<PreparedRender, PlatformError> {
        let profile_cpu = timestamp_query.is_some();
        let command_started = Instant::now();
        let engine_allocation_before = astra_observability::thread_allocation_snapshot();
        validate_scene_resource_payloads(&frame.commands)?;
        let resource_validation_allocation = astra_observability::thread_allocation_snapshot();
        let mut render_mutations = ResourceMutationJournal::new();
        let mut committed_mutations = ResourceMutationJournal::new();
        macro_rules! resource_get {
            ($id:expr) => {
                match render_mutations.get($id) {
                    Some(resource) => resource.as_ref(),
                    None => self.resources.get($id),
                }
            };
        }
        macro_rules! resource_contains {
            ($id:expr) => {
                resource_get!($id).is_some()
            };
        }
        let mut resources_changed = self.atlas.is_none();
        let mut quad_runs: SmallVec<[DrawRun<'_>; 64]> = SmallVec::new();
        let mut draw_runs: SmallVec<[DrawPrimitive<'_>; 64]> = SmallVec::new();
        let mut clip_stack: SmallVec<[RectI; 16]> = SmallVec::new();
        let mut transform_stack: SmallVec<[Transform2D; 16]> = smallvec![Transform2D::IDENTITY];
        let mut camera = Transform2D::IDENTITY;
        let mut opacity_stack: SmallVec<[f32; 16]> = smallvec![1.0_f32];
        let mut transient_sequence = 0_u64;
        // The Classic UI's retained scene contains more than 64 uniquely named
        // primitives. Keep the common bounded scene entirely on the stack so
        // static 120 Hz frames do not allocate merely while validating IDs.
        // Larger authored scenes still remain bounded by their declared frame
        // budget and may spill explicitly rather than silently dropping draws.
        let mut run_ids: SmallVec<[&str; 128]> = SmallVec::new();
        let mut drawn_resources: SmallVec<[&str; 128]> = SmallVec::new();
        let mut upload_texture_count = 0_u32;
        let mut upload_glyph_count = 0_u32;
        let mut release_resource_count = 0_u32;
        let mut transient_texture_count = 0_u32;
        let mut transient_glyph_count = 0_u32;
        let mut mesh_command_allocation_bytes = 0_u64;
        let mut mesh_command_allocation_count = 0_u64;
        let command_setup_allocation = astra_observability::thread_allocation_snapshot();
        for command in &frame.commands {
            match command {
                SceneCommand::UploadTexture { resource_id, frame } => {
                    upload_texture_count += 1;
                    validate_resource_id(resource_id)?;
                    validate_texture_metadata(frame)?;
                    if apply_mutations {
                        if committed_mutations.contains_key(resource_id) {
                            return Err(invalid(
                                "texture resource id is mutated more than once in a frame",
                            ));
                        }
                        if resource_contains!(resource_id) {
                            return Err(invalid("texture upload repeats a live resource id"));
                        }
                        let resource = AtlasResource::Texture(Arc::new(frame.clone()));
                        render_mutations.insert(resource_id.clone(), Some(resource.clone()));
                        committed_mutations.insert(resource_id.clone(), Some(resource));
                        resources_changed = true;
                    } else if resource_get!(resource_id)
                        != Some(&AtlasResource::Texture(Arc::new(frame.clone())))
                    {
                        return Err(invalid(
                            "retained texture resource does not match the recovery frame",
                        ));
                    }
                }
                SceneCommand::UploadGlyph { resource_id, glyph } => {
                    upload_glyph_count += 1;
                    validate_resource_id(resource_id)?;
                    validate_glyph_metadata(glyph)?;
                    if apply_mutations {
                        if committed_mutations.contains_key(resource_id) {
                            return Err(invalid(
                                "glyph resource id is mutated more than once in a frame",
                            ));
                        }
                        if resource_contains!(resource_id) {
                            return Err(invalid("glyph upload repeats a live resource id"));
                        }
                        let resource = AtlasResource::Glyph(Arc::new(glyph.clone()));
                        render_mutations.insert(resource_id.clone(), Some(resource.clone()));
                        committed_mutations.insert(resource_id.clone(), Some(resource));
                        resources_changed = true;
                    } else if resource_get!(resource_id)
                        != Some(&AtlasResource::Glyph(Arc::new(glyph.clone())))
                    {
                        return Err(invalid(
                            "retained glyph resource does not match the recovery frame",
                        ));
                    }
                }
                SceneCommand::ReleaseResource { resource_id } => {
                    release_resource_count += 1;
                    validate_resource_id(resource_id)?;
                    if apply_mutations {
                        if drawn_resources.contains(&resource_id.as_str()) {
                            return Err(invalid(
                                "glyph resource cannot be released after use in the same frame",
                            ));
                        }
                        if !resource_contains!(resource_id) {
                            return Err(invalid("glyph release references an unknown resource"));
                        }
                        render_mutations.insert(resource_id.clone(), None);
                        committed_mutations.insert(resource_id.clone(), None);
                        resources_changed = true;
                    } else if resource_contains!(resource_id) {
                        return Err(invalid(
                            "retained glyph release was not committed before recovery",
                        ));
                    }
                }
                SceneCommand::PushClip { rect } => {
                    let rect =
                        transformed_bounds(current_transform(camera, &transform_stack), *rect)?;
                    let clip = intersect_clip(
                        clip_stack.last().copied(),
                        rect,
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
                        || !insert_unique(&mut run_ids, id)
                        || !opacity.is_finite()
                        || !(0.0..=1.0).contains(opacity)
                        || *blend != BlendMode::Alpha
                    {
                        return Err(invalid(
                            "glyph run identity, opacity, or blend mode is invalid",
                        ));
                    }
                    let mut quads: SmallVec<[DrawQuad<'_>; 32]> = SmallVec::new();
                    for glyph in glyphs.iter() {
                        let bitmap = resource_get!(&glyph.resource_id).ok_or_else(|| {
                            invalid("glyph run references a resource that is not live")
                        })?;
                        let AtlasResource::Glyph(bitmap) = bitmap else {
                            return Err(invalid("glyph run references a non-glyph resource"));
                        };
                        quads.push(DrawQuad {
                            source: QuadSource::Resource {
                                resource_id: Cow::Borrowed(glyph.resource_id.as_str()),
                                source: RectI::new(0, 0, bitmap.width, bitmap.height),
                            },
                            destination: RectI::new(glyph.x, glyph.y, bitmap.width, bitmap.height),
                            rotation_quadrants: glyph.rotation_quadrants,
                        });
                        insert_unique(&mut drawn_resources, &glyph.resource_id);
                    }
                    push_quad_run(
                        &mut quad_runs,
                        &mut draw_runs,
                        DrawRun {
                            quads,
                            rgba: *rgba,
                            opacity: *opacity * opacity_stack.last().copied().unwrap_or(1.0),
                            clip: clip_stack.last().copied().unwrap_or(RectI::new(
                                0,
                                0,
                                frame.width,
                                frame.height,
                            )),
                            transform: current_transform(camera, &transform_stack),
                        },
                    );
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
                        || !insert_unique(&mut run_ids, id)
                        || !opacity.is_finite()
                        || !(0.0..=1.0).contains(opacity)
                        || *blend != BlendMode::Alpha
                    {
                        return Err(invalid(
                            "sprite identity, opacity, or blend mode is invalid",
                        ));
                    }
                    let texture = resource_get!(texture_id)
                        .ok_or_else(|| invalid("sprite references a resource that is not live"))?;
                    let AtlasResource::Texture(texture) = texture else {
                        return Err(invalid("sprite references a non-texture resource"));
                    };
                    let source = source.unwrap_or(RectI::new(0, 0, texture.width, texture.height));
                    validate_source_rect(source, texture.width, texture.height)?;
                    validate_destination(*destination)?;
                    insert_unique(&mut drawn_resources, texture_id);
                    push_quad_run(
                        &mut quad_runs,
                        &mut draw_runs,
                        DrawRun {
                            quads: smallvec![DrawQuad {
                                source: QuadSource::Resource {
                                    resource_id: Cow::Borrowed(texture_id.as_str()),
                                    source,
                                },
                                destination: *destination,
                                rotation_quadrants: 0,
                            }],
                            rgba: [255; 4],
                            opacity: *opacity * opacity_stack.last().copied().unwrap_or(1.0),
                            clip: clip_stack.last().copied().unwrap_or(RectI::new(
                                0,
                                0,
                                frame.width,
                                frame.height,
                            )),
                            transform: current_transform(camera, &transform_stack),
                        },
                    );
                }
                SceneCommand::Rect {
                    id,
                    x,
                    y,
                    width,
                    height,
                    rgba,
                } => {
                    let duplicate = run_ids.contains(&id.as_str());
                    if id.is_empty() || id.len() > 256 || duplicate || *width == 0 || *height == 0 {
                        let id_hash = format!("sha256:{:x}", Sha256::digest(id.as_bytes()));
                        tracing::error!(
                            event = "platform.wgpu.scene.rectangle_rejected",
                            id_hash,
                            id_empty = id.is_empty(),
                            id_too_long = id.len() > 256,
                            duplicate,
                            width = *width,
                            height = *height,
                            "wgpu scene rejected an invalid rectangle"
                        );
                        return Err(invalid("rectangle identity or dimensions are invalid"));
                    }
                    insert_unique(&mut run_ids, id);
                    push_quad_run(
                        &mut quad_runs,
                        &mut draw_runs,
                        DrawRun {
                            quads: smallvec![DrawQuad {
                                source: QuadSource::White,
                                destination: RectI::new(*x as i32, *y as i32, *width, *height),
                                rotation_quadrants: 0,
                            }],
                            rgba: *rgba,
                            opacity: opacity_stack.last().copied().unwrap_or(1.0),
                            clip: clip_stack.last().copied().unwrap_or(RectI::new(
                                0,
                                0,
                                frame.width,
                                frame.height,
                            )),
                            transform: current_transform(camera, &transform_stack),
                        },
                    );
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
                    let mesh_command_started = astra_observability::thread_allocation_snapshot();
                    if id.is_empty()
                        || id.len() > 256
                        || !insert_unique(&mut run_ids, id)
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
                            match resource_get!(resource_id) {
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
                            insert_unique(&mut drawn_resources, resource_id);
                            Some(resource_id.as_str())
                        }
                        _ => return Err(invalid("mesh material and texture binding mismatch")),
                    };
                    draw_runs.push(DrawPrimitive::Mesh(MeshRun {
                        vertices,
                        indices,
                        texture_id: resolved_texture,
                        opacity: *opacity * opacity_stack.last().copied().unwrap_or(1.0),
                        clip: clip_stack.last().copied().unwrap_or(RectI::new(
                            0,
                            0,
                            frame.width,
                            frame.height,
                        )),
                        transform: current_transform(camera, &transform_stack),
                    }));
                    let mesh_command_finished = astra_observability::thread_allocation_snapshot();
                    mesh_command_allocation_bytes = mesh_command_allocation_bytes.saturating_add(
                        mesh_command_finished
                            .allocated_bytes
                            .saturating_sub(mesh_command_started.allocated_bytes),
                    );
                    mesh_command_allocation_count = mesh_command_allocation_count.saturating_add(
                        mesh_command_finished
                            .allocation_count
                            .saturating_sub(mesh_command_started.allocation_count),
                    );
                }
                SceneCommand::Clear { rgba } => {
                    push_quad_run(
                        &mut quad_runs,
                        &mut draw_runs,
                        DrawRun {
                            quads: smallvec![DrawQuad {
                                source: QuadSource::White,
                                destination: RectI::new(0, 0, frame.width, frame.height),
                                rotation_quadrants: 0,
                            }],
                            rgba: *rgba,
                            opacity: 1.0,
                            clip: RectI::new(0, 0, frame.width, frame.height),
                            transform: Transform2D::IDENTITY,
                        },
                    );
                }
                SceneCommand::Texture {
                    id,
                    frame: texture,
                    destination,
                    opacity,
                    blend,
                }
                | SceneCommand::VideoFrame {
                    id,
                    frame: texture,
                    destination,
                    opacity,
                    blend,
                    ..
                } => {
                    transient_texture_count += 1;
                    validate_draw_identity(id, opacity, *blend, &mut run_ids)?;
                    validate_texture_metadata(texture)?;
                    validate_destination(*destination)?;
                    let resource_id = transient_resource_id(
                        &AtlasResourceView::new(&self.resources, &render_mutations),
                        &mut transient_sequence,
                        "texture",
                    )?;
                    render_mutations.insert(
                        resource_id.clone(),
                        Some(AtlasResource::Texture(Arc::new(texture.clone()))),
                    );
                    resources_changed = true;
                    push_quad_run(
                        &mut quad_runs,
                        &mut draw_runs,
                        DrawRun {
                            quads: smallvec![DrawQuad {
                                source: QuadSource::Resource {
                                    resource_id: Cow::Owned(resource_id.clone()),
                                    source: RectI::new(0, 0, texture.width, texture.height),
                                },
                                destination: *destination,
                                rotation_quadrants: 0,
                            }],
                            rgba: [255; 4],
                            opacity: *opacity * opacity_stack.last().copied().unwrap_or(1.0),
                            clip: clip_stack.last().copied().unwrap_or(RectI::new(
                                0,
                                0,
                                frame.width,
                                frame.height,
                            )),
                            transform: current_transform(camera, &transform_stack),
                        },
                    );
                }
                SceneCommand::Glyph {
                    id,
                    glyph,
                    x,
                    y,
                    rgba,
                    opacity,
                    blend,
                } => {
                    transient_glyph_count += 1;
                    validate_draw_identity(id, opacity, *blend, &mut run_ids)?;
                    validate_glyph_metadata(glyph)?;
                    let resource_id = transient_resource_id(
                        &AtlasResourceView::new(&self.resources, &render_mutations),
                        &mut transient_sequence,
                        "glyph",
                    )?;
                    render_mutations.insert(
                        resource_id.clone(),
                        Some(AtlasResource::Glyph(Arc::new(glyph.clone()))),
                    );
                    resources_changed = true;
                    push_quad_run(
                        &mut quad_runs,
                        &mut draw_runs,
                        DrawRun {
                            quads: smallvec![DrawQuad {
                                source: QuadSource::Resource {
                                    resource_id: Cow::Owned(resource_id.clone()),
                                    source: RectI::new(0, 0, glyph.width, glyph.height),
                                },
                                destination: RectI::new(*x, *y, glyph.width, glyph.height),
                                rotation_quadrants: 0,
                            }],
                            rgba: *rgba,
                            opacity: *opacity * opacity_stack.last().copied().unwrap_or(1.0),
                            clip: clip_stack.last().copied().unwrap_or(RectI::new(
                                0,
                                0,
                                frame.width,
                                frame.height,
                            )),
                            transform: current_transform(camera, &transform_stack),
                        },
                    );
                }
                SceneCommand::PushTransform { transform } => {
                    validate_transform(*transform)?;
                    let current = *transform_stack
                        .last()
                        .expect("transform stack is initialized");
                    transform_stack.push(compose_transform(current, *transform));
                }
                SceneCommand::PopTransform => {
                    if transform_stack.len() == 1 {
                        return Err(invalid("GPU scene transform stack underflowed"));
                    }
                    transform_stack.pop();
                }
                SceneCommand::SetCamera { transform } => {
                    validate_transform(*transform)?;
                    camera = *transform;
                }
                SceneCommand::PushOpacity { opacity } => {
                    validate_opacity(*opacity)?;
                    let current = opacity_stack.last().copied().unwrap_or(1.0);
                    opacity_stack.push(current * *opacity);
                }
                SceneCommand::PopOpacity => {
                    if opacity_stack.len() == 1 {
                        return Err(invalid("GPU scene opacity stack underflowed"));
                    }
                    opacity_stack.pop();
                }
                SceneCommand::FilterGraph { .. } => {
                    // Filter graphs are validated and executed by the offscreen
                    // owner after this scene pass. They carry no atlas mutation.
                }
            }
        }
        if !clip_stack.is_empty() || transform_stack.len() != 1 || opacity_stack.len() != 1 {
            return Err(invalid("GPU scene command stacks are not balanced"));
        }
        let command_walk_allocation = astra_observability::thread_allocation_snapshot();
        let resource_view = AtlasResourceView::new(&self.resources, &render_mutations);
        validate_resource_budget(&resource_view)?;
        let command_ns = profiled_elapsed_ns(profile_cpu, command_started)?;
        let command_allocation_after = astra_observability::thread_allocation_snapshot();
        let command_allocation_bytes = command_allocation_after
            .allocated_bytes
            .saturating_sub(engine_allocation_before.allocated_bytes);
        let command_allocation_breakdown = [
            resource_validation_allocation
                .allocated_bytes
                .saturating_sub(engine_allocation_before.allocated_bytes),
            command_setup_allocation
                .allocated_bytes
                .saturating_sub(resource_validation_allocation.allocated_bytes),
            command_walk_allocation
                .allocated_bytes
                .saturating_sub(command_setup_allocation.allocated_bytes),
            command_allocation_after
                .allocated_bytes
                .saturating_sub(command_walk_allocation.allocated_bytes),
        ];
        if command_allocation_bytes > 0
            && self.last_command_allocation_breakdown != Some(command_allocation_breakdown)
            && self.command_allocation_diagnostics_emitted < 32
        {
            self.command_allocation_diagnostics_emitted += 1;
            tracing::debug!(
                event = "platform.wgpu.scene.command_allocation",
                total_bytes = command_allocation_bytes,
                resource_validation_bytes = command_allocation_breakdown[0],
                command_setup_bytes = command_allocation_breakdown[1],
                command_walk_bytes = command_allocation_breakdown[2],
                resource_budget_bytes = command_allocation_breakdown[3],
                quad_runs = quad_runs.len(),
                quad_runs_spilled = quad_runs.spilled(),
                draw_primitives = draw_runs.len(),
                draw_primitives_spilled = draw_runs.spilled(),
                run_ids = run_ids.len(),
                run_ids_spilled = run_ids.spilled(),
                drawn_resources = drawn_resources.len(),
                drawn_resources_spilled = drawn_resources.spilled(),
                render_mutations = render_mutations.len(),
                committed_mutations = committed_mutations.len(),
                upload_texture_count,
                upload_glyph_count,
                release_resource_count,
                transient_texture_count,
                transient_glyph_count,
                mesh_command_allocation_bytes,
                mesh_command_allocation_count,
                "WGPU scene command allocation profile changed"
            );
        }
        self.last_command_allocation_bytes = command_allocation_bytes;
        self.last_command_allocation_breakdown = Some(command_allocation_breakdown);

        let atlas_started = Instant::now();
        let (atlas_update, upload_bytes) = if resources_changed {
            if atlas_upload_query.is_some() {
                device
                    .poll(wgpu::PollType::Poll)
                    .map_err(|_| invalid("glyph atlas staging ring polling failed"))?;
            }
            let staging_index = self.atlas_staging_index;
            let result = update_gpu_atlas(
                AtlasUpdateContext {
                    device,
                    queue,
                    layout: &self.layout,
                    sampler: &self.sampler,
                    staging_belt: &mut self.atlas_staging_belts[staging_index],
                    timestamp_query: atlas_upload_query,
                },
                self.atlas.as_ref(),
                &self.resources,
                &resource_view,
                &mut self.atlas_upload_pixels,
            );
            if atlas_upload_query.is_some() {
                self.atlas_staging_index = 0;
            }
            result?
        } else {
            (PreparedAtlasUpdate::None, 0)
        };
        let atlas_ns = profiled_elapsed_ns(profile_cpu, atlas_started)?;
        let atlas_allocation_after = astra_observability::thread_allocation_snapshot();
        self.last_atlas_allocation_bytes = atlas_allocation_after
            .allocated_bytes
            .saturating_sub(command_allocation_after.allocated_bytes);
        self.last_upload_bytes = upload_bytes;
        let (active_bind_group, placement_view) = match &atlas_update {
            PreparedAtlasUpdate::None => {
                let atlas = self
                    .atlas
                    .as_ref()
                    .ok_or_else(|| invalid("glyph atlas is unavailable"))?;
                (
                    &atlas.bind_group,
                    AtlasPlacementView::committed(&atlas.packed),
                )
            }
            PreparedAtlasUpdate::Replace(atlas) => (
                &atlas.bind_group,
                AtlasPlacementView::committed(&atlas.packed),
            ),
            PreparedAtlasUpdate::Mutate(mutation) => {
                let atlas = self
                    .atlas
                    .as_ref()
                    .ok_or_else(|| invalid("incremental atlas update has no base atlas"))?;
                (
                    &atlas.bind_group,
                    AtlasPlacementView::pending(&atlas.packed, &mutation.placements),
                )
            }
        };
        let geometry_started = Instant::now();
        self.vertex_bytes.clear();
        self.draw_batches.clear();
        build_vertices(
            frame,
            &quad_runs,
            &draw_runs,
            &placement_view,
            &resource_view,
            &mut self.vertex_bytes,
            &mut self.draw_batches,
        )?;
        let geometry_ns = profiled_elapsed_ns(profile_cpu, geometry_started)?;
        let engine_allocation_after = astra_observability::thread_allocation_snapshot();
        self.last_geometry_allocation_bytes = engine_allocation_after
            .allocated_bytes
            .saturating_sub(atlas_allocation_after.allocated_bytes);
        self.last_engine_allocation_bytes = engine_allocation_after
            .allocated_bytes
            .saturating_sub(engine_allocation_before.allocated_bytes);
        self.last_engine_allocation_count = engine_allocation_after
            .allocation_count
            .saturating_sub(engine_allocation_before.allocation_count);
        let vertex_upload_started = Instant::now();
        if !self.vertex_bytes.is_empty() {
            let required = self.vertex_bytes.len() as u64;
            let mut vertex_buffer_recreated = false;
            if self.vertex_capacity < required {
                let capacity = required
                    .max(4096)
                    .checked_next_power_of_two()
                    .ok_or_else(|| invalid("glyph vertex buffer capacity overflowed"))?;
                self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("astra-glyph-vertex-buffer"),
                    size: capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.vertex_capacity = capacity;
                vertex_buffer_recreated = true;
            }
            if vertex_upload_required(
                &self.vertex_bytes,
                &self.uploaded_vertex_bytes,
                vertex_buffer_recreated,
            ) {
                queue.write_buffer(
                    self.vertex_buffer
                        .as_ref()
                        .ok_or_else(|| invalid("glyph vertex buffer is unavailable"))?,
                    0,
                    &self.vertex_bytes,
                );
                self.last_upload_bytes = self
                    .last_upload_bytes
                    .checked_add(required)
                    .ok_or_else(|| invalid("GPU upload byte counter overflowed"))?;
                self.uploaded_vertex_bytes.clear();
                self.uploaded_vertex_bytes
                    .extend_from_slice(&self.vertex_bytes);
            }
        }
        let vertex_upload_ns = profiled_elapsed_ns(profile_cpu, vertex_upload_started)?;
        let render_submit_started = Instant::now();
        if self
            .output
            .as_ref()
            .is_none_or(|output| output.width != frame.width || output.height != frame.height)
        {
            self.output = Some(CachedOutput {
                width: frame.width,
                height: frame.height,
                texture: device.create_texture(&wgpu::TextureDescriptor {
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
                }),
            });
        }
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| invalid("glyph output texture is unavailable"))?
            .texture
            .clone();
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
                timestamp_writes: timestamp_query.map(|(query_set, beginning, end)| {
                    wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: Some(beginning),
                        end_of_pass_write_index: Some(end),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vertex_buffer) = &self.vertex_buffer {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, active_bind_group, &[]);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for batch in &self.draw_batches {
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
        let command_buffer = encoder.finish();
        let render_encode_ns = profiled_elapsed_ns(profile_cpu, render_submit_started)?;
        let queue_submit_started = Instant::now();
        queue.submit([command_buffer]);
        let queue_submit_ns = profiled_elapsed_ns(profile_cpu, queue_submit_started)?;
        self.last_draw_calls = self.draw_batches.len() as u64;
        let render_submit_ns = profiled_elapsed_ns(profile_cpu, render_submit_started)?;
        Ok((
            output,
            committed_mutations,
            atlas_update,
            GlyphCpuProfile {
                command_ns,
                atlas_ns,
                geometry_ns,
                vertex_upload_ns,
                render_encode_ns,
                queue_submit_ns,
                render_submit_ns,
            },
        ))
    }
}

fn profiled_elapsed_ns(enabled: bool, started: Instant) -> Result<u64, PlatformError> {
    if !enabled {
        return Ok(0);
    }
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| invalid("glyph CPU profile duration overflowed"))
}

fn vertex_upload_required(current: &[u8], uploaded: &[u8], buffer_recreated: bool) -> bool {
    buffer_recreated || current != uploaded
}

fn validate_resource_budget(resources: &AtlasResourceView<'_>) -> Result<(), PlatformError> {
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

enum SceneResourcePayload<'a> {
    Texture(&'a TextureFrame),
    Glyph(&'a GlyphBitmap),
}

impl SceneResourcePayload<'_> {
    fn byte_len(&self) -> usize {
        match self {
            Self::Texture(frame) => frame.rgba8.len(),
            Self::Glyph(glyph) => glyph.pixels.len(),
        }
    }

    fn validate(&self) -> Result<(), PlatformError> {
        match self {
            Self::Texture(frame) => validate_texture(frame),
            Self::Glyph(glyph) => validate_glyph(glyph),
        }
    }
}

fn validate_scene_resource_payloads(commands: &[SceneCommand]) -> Result<(), PlatformError> {
    const PARALLEL_PAYLOAD_BYTES: usize = 256 * 1024;
    const MAX_VALIDATION_WORKERS: usize = 4;
    let mut large = Vec::new();
    for (index, command) in commands.iter().enumerate() {
        let payload = match command {
            SceneCommand::UploadTexture { frame, .. }
            | SceneCommand::Texture { frame, .. }
            | SceneCommand::VideoFrame { frame, .. } => SceneResourcePayload::Texture(frame),
            SceneCommand::UploadGlyph { glyph, .. } | SceneCommand::Glyph { glyph, .. } => {
                SceneResourcePayload::Glyph(glyph)
            }
            _ => continue,
        };
        if payload.byte_len() >= PARALLEL_PAYLOAD_BYTES {
            large.push((index, payload));
        } else {
            payload.validate()?;
        }
    }
    if large.len() <= 1 {
        return large
            .into_iter()
            .try_for_each(|(_, payload)| payload.validate());
    }
    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(MAX_VALIDATION_WORKERS)
        .min(large.len());
    if worker_count <= 1 {
        return large
            .into_iter()
            .try_for_each(|(_, payload)| payload.validate());
    }
    let mut buckets = (0..worker_count).map(|_| Vec::new()).collect::<Vec<_>>();
    for (position, payload) in large.into_iter().enumerate() {
        buckets[position % worker_count].push(payload);
    }
    let mut failures = std::thread::scope(|scope| {
        let handles = buckets
            .into_iter()
            .map(|bucket| {
                scope.spawn(move || {
                    bucket.into_iter().find_map(|(index, payload)| {
                        payload.validate().err().map(|error| (index, error))
                    })
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().expect("resource validation worker panicked"))
            .collect::<Vec<_>>()
    });
    failures.sort_by_key(|(index, _)| *index);
    if let Some((_, error)) = failures.into_iter().next() {
        return Err(error);
    }
    Ok(())
}

fn validate_glyph_metadata(glyph: &GlyphBitmap) -> Result<(), PlatformError> {
    let channels = match glyph.format {
        GlyphBitmapFormat::Alpha8 => 1usize,
        GlyphBitmapFormat::Rgba8 => 4usize,
    };
    let expected = (glyph.width as usize)
        .checked_mul(glyph.height as usize)
        .and_then(|pixels| pixels.checked_mul(channels));
    if glyph.width == 0 || glyph.height == 0 || expected != Some(glyph.pixels.len()) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "glyph dimensions or format is invalid",
        ));
    }
    Ok(())
}

fn validate_glyph(glyph: &GlyphBitmap) -> Result<(), PlatformError> {
    validate_glyph_metadata(glyph)?;
    if astra_core::Hash256::from_sha256(&glyph.pixels) != glyph.hash {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "glyph dimensions, format, or content hash is invalid",
        ));
    }
    Ok(())
}

fn validate_texture_metadata(texture: &TextureFrame) -> Result<(), PlatformError> {
    let expected = (texture.width as usize)
        .checked_mul(texture.height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if texture.width == 0 || texture.height == 0 || expected != Some(texture.rgba8.len()) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.present_scene",
            "texture dimensions are invalid",
        ));
    }
    Ok(())
}

fn validate_texture(texture: &TextureFrame) -> Result<(), PlatformError> {
    validate_texture_metadata(texture)?;
    if astra_core::Hash256::from_sha256(&texture.rgba8) != texture.hash {
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

fn pack_atlas(resources: &AtlasResourceView<'_>) -> Result<PackedAtlas, PlatformError> {
    pack_atlas_with_min_side(resources, MIN_ATLAS_SIDE)
}

fn pack_atlas_with_min_side(
    resources: &AtlasResourceView<'_>,
    minimum_side: u32,
) -> Result<PackedAtlas, PlatformError> {
    let widest = resources
        .values()
        .map(|resource| resource.width() + ATLAS_PADDING * 2)
        .max()
        .unwrap_or(64);
    let total_area = resources.values().try_fold(0_u64, |total, resource| {
        let width = u64::from(resource.width() + ATLAS_PADDING * 2);
        let height = u64::from(resource.height() + ATLAS_PADDING * 2);
        total
            .checked_add(
                width
                    .checked_mul(height)
                    .ok_or_else(|| invalid("glyph atlas resource area overflowed"))?,
            )
            .ok_or_else(|| invalid("glyph atlas total area overflowed"))
    })?;
    if widest > ATLAS_SIDE || total_area > u64::from(ATLAS_SIDE) * u64::from(ATLAS_SIDE) {
        return Err(invalid("glyph is larger than the configured atlas"));
    }
    let mut ordered = resources.iter().collect::<Vec<_>>();
    ordered.sort_by(|(left_id, left), (right_id, right)| {
        right
            .height()
            .cmp(&left.height())
            .then_with(|| right.width().cmp(&left.width()))
            .then_with(|| left_id.cmp(right_id))
    });
    for side in [MIN_ATLAS_SIDE, 2048, ATLAS_SIDE]
        .into_iter()
        .filter(|side| *side >= minimum_side)
    {
        if let Some(packed) = pack_atlas_for_side(&ordered, side)? {
            return Ok(packed);
        }
    }
    Err(invalid("glyph atlas capacity was exceeded"))
}

fn pack_atlas_at_side(
    resources: &AtlasResourceView<'_>,
    side: u32,
) -> Result<Option<PackedAtlas>, PlatformError> {
    if !(MIN_ATLAS_SIDE..=ATLAS_SIDE).contains(&side) || !side.is_power_of_two() {
        return Err(invalid("glyph atlas side is outside the supported range"));
    }
    let mut ordered = resources.iter().collect::<Vec<_>>();
    ordered.sort_by(|(left_id, left), (right_id, right)| {
        right
            .height()
            .cmp(&left.height())
            .then_with(|| right.width().cmp(&left.width()))
            .then_with(|| left_id.cmp(right_id))
    });
    pack_atlas_for_side(&ordered, side)
}

fn pack_atlas_for_side(
    ordered: &[(&String, &AtlasResource)],
    side: u32,
) -> Result<Option<PackedAtlas>, PlatformError> {
    let mut placements = BTreeMap::new();
    let mut x = ATLAS_PADDING * 3;
    let mut y = ATLAS_PADDING;
    let mut row_height = 0;
    for &(resource_id, resource) in ordered {
        if resource.width() + ATLAS_PADDING * 2 > side
            || resource.height() + ATLAS_PADDING * 2 > side
        {
            return Ok(None);
        }
        if x + resource.width() + ATLAS_PADDING > side {
            x = ATLAS_PADDING * 3;
            y = y
                .checked_add(row_height + ATLAS_PADDING)
                .ok_or_else(|| invalid("glyph atlas row overflowed"))?;
            row_height = 0;
        }
        if y + resource.height() + ATLAS_PADDING > side {
            return Ok(None);
        }
        placements.insert(
            (*resource_id).clone(),
            AtlasPlacement {
                x,
                y,
                width: resource.width(),
                height: resource.height(),
            },
        );
        x += resource.width() + ATLAS_PADDING;
        row_height = row_height.max(resource.height());
    }
    Ok(Some(PackedAtlas {
        placements,
        width: side,
        height: side,
        cursor_x: x,
        cursor_y: y,
        row_height,
        free_slots: Vec::new(),
        freed_area: 0,
    }))
}

fn update_gpu_atlas(
    mut context: AtlasUpdateContext<'_>,
    current: Option<&GpuAtlas>,
    old_resources: &BTreeMap<String, AtlasResource>,
    new_resources: &AtlasResourceView<'_>,
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
        .filter(|(id, resource)| old_resources.get(*id) != Some(*resource))
        .collect::<Vec<_>>();
    for (resource_id, resource) in &uploads {
        if current.packed.placements.contains_key(*resource_id) {
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

fn repack_gpu_atlas(
    context: &mut AtlasUpdateContext<'_>,
    current: &GpuAtlas,
    old_resources: &BTreeMap<String, AtlasResource>,
    new_resources: &AtlasResourceView<'_>,
    upload_pixels: &mut Vec<u8>,
) -> Result<(PreparedAtlasUpdate, u64), PlatformError> {
    let mut side = current.packed.width;
    let packed = loop {
        if let Some(packed) = pack_atlas_at_side(new_resources, side)? {
            break packed;
        }
        if side == ATLAS_SIDE {
            return Err(invalid("glyph atlas capacity was exceeded"));
        }
        side = side.saturating_mul(2).min(ATLAS_SIDE);
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

fn create_empty_gpu_atlas(context: &AtlasUpdateContext<'_>, packed: PackedAtlas) -> GpuAtlas {
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
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = context
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-glyph-atlas-bind-group"),
            layout: context.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(context.sampler),
                },
            ],
        });
    GpuAtlas {
        texture,
        bind_group,
        packed,
    }
}

fn create_reserved_gpu_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
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
        sampler,
        staging_belt: &mut staging_belt,
        timestamp_query: None,
    };
    let atlas = create_empty_gpu_atlas(&context, packed);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &atlas.texture,
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
    atlas
}

fn create_full_gpu_atlas(
    context: &mut AtlasUpdateContext<'_>,
    resources: &AtlasResourceView<'_>,
    upload_pixels: &mut Vec<u8>,
) -> Result<GpuAtlas, PlatformError> {
    create_full_gpu_atlas_with_min_side(context, resources, upload_pixels, MIN_ATLAS_SIDE)
}

fn create_full_gpu_atlas_with_min_side(
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = context
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-glyph-atlas-bind-group"),
            layout: context.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(context.sampler),
                },
            ],
        });
    Ok(GpuAtlas {
        texture,
        bind_group,
        packed,
    })
}

fn release_pending_atlas_slot(
    allocator: &mut AtlasAllocatorState<'_>,
    placement: AtlasPlacement,
) -> Result<(), PlatformError> {
    let slot = placement_to_slot(placement)?;
    allocator.freed_area = allocator
        .freed_area
        .checked_add(atlas_slot_area(slot))
        .ok_or_else(|| invalid("glyph atlas freed area overflowed"))?;
    insert_pending_free_slot(allocator, slot);
    Ok(())
}

fn insert_pending_free_slot(allocator: &mut AtlasAllocatorState<'_>, mut slot: AtlasSlot) {
    loop {
        if let Some((index, merged)) = allocator
            .base_free_slots
            .iter()
            .enumerate()
            .filter(|(index, _)| !allocator.consumed_free_slots.contains(index))
            .find_map(|(index, candidate)| {
                merge_adjacent_atlas_slots(slot, *candidate).map(|merged| (index, merged))
            })
        {
            allocator.consumed_free_slots.insert(index);
            slot = merged;
            continue;
        }
        if let Some((index, merged)) =
            allocator
                .added_free_slots
                .iter()
                .enumerate()
                .find_map(|(index, candidate)| {
                    merge_adjacent_atlas_slots(slot, *candidate).map(|merged| (index, merged))
                })
        {
            allocator.added_free_slots.remove(index);
            slot = merged;
            continue;
        }
        break;
    }
    allocator.added_free_slots.push(slot);
    allocator.added_free_slots.sort_by_key(atlas_slot_sort_key);
}

fn merge_adjacent_atlas_slots(left: AtlasSlot, right: AtlasSlot) -> Option<AtlasSlot> {
    if left.y == right.y && left.height == right.height {
        if left.x.checked_add(left.width) == Some(right.x) {
            return Some(AtlasSlot {
                x: left.x,
                y: left.y,
                width: left.width.checked_add(right.width)?,
                height: left.height,
            });
        }
        if right.x.checked_add(right.width) == Some(left.x) {
            return Some(AtlasSlot {
                x: right.x,
                y: right.y,
                width: right.width.checked_add(left.width)?,
                height: right.height,
            });
        }
    }
    if left.x == right.x && left.width == right.width {
        if left.y.checked_add(left.height) == Some(right.y) {
            return Some(AtlasSlot {
                x: left.x,
                y: left.y,
                width: left.width,
                height: left.height.checked_add(right.height)?,
            });
        }
        if right.y.checked_add(right.height) == Some(left.y) {
            return Some(AtlasSlot {
                x: right.x,
                y: right.y,
                width: right.width,
                height: right.height.checked_add(left.height)?,
            });
        }
    }
    None
}

fn allocate_pending_atlas_slot(
    allocator: &mut AtlasAllocatorState<'_>,
    width: u32,
    height: u32,
) -> Result<AtlasPlacement, PlatformError> {
    let required_width = width + ATLAS_PADDING * 2;
    let required_height = height + ATLAS_PADDING * 2;
    let base = allocator
        .base_free_slots
        .iter()
        .enumerate()
        .filter(|(index, slot)| {
            !allocator.consumed_free_slots.contains(index)
                && slot.width >= required_width
                && slot.height >= required_height
        })
        .min_by_key(|(_, slot)| atlas_slot_sort_key(slot))
        .map(|(index, slot)| (index, *slot));
    let added = allocator
        .added_free_slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.width >= required_width && slot.height >= required_height)
        .min_by_key(|(_, slot)| atlas_slot_sort_key(slot))
        .map(|(index, slot)| (index, *slot));
    let selected = match (base, added) {
        (Some((index, slot)), Some((added_index, added_slot))) => {
            if atlas_slot_sort_key(&slot) <= atlas_slot_sort_key(&added_slot) {
                allocator.consumed_free_slots.insert(index);
                slot
            } else {
                allocator.added_free_slots.remove(added_index)
            }
        }
        (Some((index, slot)), None) => {
            allocator.consumed_free_slots.insert(index);
            slot
        }
        (None, Some((index, _))) => allocator.added_free_slots.remove(index),
        (None, None) => {
            if allocator.cursor_x + required_width > allocator.width {
                allocator.cursor_x = ATLAS_PADDING * 3;
                allocator.cursor_y = allocator
                    .cursor_y
                    .checked_add(allocator.row_height + ATLAS_PADDING)
                    .ok_or_else(|| invalid("glyph atlas row overflowed"))?;
                allocator.row_height = 0;
            }
            if allocator.cursor_y + required_height > allocator.height {
                return Err(invalid("glyph atlas capacity was exceeded"));
            }
            let placement = AtlasPlacement {
                x: allocator.cursor_x + ATLAS_PADDING,
                y: allocator.cursor_y + ATLAS_PADDING,
                width,
                height,
            };
            allocator.cursor_x += required_width;
            allocator.row_height = allocator.row_height.max(required_height);
            return Ok(placement);
        }
    };
    let used_area = u64::from(required_width) * u64::from(required_height);
    allocator.freed_area = allocator
        .freed_area
        .checked_sub(used_area)
        .ok_or_else(|| invalid("glyph atlas free-slot area accounting underflowed"))?;
    if selected.width > required_width {
        insert_pending_free_slot(
            allocator,
            AtlasSlot {
                x: selected.x + required_width,
                y: selected.y,
                width: selected.width - required_width,
                height: required_height,
            },
        );
    }
    if selected.height > required_height {
        insert_pending_free_slot(
            allocator,
            AtlasSlot {
                x: selected.x,
                y: selected.y + required_height,
                width: selected.width,
                height: selected.height - required_height,
            },
        );
    }
    Ok(AtlasPlacement {
        x: selected.x + ATLAS_PADDING,
        y: selected.y + ATLAS_PADDING,
        width,
        height,
    })
}

fn placement_to_slot(placement: AtlasPlacement) -> Result<AtlasSlot, PlatformError> {
    Ok(AtlasSlot {
        x: placement
            .x
            .checked_sub(ATLAS_PADDING)
            .ok_or_else(|| invalid("glyph atlas placement padding underflowed"))?,
        y: placement
            .y
            .checked_sub(ATLAS_PADDING)
            .ok_or_else(|| invalid("glyph atlas placement padding underflowed"))?,
        width: placement.width + ATLAS_PADDING * 2,
        height: placement.height + ATLAS_PADDING * 2,
    })
}

fn atlas_slot_area(slot: AtlasSlot) -> u64 {
    u64::from(slot.width) * u64::from(slot.height)
}

fn atlas_slot_sort_key(slot: &AtlasSlot) -> (u64, u32, u32) {
    (atlas_slot_area(*slot), slot.y, slot.x)
}

fn upload_resource_rect(
    queue: &wgpu::Queue,
    encoder: Option<&mut wgpu::CommandEncoder>,
    texture: &wgpu::Texture,
    resource: &AtlasResource,
    placement: AtlasPlacement,
    upload_pixels: &mut Vec<u8>,
    staging_belt: &mut wgpu::util::StagingBelt,
) -> Result<(), PlatformError> {
    let width = resource.width() + ATLAS_PADDING * 2;
    let height = resource.height() + ATLAS_PADDING * 2;
    prepare_upload_pixels(upload_pixels, width, height)?;
    write_padded_resource(resource, upload_pixels, width, 0, 0)?;
    let origin = wgpu::Origin3d {
        x: placement.x - ATLAS_PADDING,
        y: placement.y - ATLAS_PADDING,
        z: 0,
    };
    if let Some(encoder) = encoder {
        encode_texture_upload_belt(
            encoder,
            texture,
            upload_pixels,
            width,
            height,
            origin,
            staging_belt,
        )?;
    } else {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            upload_pixels,
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
    }
    Ok(())
}

fn build_atlas_pixels(
    resources: &AtlasResourceView<'_>,
    atlas: &PackedAtlas,
    upload_pixels: &mut Vec<u8>,
) -> Result<(), PlatformError> {
    prepare_upload_pixels(upload_pixels, atlas.width, atlas.height)?;
    upload_pixels.fill(0);
    upload_pixels[..4].copy_from_slice(&[255; 4]);
    for (resource_id, resource) in resources.iter() {
        let placement = atlas
            .placements
            .get(resource_id)
            .ok_or_else(|| invalid("glyph atlas placement is missing"))?;
        write_padded_resource(
            resource,
            upload_pixels,
            atlas.width,
            placement.x - ATLAS_PADDING,
            placement.y - ATLAS_PADDING,
        )?;
    }
    Ok(())
}

fn write_padded_resource(
    resource: &AtlasResource,
    destination: &mut [u8],
    destination_width: u32,
    destination_x: u32,
    destination_y: u32,
) -> Result<(), PlatformError> {
    let source_width = resource.width() as usize;
    let source_height = resource.height() as usize;
    let destination_width = destination_width as usize;
    let destination_x = destination_x as usize;
    let destination_y = destination_y as usize;
    let padded_width = source_width + ATLAS_PADDING as usize * 2;
    let padded_height = source_height + ATLAS_PADDING as usize * 2;
    let required_rows = destination_y
        .checked_add(padded_height)
        .ok_or_else(|| invalid("glyph atlas padded row overflowed"))?;
    let required_columns = destination_x
        .checked_add(padded_width)
        .ok_or_else(|| invalid("glyph atlas padded column overflowed"))?;
    let required_bytes = required_rows
        .checked_mul(destination_width)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("glyph atlas padded destination overflowed"))?;
    if source_width == 0
        || source_height == 0
        || required_columns > destination_width
        || required_bytes > destination.len()
    {
        return Err(invalid(
            "glyph atlas padded resource is outside destination",
        ));
    }
    match resource {
        AtlasResource::Glyph(glyph) if glyph.format == GlyphBitmapFormat::Alpha8 => {
            let padding = ATLAS_PADDING as usize;
            for padded_row in 0..padded_height {
                let source_row = padded_row.saturating_sub(padding).min(source_height - 1);
                for padded_column in 0..padded_width {
                    let source_column = padded_column.saturating_sub(padding).min(source_width - 1);
                    let alpha = glyph.pixels[source_row * source_width + source_column];
                    let offset = ((destination_y + padded_row) * destination_width
                        + destination_x
                        + padded_column)
                        * 4;
                    destination[offset..offset + 4].copy_from_slice(&[alpha; 4]);
                }
            }
        }
        AtlasResource::Glyph(glyph) => write_padded_rgba_rows(
            &glyph.pixels,
            source_width,
            source_height,
            destination,
            destination_width,
            destination_x,
            destination_y,
        )?,
        AtlasResource::Texture(texture) => write_padded_rgba_rows(
            &texture.rgba8,
            source_width,
            source_height,
            destination,
            destination_width,
            destination_x,
            destination_y,
        )?,
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_padded_rgba_rows(
    source: &[u8],
    source_width: usize,
    source_height: usize,
    destination: &mut [u8],
    destination_width: usize,
    destination_x: usize,
    destination_y: usize,
) -> Result<(), PlatformError> {
    let padding = ATLAS_PADDING as usize;
    let source_row_bytes = source_width
        .checked_mul(4)
        .ok_or_else(|| invalid("glyph atlas source row overflowed"))?;
    if source.len() != source_row_bytes.saturating_mul(source_height) {
        return Err(invalid("glyph atlas RGBA resource byte count is invalid"));
    }
    for padded_row in 0..source_height + padding * 2 {
        let source_row = padded_row.saturating_sub(padding).min(source_height - 1);
        let source_start = source_row * source_row_bytes;
        let destination_start =
            ((destination_y + padded_row) * destination_width + destination_x) * 4;
        let interior_start = destination_start + padding * 4;
        destination[interior_start..interior_start + source_row_bytes]
            .copy_from_slice(&source[source_start..source_start + source_row_bytes]);
        for column in 0..padding {
            let left = destination_start + column * 4;
            destination[left..left + 4].copy_from_slice(&source[source_start..source_start + 4]);
        }
        let right_source = source_start + source_row_bytes - 4;
        let right_destination = interior_start + source_row_bytes;
        for column in 0..padding {
            let right = right_destination + column * 4;
            destination[right..right + 4].copy_from_slice(&source[right_source..right_source + 4]);
        }
    }
    Ok(())
}

fn prepare_upload_pixels(
    upload_pixels: &mut Vec<u8>,
    width: u32,
    height: u32,
) -> Result<(), PlatformError> {
    let required = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("glyph atlas upload byte count overflowed"))?;
    if required > MAX_ATLAS_UPLOAD_BYTES {
        return Err(invalid(
            "glyph atlas upload exceeds the reserved staging budget",
        ));
    }
    if upload_pixels.capacity() < required {
        upload_pixels
            .try_reserve_exact(required - upload_pixels.capacity())
            .map_err(|_| invalid("glyph atlas upload staging allocation failed"))?;
    }
    upload_pixels.clear();
    upload_pixels.resize(required, 0);
    Ok(())
}

fn upload_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pixels: &[u8],
    width: u32,
    height: u32,
    timestamp_query: Option<(&wgpu::QuerySet, u32, u32)>,
) -> Result<wgpu::Texture, PlatformError> {
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    if let Some((query_set, begin, end)) = timestamp_query {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra-glyph-atlas-upload-encoder"),
        });
        encoder.write_timestamp(query_set, begin);
        encode_texture_upload_once(
            device,
            &mut encoder,
            &texture,
            pixels,
            width,
            height,
            wgpu::Origin3d::ZERO,
        )?;
        encoder.write_timestamp(query_set, end);
        queue.submit([encoder.finish()]);
    } else {
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
    }
    Ok(texture)
}

fn primed_staging_belt(device: &wgpu::Device) -> wgpu::util::StagingBelt {
    let mut belt = wgpu::util::StagingBelt::new(device.clone(), ATLAS_STAGING_CHUNK_BYTES);
    {
        let _ = belt.allocate(
            wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT)
                .expect("copy buffer alignment is non-zero"),
            wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT)
                .expect("copy buffer alignment is non-zero"),
        );
    }
    belt
}

fn encode_texture_upload_once(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    pixels: &[u8],
    width: u32,
    height: u32,
    origin: wgpu::Origin3d,
) -> Result<(), PlatformError> {
    let tight_row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("glyph atlas upload row overflowed"))?;
    let padded_row =
        tight_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let size = u64::from(padded_row)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("glyph atlas staging buffer size overflowed"))?;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("astra-glyph-atlas-initial-upload-staging"),
        size,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    {
        let mut mapped = staging.slice(..).get_mapped_range_mut();
        for row in 0..height as usize {
            let source = row * tight_row as usize;
            let destination = row * padded_row as usize;
            mapped
                .slice(destination..destination + tight_row as usize)
                .copy_from_slice(&pixels[source..source + tight_row as usize]);
        }
    }
    staging.unmap();
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

fn encode_texture_upload_belt(
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    pixels: &[u8],
    width: u32,
    height: u32,
    origin: wgpu::Origin3d,
    staging_belt: &mut wgpu::util::StagingBelt,
) -> Result<(), PlatformError> {
    let tight_row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("glyph atlas upload row overflowed"))?;
    let padded_row =
        tight_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let size = u64::from(padded_row)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("glyph atlas staging buffer size overflowed"))?;
    let size = wgpu::BufferSize::new(size)
        .ok_or_else(|| invalid("glyph atlas staging buffer cannot be empty"))?;
    let alignment = wgpu::BufferSize::new(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
        .expect("copy row alignment is non-zero");
    let staging = staging_belt.allocate(size, alignment);
    {
        let mut mapped = staging.get_mapped_range_mut();
        for row in 0..height as usize {
            let source = row * tight_row as usize;
            let destination = row * padded_row as usize;
            mapped
                .slice(destination..destination + tight_row as usize)
                .copy_from_slice(&pixels[source..source + tight_row as usize]);
        }
    }
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: staging.buffer(),
            layout: wgpu::TexelCopyBufferLayout {
                offset: staging.offset(),
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

fn build_vertices(
    frame: &SceneFrame,
    quad_runs: &[DrawRun],
    runs: &[DrawPrimitive],
    atlas: &AtlasPlacementView<'_>,
    resources: &AtlasResourceView<'_>,
    bytes: &mut Vec<u8>,
    batches: &mut Vec<DrawBatch>,
) -> Result<(), PlatformError> {
    let mut vertex_count = 0u32;
    for primitive in runs {
        let clip = match primitive {
            DrawPrimitive::Quads(index) => quad_runs[*index].clip,
            DrawPrimitive::Mesh(run) => run.clip,
        };
        if clip.width == 0 || clip.height == 0 {
            continue;
        }
        let first_vertex = vertex_count;
        match primitive {
            DrawPrimitive::Quads(index) => {
                let run = &quad_runs[*index];
                let color = straight_to_premultiplied_linear(run.rgba, run.opacity);
                for quad in &run.quads {
                    let left = quad.destination.x as f32;
                    let top = quad.destination.y as f32;
                    let rotated = quad.rotation_quadrants % 4;
                    let (draw_width, draw_height) = if rotated % 2 == 0 {
                        (quad.destination.width, quad.destination.height)
                    } else {
                        (quad.destination.height, quad.destination.width)
                    };
                    let right = left + draw_width as f32;
                    let bottom = top + draw_height as f32;
                    let (u0, v0, u1, v1) = match &quad.source {
                        QuadSource::Resource {
                            resource_id,
                            source,
                        } => {
                            let placement = atlas
                                .get(resource_id.as_ref())
                                .ok_or_else(|| invalid("draw resource has no atlas placement"))?;
                            let source_right = source.x as u32 + source.width;
                            let source_bottom = source.y as u32 + source.height;
                            (
                                (placement.x + source.x as u32) as f32 / atlas.width() as f32,
                                (placement.y + source.y as u32) as f32 / atlas.height() as f32,
                                (placement.x + source_right) as f32 / atlas.width() as f32,
                                (placement.y + source_bottom) as f32 / atlas.height() as f32,
                            )
                        }
                        QuadSource::White => {
                            let u = 0.5 / atlas.width() as f32;
                            let v = 0.5 / atlas.height() as f32;
                            (u, v, u, v)
                        }
                    };
                    let uv = match rotated {
                        0 => [(u0, v0), (u1, v0), (u1, v1), (u0, v1)],
                        1 => [(u0, v1), (u0, v0), (u1, v0), (u1, v1)],
                        2 => [(u1, v1), (u0, v1), (u0, v0), (u1, v0)],
                        3 => [(u1, v0), (u1, v1), (u0, v1), (u0, v0)],
                        _ => unreachable!(),
                    };
                    let positions = [(left, top), (right, top), (right, bottom), (left, bottom)];
                    for corner in [0, 1, 2, 0, 2, 3] {
                        let (x, y) = transform_point(
                            run.transform,
                            positions[corner].0,
                            positions[corner].1,
                        );
                        let (u, v) = uv[corner];
                        push_vertex(bytes, x, y, u, v, color, frame.width, frame.height);
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
                            .get(resource_id)
                            .ok_or_else(|| invalid("mesh texture has no atlas placement"))?;
                        Ok::<_, PlatformError>((placement, resource.width(), resource.height()))
                    })
                    .transpose()?;
                for index in run.indices {
                    let vertex = &run.vertices[*index as usize];
                    let (x, y) =
                        transform_point(run.transform, vertex.position[0], vertex.position[1]);
                    let (u, v) = if let Some((placement, width, height)) = texture_mapping {
                        (
                            (placement.x as f32 + vertex.uv[0] * width as f32)
                                / atlas.width() as f32,
                            (placement.y as f32 + vertex.uv[1] * height as f32)
                                / atlas.height() as f32,
                        )
                    } else {
                        (0.5 / atlas.width() as f32, 0.5 / atlas.height() as f32)
                    };
                    push_vertex(
                        bytes,
                        x,
                        y,
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
    Ok(())
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

fn validate_draw_identity<'a>(
    id: &'a str,
    opacity: &f32,
    blend: BlendMode,
    run_ids: &mut SmallVec<[&'a str; 128]>,
) -> Result<(), PlatformError> {
    if id.is_empty()
        || id.len() > 256
        || !insert_unique(run_ids, id)
        || !opacity.is_finite()
        || !(0.0..=1.0).contains(opacity)
        || blend != BlendMode::Alpha
    {
        return Err(invalid("draw identity, opacity, or blend mode is invalid"));
    }
    Ok(())
}

fn insert_unique<'a>(values: &mut SmallVec<[&'a str; 128]>, value: &'a str) -> bool {
    if values.contains(&value) {
        false
    } else {
        values.push(value);
        true
    }
}

fn transient_resource_id(
    resources: &AtlasResourceView<'_>,
    sequence: &mut u64,
    kind: &str,
) -> Result<String, PlatformError> {
    loop {
        *sequence = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("transient scene resource sequence overflowed"))?;
        let id = format!("astra.transient.{kind}.{sequence}");
        if !resources.contains_key(&id) {
            return Ok(id);
        }
    }
}

fn validate_opacity(opacity: f32) -> Result<(), PlatformError> {
    if opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(invalid("scene opacity is invalid"))
    }
}

fn validate_transform(transform: Transform2D) -> Result<(), PlatformError> {
    let values = [
        transform.m11,
        transform.m12,
        transform.m21,
        transform.m22,
        transform.tx,
        transform.ty,
    ];
    let determinant = transform.m11 * transform.m22 - transform.m12 * transform.m21;
    if values.into_iter().all(f32::is_finite)
        && determinant.is_finite()
        && determinant.abs() >= f32::EPSILON
    {
        Ok(())
    } else {
        Err(invalid("scene transform is invalid or singular"))
    }
}

fn compose_transform(current: Transform2D, next: Transform2D) -> Transform2D {
    Transform2D {
        m11: next.m11 * current.m11 + next.m21 * current.m12,
        m12: next.m12 * current.m11 + next.m22 * current.m12,
        m21: next.m11 * current.m21 + next.m21 * current.m22,
        m22: next.m12 * current.m21 + next.m22 * current.m22,
        tx: next.m11 * current.tx + next.m21 * current.ty + next.tx,
        ty: next.m12 * current.tx + next.m22 * current.ty + next.ty,
    }
}

fn current_transform(camera: Transform2D, stack: &[Transform2D]) -> Transform2D {
    compose_transform(
        stack.last().copied().unwrap_or(Transform2D::IDENTITY),
        camera,
    )
}

fn transform_point(transform: Transform2D, x: f32, y: f32) -> (f32, f32) {
    (
        transform.m11 * x + transform.m21 * y + transform.tx,
        transform.m12 * x + transform.m22 * y + transform.ty,
    )
}

fn transformed_bounds(transform: Transform2D, rect: RectI) -> Result<RectI, PlatformError> {
    let right = rect.x as f32 + rect.width as f32;
    let bottom = rect.y as f32 + rect.height as f32;
    let points = [
        transform_point(transform, rect.x as f32, rect.y as f32),
        transform_point(transform, right, rect.y as f32),
        transform_point(transform, right, bottom),
        transform_point(transform, rect.x as f32, bottom),
    ];
    let min_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min);
    let min_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = points
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max);
    if ![min_x, min_y, max_x, max_y].into_iter().all(f32::is_finite)
        || min_x < i32::MIN as f32
        || min_y < i32::MIN as f32
        || max_x > i32::MAX as f32
        || max_y > i32::MAX as f32
    {
        return Err(invalid(
            "transformed scene clip is outside supported coordinates",
        ));
    }
    let left = min_x.floor() as i32;
    let top = min_y.floor() as i32;
    let right = max_x.ceil() as i64;
    let bottom = max_y.ceil() as i64;
    let width = u32::try_from(right - i64::from(left))
        .map_err(|_| invalid("transformed scene clip width overflowed"))?;
    let height = u32::try_from(bottom - i64::from(top))
        .map_err(|_| invalid("transformed scene clip height overflowed"))?;
    Ok(RectI::new(left, top, width, height))
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
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
            }],
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
    let alpha = sample.a * input.color.a;
    return vec4<f32>(sample.rgb * input.color.rgb * sample.a, alpha);
}
"#;

#[cfg(test)]
mod tests {
    use super::{
        allocate_pending_atlas_slot, insert_unique, pack_atlas, prepare_upload_pixels,
        release_pending_atlas_slot, vertex_upload_required, write_padded_resource,
        AtlasAllocatorState, AtlasResource, AtlasResourceView, ResourceMutationJournal,
        ATLAS_PADDING, ATLAS_SIDE, MAX_ATLAS_UPLOAD_BYTES,
    };
    use astra_core::Hash256;
    use astra_media_core::TextureFrame;
    use astra_platform::PlatformErrorCode;
    use smallvec::SmallVec;
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn classic_scene_id_journal_stays_within_inline_capacity() {
        let mut ids: SmallVec<[&str; 128]> = SmallVec::new();
        for id in ["a"; 128] {
            // Distinct suffixes are unnecessary for this capacity invariant;
            // insert directly so the test only exercises the inline journal.
            ids.push(id);
        }
        assert_eq!(ids.len(), 128);
        assert!(!ids.spilled());

        let mut unique_ids: SmallVec<[&str; 128]> = SmallVec::new();
        assert!(insert_unique(&mut unique_ids, "classic.background"));
        assert!(!insert_unique(&mut unique_ids, "classic.background"));
    }

    #[test]
    fn atlas_width_tracks_total_area_and_packs_multiple_stage_textures() {
        let rgba8 = vec![0x7f; 800 * 600 * 4];
        let texture = TextureFrame {
            width: 800,
            height: 600,
            hash: Hash256::from_sha256(&rgba8),
            rgba8: rgba8.into(),
        };
        let resources = (0..8)
            .map(|index| {
                (
                    format!("texture.stage.{index}"),
                    AtlasResource::Texture(Arc::new(texture.clone())),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let mutations = ResourceMutationJournal::new();
        let packed = pack_atlas(&AtlasResourceView::new(&resources, &mutations))
            .expect("stage textures must fit the bounded atlas");

        assert_eq!(packed.placements.len(), 8);
        assert_eq!(packed.width, 4096);
        assert_eq!(packed.height, 4096);
    }

    #[test]
    fn resource_view_applies_mutations_without_cloning_the_base_map() {
        let texture = |value| {
            let rgba8 = vec![value; 4];
            AtlasResource::Texture(Arc::new(TextureFrame {
                width: 1,
                height: 1,
                hash: Hash256::from_sha256(&rgba8),
                rgba8: rgba8.into(),
            }))
        };
        let base = [
            ("keep".to_string(), texture(1)),
            ("release".to_string(), texture(2)),
            ("replace".to_string(), texture(3)),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        let mutations = [
            ("add".to_string(), Some(texture(4))),
            ("release".to_string(), None),
            ("replace".to_string(), Some(texture(5))),
        ]
        .into_iter()
        .collect::<ResourceMutationJournal>();
        let view = AtlasResourceView::new(&base, &mutations);

        assert_eq!(view.len(), 3);
        assert!(view.contains_key("keep"));
        assert!(view.contains_key("add"));
        assert!(!view.contains_key("release"));
        assert!(view.get("replace") == mutations["replace"].as_ref());
        assert_eq!(
            view.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec!["keep", "add", "replace"]
        );
    }

    #[test]
    fn released_atlas_slot_is_reused_without_repacking_live_resources() {
        let rgba8 = vec![0x7f; 64 * 64 * 4];
        let texture = TextureFrame {
            width: 64,
            height: 64,
            hash: Hash256::from_sha256(&rgba8),
            rgba8: rgba8.into(),
        };
        let resources = [(
            "texture.stable".to_string(),
            AtlasResource::Texture(Arc::new(texture)),
        )]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        let mutations = ResourceMutationJournal::new();
        let packed = pack_atlas(&AtlasResourceView::new(&resources, &mutations)).unwrap();
        let original = packed.placements["texture.stable"];
        let mut allocator = AtlasAllocatorState::new(&packed);
        release_pending_atlas_slot(&mut allocator, original).unwrap();
        let reused = allocate_pending_atlas_slot(&mut allocator, 32, 32).unwrap();
        assert_eq!((reused.x, reused.y), (original.x, original.y));
        let original_area = u64::from(original.width + ATLAS_PADDING * 2)
            * u64::from(original.height + ATLAS_PADDING * 2);
        let reused_area = u64::from(32 + ATLAS_PADDING * 2).pow(2);
        assert_eq!(allocator.freed_area, original_area - reused_area);
        assert_eq!(allocator.added_free_slots.len(), 2);
        release_pending_atlas_slot(&mut allocator, reused).unwrap();
        assert_eq!(allocator.freed_area, original_area);
        assert_eq!(allocator.added_free_slots.len(), 1);
        assert_eq!(
            (
                allocator.added_free_slots[0].width,
                allocator.added_free_slots[0].height
            ),
            (
                original.width + ATLAS_PADDING * 2,
                original.height + ATLAS_PADDING * 2
            )
        );
    }

    #[test]
    fn stable_vertex_payload_skips_upload_until_bytes_or_buffer_change() {
        let uploaded = [1_u8, 2, 3, 4];
        assert!(!vertex_upload_required(&uploaded, &uploaded, false));
        assert!(vertex_upload_required(&[1, 2, 3, 5], &uploaded, false));
        assert!(vertex_upload_required(&uploaded, &uploaded, true));
    }

    #[test]
    fn rgba_row_upload_preserves_clamped_padding_pixels() {
        let rgba8 = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let resource = AtlasResource::Texture(Arc::new(TextureFrame {
            width: 2,
            height: 2,
            hash: Hash256::from_sha256(&rgba8),
            rgba8: rgba8.into(),
        }));
        let mut destination = vec![0; 4 * 4 * 4];
        write_padded_resource(&resource, &mut destination, 4, 0, 0).unwrap();
        let pixels = destination
            .chunks_exact(4)
            .map(|pixel| pixel[0])
            .collect::<Vec<_>>();
        assert_eq!(
            pixels,
            vec![1, 1, 5, 5, 1, 1, 5, 5, 9, 9, 13, 13, 9, 9, 13, 13]
        );
    }

    #[test]
    fn reserved_atlas_staging_reuses_storage_across_upload_sizes() {
        let mut pixels = Vec::with_capacity(MAX_ATLAS_UPLOAD_BYTES);
        let storage = pixels.as_ptr();

        prepare_upload_pixels(&mut pixels, 800, 600).unwrap();
        assert_eq!(pixels.len(), 800 * 600 * 4);
        assert_eq!(pixels.as_ptr(), storage);

        prepare_upload_pixels(&mut pixels, 1280, 720).unwrap();
        assert_eq!(pixels.len(), 1280 * 720 * 4);
        assert_eq!(pixels.as_ptr(), storage);

        let error = prepare_upload_pixels(&mut pixels, ATLAS_SIDE + 1, ATLAS_SIDE).unwrap_err();
        assert_eq!(error.code, PlatformErrorCode::InvalidState);
    }
}
