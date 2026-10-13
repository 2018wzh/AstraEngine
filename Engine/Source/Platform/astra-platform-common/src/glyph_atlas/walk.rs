use super::*;

mod control;
mod draw;

pub(super) struct WalkedScene<'a> {
    pub(super) render_mutations: ResourceMutationJournal,
    pub(super) committed_mutations: ResourceMutationJournal,
    pub(super) resources_changed: bool,
    pub(super) quad_runs: SmallVec<[DrawRun<'a>; 64]>,
    pub(super) draw_runs: SmallVec<[DrawPrimitive<'a>; 64]>,
    pub(super) clip_stack: SmallVec<[RectI; 16]>,
    pub(super) pixel_masks: SmallVec<[u64; 16]>,
    pub(super) transform_stack: SmallVec<[Transform2D; 16]>,
    pub(super) camera: Transform2D,
    pub(super) opacity_stack: SmallVec<[f32; 16]>,
    pub(super) transient_sequence: u64,
    pub(super) run_ids: SceneIdSet<'a>,
    pub(super) drawn_resources: SceneIdSet<'a>,
    pub(super) upload_texture_count: u32,
    pub(super) texture_region_uploads: SmallVec<[TextureRegionUpload<'a>; 16]>,
    pub(super) upload_glyph_count: u32,
    pub(super) release_resource_count: u32,
    pub(super) transient_texture_count: u32,
    pub(super) transient_glyph_count: u32,
    pub(super) mesh_command_allocation_bytes: u64,
    pub(super) mesh_command_allocation_count: u64,
    pub(super) default_compositing: SceneCompositing2D,
}
impl<'a> WalkedScene<'a> {
    pub(super) fn new(atlas_missing: bool, default_compositing: SceneCompositing2D) -> Self {
        Self {
            render_mutations: ResourceMutationJournal::new(),
            committed_mutations: ResourceMutationJournal::new(),
            resources_changed: atlas_missing,
            transform_stack: smallvec![Transform2D::IDENTITY],
            camera: Transform2D::IDENTITY,
            opacity_stack: smallvec![1.0_f32],
            run_ids: SceneIdSet::default(),
            drawn_resources: SceneIdSet::default(),
            default_compositing,
            quad_runs: SmallVec::new(),
            draw_runs: SmallVec::new(),
            clip_stack: SmallVec::new(),
            pixel_masks: SmallVec::new(),
            transient_sequence: 0,
            upload_texture_count: 0,
            texture_region_uploads: SmallVec::new(),
            upload_glyph_count: 0,
            release_resource_count: 0,
            transient_texture_count: 0,
            transient_glyph_count: 0,
            mesh_command_allocation_bytes: 0,
            mesh_command_allocation_count: 0,
        }
    }
    pub(super) fn consume(
        &mut self,
        resources: &BTreeMap<String, AtlasResource>,
        frame: &'a SceneFrame,
        apply_mutations: bool,
    ) -> Result<(), PlatformError> {
        for command in &frame.commands {
            if !self.apply_control(command, resources, frame, apply_mutations)? {
                self.apply_draw(command, resources, frame)?;
            }
        }
        if !self.pixel_masks.is_empty()
            || !self.clip_stack.is_empty()
            || self.transform_stack.len() != 1
            || self.opacity_stack.len() != 1
        {
            return Err(invalid("GPU scene command stacks are not balanced"));
        }
        Ok(())
    }
}
