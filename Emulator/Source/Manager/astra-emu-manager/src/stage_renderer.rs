use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
};

use std::borrow::Cow;

use astra_byte_source::OwnedByteBuffer;
use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacyTextureFilter, LegacyTextureFormat, LegacyVertexV1,
    LegacyVideoMode,
};
use astra_emu_family_support::copy_surface_to_straight_rgba8;
use astra_emu_manager::{AstraUnderlayRenderer, TranslationOverlayView, WgpuFrameContext};
use astra_media::{FilterGraph, FilterNode, FilterParam, FilterTarget, FilterValidator};
use astra_plugin_abi::{
    RuntimeLiveBlendMode, RuntimeLiveDraw, RuntimeLiveFilterGraph, RuntimeLiveFilterParam,
    RuntimeLiveFilterTarget, RuntimeLiveLayerBlend, RuntimeLiveLayerFilter,
    RuntimeLiveLayerOperation, RuntimeLiveLayerState, RuntimeLiveLayerTransaction,
    RuntimeLiveSceneCompositing, RuntimeLiveSceneResourceOperation, RuntimeLiveSceneTransaction,
    RuntimeLiveScissor, RuntimeLiveTextureFilter, RuntimeLiveTextureFormat, RuntimeLiveVertex,
};
use wgpu::util::DeviceExt;

use crate::{video_executor::HostVideoFrame, RuntimeBridge};

const STAGE_WIDTH: u32 = 1024;
const STAGE_HEIGHT: u32 = 768;
const VIDEO_TEXTURE_ID: u32 = u32::MAX - 1;

pub(crate) struct ManagerStageRenderer {
    pub(crate) texture: Option<wgpu::Texture>,
    pub(crate) scene_texture: Option<wgpu::Texture>,
    pub(crate) runtime: Rc<RefCell<RuntimeBridge>>,
    pub(crate) gpu: Option<StageGpu>,
    pub(crate) stage_width: u32,
    pub(crate) stage_height: u32,
    pub(crate) texture_dirty: bool,
    pub(crate) scene_initialized: bool,
    pub(crate) scene_compositing: Option<RuntimeLiveSceneCompositing>,
}

pub(crate) struct StageGpu {
    bind_group_layout: wgpu::BindGroupLayout,
    linear_sampler: wgpu::Sampler,
    nearest_sampler: wgpu::Sampler,
    alpha_pipeline: wgpu::RenderPipeline,
    opaque_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    multiply_pipeline: wgpu::RenderPipeline,
    screen_pipeline: wgpu::RenderPipeline,
    encoded_alpha_pipeline: wgpu::RenderPipeline,
    encoded_opaque_pipeline: wgpu::RenderPipeline,
    encoded_add_pipeline: wgpu::RenderPipeline,
    encoded_multiply_pipeline: wgpu::RenderPipeline,
    encoded_screen_pipeline: wgpu::RenderPipeline,
    filter_bind_group_layout: wgpu::BindGroupLayout,
    filter_pipeline: wgpu::RenderPipeline,
    textures: BTreeMap<u32, TextureResource>,
    layer_states: BTreeMap<String, RuntimeLiveLayerState>,
    layer_surface_ids: BTreeMap<String, u32>,
    layer_filtered_ids: BTreeMap<String, u32>,
    layer_filter_graphs: BTreeMap<String, RuntimeLiveFilterGraph>,
    layer_sequence: Option<u64>,
    layer_session_id: Option<String>,
    next_layer_texture_id: u32,
    video_source: Option<Arc<[u8]>>,
}

struct TextureResource {
    _texture: wgpu::Texture,
    linear_bind_group: wgpu::BindGroup,
    nearest_bind_group: wgpu::BindGroup,
    generation: u64,
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    compositing: RuntimeLiveSceneCompositing,
}

impl AstraUnderlayRenderer for ManagerStageRenderer {
    fn setup(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        if self.texture.is_some() || self.gpu.is_some() {
            return Err("ASTRA_EMU_STAGE_RENDERER_DUPLICATE_SETUP".into());
        }
        self.texture = Some(create_stage_texture(context.device));
        self.scene_texture = Some(create_stage_texture(context.device));
        self.stage_width = STAGE_WIDTH;
        self.stage_height = STAGE_HEIGHT;
        self.scene_compositing = None;
        self.gpu = Some(StageGpu::new(context.device));
        Ok(())
    }

    fn stage_texture(&self) -> Option<wgpu::Texture> {
        self.texture.clone()
    }

    fn take_stage_texture_update(&mut self) -> Option<(wgpu::Texture, u32, u32)> {
        if !self.texture_dirty {
            return None;
        }
        self.texture_dirty = false;
        self.texture
            .clone()
            .map(|texture| (texture, self.stage_width, self.stage_height))
    }

    fn translation_overlay(&self) -> Option<TranslationOverlayView> {
        self.runtime
            .try_borrow()
            .ok()
            .and_then(|runtime| runtime.translation_overlay())
            .map(|overlay| TranslationOverlayView {
                source: overlay.source,
                translated: overlay.translated,
                status: overlay.status,
                endpoint: overlay.endpoint,
                model: overlay.model,
                sent_scope: overlay.sent_scope,
            })
    }

    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        let (layer_commit, video, filter_preset) = {
            let mut runtime = self
                .runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?;
            (
                runtime.take_next_live_layer(),
                runtime.current_video_frame(),
                runtime.filter_preset().to_owned(),
            )
        };
        if let Some((session_id, transaction, surfaces)) = layer_commit {
            if transaction.viewport_width != self.stage_width
                || transaction.viewport_height != self.stage_height
            {
                self.texture = Some(create_stage_texture_with_dimensions(
                    context.device,
                    transaction.viewport_width,
                    transaction.viewport_height,
                ));
                self.scene_texture = Some(create_scene_texture_with_dimensions(
                    context.device,
                    transaction.viewport_width,
                    transaction.viewport_height,
                    RuntimeLiveSceneCompositing::LinearSrgb,
                ));
                self.stage_width = transaction.viewport_width;
                self.stage_height = transaction.viewport_height;
                self.texture_dirty = true;
                self.scene_initialized = false;
            }
            let texture = self
                .scene_texture
                .as_ref()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
            self.gpu
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
                .render_layer_transaction(&context, texture, &session_id, &surfaces, transaction)?;
            self.scene_compositing = Some(RuntimeLiveSceneCompositing::LinearSrgb);
            self.scene_initialized = true;
            self.runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .acknowledge_presentation();
        }
        if let Some(video) = video {
            if video.stage_width != self.stage_width || video.stage_height != self.stage_height {
                return Err("ASTRA_EMU_VIDEO_STAGE_DIMENSIONS_MISMATCH".into());
            }
            let texture = self
                .scene_texture
                .as_ref()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
            let compositing = self.scene_compositing.unwrap_or_default();
            self.gpu
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
                .render_video(&context, texture, video, compositing)?;
            self.scene_initialized = true;
        }
        if self.scene_initialized {
            let source = self
                .scene_texture
                .as_ref()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
            let target = self
                .texture
                .as_ref()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
            self.gpu
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
                .apply_final_filter(
                    &context,
                    source,
                    target,
                    &filter_preset,
                    self.scene_compositing.unwrap_or_default(),
                )?;
        }
        Ok(())
    }

    fn teardown(&mut self) {
        self.gpu = None;
        self.texture = None;
        self.scene_texture = None;
        self.scene_initialized = false;
        self.scene_compositing = None;
    }
}

impl StageGpu {
    fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra.emu.stage.texture-layout"),
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
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("astra.emu.stage.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("astra.emu.stage.nearest-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let alpha_pipeline = create_pipeline(device, &bind_group_layout, alpha_blend(), "alpha");
        let opaque_pipeline = create_pipeline(
            device,
            &bind_group_layout,
            wgpu::BlendState::REPLACE,
            "opaque",
        );
        let add_pipeline = create_pipeline(device, &bind_group_layout, add_blend(), "add");
        let multiply_pipeline =
            create_pipeline(device, &bind_group_layout, multiply_blend(), "multiply");
        let screen_pipeline = create_pipeline(device, &bind_group_layout, screen_blend(), "screen");
        let encoded_alpha_pipeline = create_pipeline_for_format(
            device,
            &bind_group_layout,
            alpha_blend(),
            "encoded-alpha",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let encoded_opaque_pipeline = create_pipeline_for_format(
            device,
            &bind_group_layout,
            wgpu::BlendState::REPLACE,
            "encoded-opaque",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let encoded_add_pipeline = create_pipeline_for_format(
            device,
            &bind_group_layout,
            add_blend(),
            "encoded-add",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let encoded_multiply_pipeline = create_pipeline_for_format(
            device,
            &bind_group_layout,
            multiply_blend(),
            "encoded-multiply",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let encoded_screen_pipeline = create_pipeline_for_format(
            device,
            &bind_group_layout,
            screen_blend(),
            "encoded-screen",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let filter_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("astra.emu.filter.texture-layout"),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let filter_pipeline = create_filter_pipeline(device, &filter_bind_group_layout);
        Self {
            bind_group_layout,
            linear_sampler,
            nearest_sampler,
            alpha_pipeline,
            opaque_pipeline,
            add_pipeline,
            multiply_pipeline,
            screen_pipeline,
            encoded_alpha_pipeline,
            encoded_opaque_pipeline,
            encoded_add_pipeline,
            encoded_multiply_pipeline,
            encoded_screen_pipeline,
            filter_bind_group_layout,
            filter_pipeline,
            textures: BTreeMap::new(),
            layer_states: BTreeMap::new(),
            layer_surface_ids: BTreeMap::new(),
            layer_filtered_ids: BTreeMap::new(),
            layer_filter_graphs: BTreeMap::new(),
            layer_sequence: None,
            layer_session_id: None,
            next_layer_texture_id: 1,
            video_source: None,
        }
    }

    fn apply_final_filter(
        &mut self,
        context: &WgpuFrameContext<'_>,
        source: &wgpu::Texture,
        target: &wgpu::Texture,
        preset_id: &str,
        compositing: RuntimeLiveSceneCompositing,
    ) -> Result<(), String> {
        let mode = match preset_id {
            "none" => 0_u32,
            "grayscale" => 1,
            "crt-soft" => 2,
            "warm" => 3,
            _ => return Err("ASTRA_EMU_FILTER_PRESET_UNSUPPORTED".into()),
        };
        let params = filter_uniform_bytes(mode, source.width(), source.height(), [0.0; 4]);
        let uniform = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("astra.emu.filter.params"),
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let source_view = source.create_view(&wgpu::TextureViewDescriptor {
            format: (compositing == RuntimeLiveSceneCompositing::EncodedSrgb)
                .then_some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ..Default::default()
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("astra.emu.filter.bind-group"),
                layout: &self.filter_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.filter.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.filter.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.filter_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        context.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Applies a prepared incremental packet without rebuilding the retained
    /// texture set.  The backend independently re-prepares the packet before
    /// any queue write, so a forged or stale `PreparedCommit` cannot mutate GPU
    /// resources.
    fn render_layer_transaction(
        &mut self,
        context: &WgpuFrameContext<'_>,
        target: &wgpu::Texture,
        session_id: &str,
        surfaces: &astra_emu_family_support::LegacySurfaceStoreV9,
        transaction: RuntimeLiveLayerTransaction,
    ) -> Result<(), String> {
        if self.layer_session_id.as_deref() != Some(session_id) {
            for texture_id in self.layer_surface_ids.values() {
                self.textures.remove(texture_id);
            }
            for texture_id in self.layer_filtered_ids.values() {
                self.textures.remove(texture_id);
            }
            self.layer_states.clear();
            self.layer_surface_ids.clear();
            self.layer_filtered_ids.clear();
            self.layer_filter_graphs.clear();
            self.layer_sequence = None;
            self.layer_session_id = Some(session_id.to_owned());
        }
        if transaction.viewport_width == 0
            || transaction.viewport_height == 0
            || self
                .layer_sequence
                .is_some_and(|sequence| transaction.sequence <= sequence)
        {
            return Err("ASTRA_EMU_STAGE_LAYER_TRANSACTION_INVALID".into());
        }
        let mut touched = BTreeSet::new();
        for operation in transaction.operations {
            match operation {
                RuntimeLiveLayerOperation::Create(layer) => {
                    if !touched.insert(layer.layer_id.clone())
                        || self.layer_states.contains_key(&layer.layer_id)
                    {
                        return Err("ASTRA_EMU_STAGE_LAYER_CREATE_CONFLICT".into());
                    }
                    self.layer_states.insert(layer.layer_id.clone(), layer);
                }
                RuntimeLiveLayerOperation::Update(layer) => {
                    if !touched.insert(layer.layer_id.clone())
                        || !self.layer_states.contains_key(&layer.layer_id)
                    {
                        return Err("ASTRA_EMU_STAGE_LAYER_UPDATE_CONFLICT".into());
                    }
                    self.layer_states.insert(layer.layer_id.clone(), layer);
                }
                RuntimeLiveLayerOperation::Destroy { layer_id } => {
                    if !touched.insert(layer_id.clone())
                        || self.layer_states.remove(&layer_id).is_none()
                    {
                        return Err("ASTRA_EMU_STAGE_LAYER_DESTROY_CONFLICT".into());
                    }
                    if let Some(texture_id) = self.layer_filtered_ids.remove(&layer_id) {
                        self.textures.remove(&texture_id);
                    }
                    self.layer_filter_graphs.remove(&layer_id);
                }
            }
        }
        self.layer_sequence = Some(transaction.sequence);

        let retained_surfaces = self
            .layer_states
            .values()
            .map(|layer| layer.surface_id.clone())
            .collect::<BTreeSet<_>>();
        let removed = self
            .layer_surface_ids
            .keys()
            .filter(|surface_id| !retained_surfaces.contains(*surface_id))
            .cloned()
            .collect::<Vec<_>>();
        for surface_id in removed {
            if let Some(texture_id) = self.layer_surface_ids.remove(&surface_id) {
                self.textures.remove(&texture_id);
            }
        }

        let mut layers = self.layer_states.values().cloned().collect::<Vec<_>>();
        layers.sort_by(|left, right| {
            left.z_index
                .cmp(&right.z_index)
                .then_with(|| left.layer_id.cmp(&right.layer_id))
        });
        let mut draws = Vec::with_capacity(layers.len());
        for layer in layers {
            let texture_id = match self.layer_surface_ids.get(&layer.surface_id).copied() {
                Some(texture_id) => texture_id,
                None => {
                    let texture_id = self.next_layer_texture_id;
                    if texture_id >= VIDEO_TEXTURE_ID {
                        return Err("ASTRA_EMU_STAGE_LAYER_TEXTURE_ID_EXHAUSTED".into());
                    }
                    self.next_layer_texture_id = texture_id
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_STAGE_LAYER_TEXTURE_ID_EXHAUSTED".to_owned())?;
                    self.layer_surface_ids
                        .insert(layer.surface_id.clone(), texture_id);
                    texture_id
                }
            };
            let existing_generation = self
                .textures
                .get(&texture_id)
                .map(|resource| resource.generation);
            if existing_generation != Some(layer.generation) {
                if matches!(
                    layer.damage,
                    astra_plugin_abi::RuntimeLiveSurfaceDamage::Unchanged
                ) {
                    return Err("ASTRA_EMU_STAGE_LAYER_UNCHANGED_GENERATION".into());
                }
                if let astra_plugin_abi::RuntimeLiveSurfaceDamage::Rects(rects) = &layer.damage {
                    for rect in rects {
                        if rect.width == 0
                            || rect.height == 0
                            || rect
                                .x
                                .checked_add(rect.width)
                                .is_none_or(|right| right > layer.width)
                            || rect
                                .y
                                .checked_add(rect.height)
                                .is_none_or(|bottom| bottom > layer.height)
                        {
                            return Err("ASTRA_EMU_STAGE_LAYER_DAMAGE_BOUNDS".into());
                        }
                    }
                }
                let rgba8 = surfaces
                    .with_committed_surface(
                        session_id,
                        &layer.surface_id,
                        layer.generation,
                        |pixels, width, height, stride, format| {
                            if width != layer.width || height != layer.height {
                                return Err("ASTRA_EMU_STAGE_LAYER_SURFACE_DIMENSIONS".to_owned());
                            }
                            copy_surface_to_straight_rgba8(pixels, width, height, stride, format)
                                .map_err(str::to_owned)
                        },
                    )
                    .map_err(|error| error.to_string())??;
                match existing_generation {
                    Some(generation) => {
                        if layer.generation <= generation {
                            return Err("ASTRA_EMU_STAGE_LAYER_GENERATION".into());
                        }
                        self.upload_live_partial(
                            context,
                            texture_id,
                            layer.generation,
                            0,
                            0,
                            layer.width,
                            layer.height,
                            RuntimeLiveTextureFormat::Rgba8,
                            rgba8.into(),
                            RuntimeLiveSceneCompositing::LinearSrgb,
                        )?;
                    }
                    None => self.upload_live(
                        context,
                        texture_id,
                        layer.generation,
                        layer.width,
                        layer.height,
                        RuntimeLiveTextureFormat::Rgba8,
                        rgba8.into(),
                        RuntimeLiveSceneCompositing::LinearSrgb,
                    )?,
                }
            }
            let draw_texture_id = match &layer.filter_graph {
                Some(graph) if graph.nodes.is_empty() => {
                    validate_runtime_filter_graph(graph)?;
                    self.remove_layer_filter_texture(&layer.layer_id);
                    texture_id
                }
                Some(graph) => {
                    let graph_changed =
                        self.layer_filter_graphs.get(&layer.layer_id) != Some(graph);
                    let filtered_id = self.layer_filtered_texture_id(&layer.layer_id)?;
                    let filtered_generation = self
                        .textures
                        .get(&filtered_id)
                        .map(|resource| resource.generation);
                    if graph_changed || filtered_generation != Some(layer.generation) {
                        self.apply_layer_filter_graph(
                            context,
                            texture_id,
                            filtered_id,
                            layer.generation,
                            graph,
                        )?;
                        self.layer_filter_graphs
                            .insert(layer.layer_id.clone(), graph.clone());
                    }
                    filtered_id
                }
                None => {
                    self.remove_layer_filter_texture(&layer.layer_id);
                    texture_id
                }
            };
            draws.push(layer_draw(&layer, draw_texture_id)?);
        }

        let width = transaction.viewport_width;
        let height = transaction.viewport_height;
        let vertex_bytes = runtime_live_scene_vertex_bytes(&draws, width, height)?;
        let vertex_buffer = (!vertex_bytes.is_empty()).then(|| {
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("astra.emu.stage.layer-vertices"),
                    contents: &vertex_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.stage.layer-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.stage.layer-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vertex_buffer) = &vertex_buffer {
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            }
            for (draw_index, draw) in draws.iter().enumerate() {
                self.draw_live(
                    &mut pass,
                    draw,
                    draw_index,
                    width,
                    height,
                    RuntimeLiveSceneCompositing::LinearSrgb,
                )?;
            }
        }
        context.queue.submit([encoder.finish()]);
        Ok(())
    }

    fn layer_filtered_texture_id(&mut self, layer_id: &str) -> Result<u32, String> {
        if let Some(texture_id) = self.layer_filtered_ids.get(layer_id) {
            return Ok(*texture_id);
        }
        let texture_id = self.next_layer_texture_id;
        if texture_id >= VIDEO_TEXTURE_ID {
            return Err("ASTRA_EMU_STAGE_LAYER_TEXTURE_ID_EXHAUSTED".into());
        }
        self.next_layer_texture_id = texture_id
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_STAGE_LAYER_TEXTURE_ID_EXHAUSTED".to_owned())?;
        self.layer_filtered_ids
            .insert(layer_id.to_owned(), texture_id);
        Ok(texture_id)
    }

    fn remove_layer_filter_texture(&mut self, layer_id: &str) {
        if let Some(texture_id) = self.layer_filtered_ids.remove(layer_id) {
            self.textures.remove(&texture_id);
        }
        self.layer_filter_graphs.remove(layer_id);
    }

    fn apply_layer_filter_graph(
        &mut self,
        context: &WgpuFrameContext<'_>,
        source_texture_id: u32,
        target_texture_id: u32,
        generation: u64,
        graph: &RuntimeLiveFilterGraph,
    ) -> Result<(), String> {
        let graph = validate_runtime_filter_graph(graph)?;
        let source = self
            .textures
            .get(&source_texture_id)
            .ok_or_else(|| "ASTRA_EMU_STAGE_FILTER_SOURCE_MISSING".to_owned())?;
        if source.format != LegacyTextureFormat::Rgba8
            || source.compositing != RuntimeLiveSceneCompositing::LinearSrgb
        {
            return Err("ASTRA_EMU_STAGE_FILTER_SOURCE_FORMAT".into());
        }
        let width = source.width;
        let height = source.height;
        let mut current: Option<wgpu::Texture> = None;
        for node in &graph.nodes {
            let source_view = match &current {
                Some(texture) => texture.create_view(&wgpu::TextureViewDescriptor::default()),
                None => source
                    ._texture
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            };
            let target = create_filter_target_texture(context.device, width, height);
            let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let (mode, values) = filter_node_uniform(node)?;
            self.run_filter_pass(
                context,
                &source_view,
                &target_view,
                width,
                height,
                mode,
                values,
            );
            current = Some(target);
        }
        let texture = current.ok_or_else(|| "ASTRA_EMU_STAGE_FILTER_GRAPH_EMPTY".to_owned())?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let linear_bind_group = texture_bind_group(
            context.device,
            &self.bind_group_layout,
            &view,
            &self.linear_sampler,
            "astra.emu.stage.filtered-resource-linear-bind-group",
        );
        let nearest_bind_group = texture_bind_group(
            context.device,
            &self.bind_group_layout,
            &view,
            &self.nearest_sampler,
            "astra.emu.stage.filtered-resource-nearest-bind-group",
        );
        self.textures.insert(
            target_texture_id,
            TextureResource {
                _texture: texture,
                linear_bind_group,
                nearest_bind_group,
                generation,
                width,
                height,
                format: LegacyTextureFormat::Rgba8,
                compositing: RuntimeLiveSceneCompositing::LinearSrgb,
            },
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn run_filter_pass(
        &self,
        context: &WgpuFrameContext<'_>,
        source_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        mode: u32,
        values: [f32; 4],
    ) {
        let uniform_bytes = filter_uniform_bytes(mode, width, height, values);
        let uniform = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("astra.emu.layer-filter.params"),
                contents: &uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("astra.emu.layer-filter.bind-group"),
                layout: &self.filter_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.layer-filter.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.layer-filter.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.filter_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        context.queue.submit([encoder.finish()]);
    }

    #[allow(dead_code)]
    fn render_scene_live(
        &mut self,
        context: &WgpuFrameContext<'_>,
        target: &wgpu::Texture,
        transaction: RuntimeLiveSceneTransaction,
    ) -> Result<(), String> {
        transaction.validate().map_err(|error| error.to_string())?;
        let RuntimeLiveSceneTransaction {
            width,
            height,
            compositing,
            resources,
            draws,
            reset_resources,
            ..
        } = transaction;
        if draws.iter().any(|draw| draw.texture_id == VIDEO_TEXTURE_ID)
            || resources.iter().any(|operation| match operation {
                RuntimeLiveSceneResourceOperation::CreateTexture { texture_id, .. }
                | RuntimeLiveSceneResourceOperation::UpdateTexture { texture_id, .. }
                | RuntimeLiveSceneResourceOperation::DestroyTexture { texture_id, .. } => {
                    *texture_id == VIDEO_TEXTURE_ID
                }
            })
        {
            return Err("ASTRA_EMU_STAGE_LIVE_VIDEO_TEXTURE_ID_RESERVED".into());
        }
        if reset_resources {
            self.textures.clear();
            self.video_source = None;
        } else if self
            .textures
            .values()
            .any(|resource| resource.compositing != compositing)
        {
            return Err("ASTRA_EMU_STAGE_COMPOSITING_RESOURCE_EPOCH".into());
        }
        for operation in resources {
            match operation {
                RuntimeLiveSceneResourceOperation::CreateTexture {
                    texture_id,
                    generation,
                    width,
                    height,
                    format,
                    pixels,
                } => {
                    if self.textures.contains_key(&texture_id) {
                        return Err("ASTRA_EMU_STAGE_LIVE_TEXTURE_DUPLICATE".into());
                    }
                    self.upload_live(
                        context,
                        texture_id,
                        generation,
                        width,
                        height,
                        format,
                        pixels,
                        compositing,
                    )?;
                }
                RuntimeLiveSceneResourceOperation::UpdateTexture {
                    texture_id,
                    generation,
                    x,
                    y,
                    width,
                    height,
                    format,
                    pixels,
                } => self.upload_live_partial(
                    context,
                    texture_id,
                    generation,
                    x,
                    y,
                    width,
                    height,
                    format,
                    pixels,
                    compositing,
                )?,
                RuntimeLiveSceneResourceOperation::DestroyTexture {
                    texture_id,
                    generation,
                } => {
                    let resource = self
                        .textures
                        .get(&texture_id)
                        .ok_or_else(|| "ASTRA_EMU_STAGE_LIVE_TEXTURE_MISSING".to_owned())?;
                    if generation <= resource.generation {
                        return Err("ASTRA_EMU_STAGE_LIVE_TEXTURE_GENERATION".into());
                    }
                    self.textures.remove(&texture_id);
                }
            }
        }
        let vertex_bytes = runtime_live_scene_vertex_bytes(&draws, width, height)?;
        let vertex_buffer = (!vertex_bytes.is_empty()).then(|| {
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("astra.emu.stage.live-scene-vertices"),
                    contents: &vertex_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.stage.live-scene-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.stage.live-scene-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vertex_buffer) = &vertex_buffer {
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            }
            for (draw_index, draw) in draws.iter().enumerate() {
                self.draw_live(&mut pass, draw, draw_index, width, height, compositing)?;
            }
        }
        context.queue.submit([encoder.finish()]);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn upload_live(
        &mut self,
        context: &WgpuFrameContext<'_>,
        texture_id: u32,
        generation: u64,
        width: u32,
        height: u32,
        format: RuntimeLiveTextureFormat,
        pixels: OwnedByteBuffer,
        compositing: RuntimeLiveSceneCompositing,
    ) -> Result<(), String> {
        let format = legacy_texture_format(format);
        let rgba = to_rgba(format, pixels.as_slice())?;
        if rgba.len() != texture_byte_len(width, height, 4)? {
            return Err("ASTRA_EMU_STAGE_LIVE_TEXTURE_LENGTH".into());
        }
        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("astra.emu.stage.live-resource"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: scene_texture_format(compositing),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
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
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let linear_bind_group = texture_bind_group(
            context.device,
            &self.bind_group_layout,
            &view,
            &self.linear_sampler,
            "astra.emu.stage.live-resource-linear-bind-group",
        );
        let nearest_bind_group = texture_bind_group(
            context.device,
            &self.bind_group_layout,
            &view,
            &self.nearest_sampler,
            "astra.emu.stage.live-resource-nearest-bind-group",
        );
        self.textures.insert(
            texture_id,
            TextureResource {
                _texture: texture,
                linear_bind_group,
                nearest_bind_group,
                generation,
                width,
                height,
                format,
                compositing,
            },
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn upload_live_partial(
        &mut self,
        context: &WgpuFrameContext<'_>,
        texture_id: u32,
        generation: u64,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: RuntimeLiveTextureFormat,
        pixels: OwnedByteBuffer,
        compositing: RuntimeLiveSceneCompositing,
    ) -> Result<(), String> {
        let format = legacy_texture_format(format);
        let resource = self
            .textures
            .get(&texture_id)
            .ok_or_else(|| "ASTRA_EMU_STAGE_LIVE_TEXTURE_MISSING".to_owned())?;
        if resource.generation == 0
            || generation <= resource.generation
            || resource.format != format
            || resource.compositing != compositing
            || x.checked_add(width)
                .is_none_or(|right| right > resource.width)
            || y.checked_add(height)
                .is_none_or(|bottom| bottom > resource.height)
        {
            return Err("ASTRA_EMU_STAGE_LIVE_TEXTURE_REGION".into());
        }
        let rgba = to_rgba(format, pixels.as_slice())?;
        if rgba.len() != texture_byte_len(width, height, 4)? {
            return Err("ASTRA_EMU_STAGE_LIVE_TEXTURE_LENGTH".into());
        }
        context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &resource._texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
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
        self.textures
            .get_mut(&texture_id)
            .ok_or_else(|| "ASTRA_EMU_STAGE_LIVE_TEXTURE_MISSING".to_owned())?
            .generation = generation;
        Ok(())
    }

    fn render_video(
        &mut self,
        context: &WgpuFrameContext<'_>,
        target: &wgpu::Texture,
        frame: HostVideoFrame,
        compositing: RuntimeLiveSceneCompositing,
    ) -> Result<(), String> {
        if !self
            .video_source
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, &frame.rgba8))
        {
            let expected = usize::try_from(frame.width)
                .ok()
                .and_then(|width| {
                    usize::try_from(frame.height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| "ASTRA_EMU_VIDEO_FRAME_BOUNDS".to_owned())?;
            if frame.rgba8.len() != expected {
                return Err("ASTRA_EMU_VIDEO_FRAME_LENGTH".into());
            }
            let texture = context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("astra.emu.video.frame"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: scene_texture_format(compositing),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba8,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let linear_bind_group = texture_bind_group(
                context.device,
                &self.bind_group_layout,
                &view,
                &self.linear_sampler,
                "astra.emu.video.frame-linear-bind-group",
            );
            let nearest_bind_group = texture_bind_group(
                context.device,
                &self.bind_group_layout,
                &view,
                &self.nearest_sampler,
                "astra.emu.video.frame-nearest-bind-group",
            );
            self.textures.insert(
                VIDEO_TEXTURE_ID,
                TextureResource {
                    _texture: texture,
                    linear_bind_group,
                    nearest_bind_group,
                    generation: 0,
                    width: frame.width,
                    height: frame.height,
                    format: LegacyTextureFormat::Rgba8,
                    compositing,
                },
            );
            self.video_source = Some(Arc::clone(&frame.rgba8));
        }
        let draw = fullscreen_video_draw(
            VIDEO_TEXTURE_ID,
            frame.stage_width,
            frame.stage_height,
            frame.mode,
        );
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.video.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.video.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.draw(
                context.device,
                &mut pass,
                &draw,
                frame.stage_width,
                frame.stage_height,
                compositing,
            )?;
        }
        context.queue.submit([encoder.finish()]);
        Ok(())
    }

    fn draw<'a>(
        &'a self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'a>,
        draw: &LegacyDrawV1,
        stage_width: u32,
        stage_height: u32,
        compositing: RuntimeLiveSceneCompositing,
    ) -> Result<(), String> {
        let resource = self
            .textures
            .get(&draw.texture_id)
            .ok_or_else(|| "ASTRA_EMU_STAGE_TEXTURE_MISSING".to_owned())?;
        let pipeline = self.pipeline(compositing, draw.blend);
        let bytes = vertex_bytes(draw, stage_width, stage_height)?;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("astra.emu.stage.vertices"),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(
            0,
            match draw.texture_filter {
                LegacyTextureFilter::Nearest => &resource.nearest_bind_group,
                LegacyTextureFilter::Linear => &resource.linear_bind_group,
            },
            &[],
        );
        if let Some(scissor) = draw.scissor {
            if scissor.x < 0 || scissor.y < 0 || scissor.width <= 0 || scissor.height <= 0 {
                return Err("ASTRA_EMU_STAGE_SCISSOR_INVALID".into());
            }
            let right = u32::try_from(scissor.x)
                .ok()
                .and_then(|x| x.checked_add(scissor.width as u32));
            let bottom = u32::try_from(scissor.y)
                .ok()
                .and_then(|y| y.checked_add(scissor.height as u32));
            if right.is_none_or(|value| value > stage_width)
                || bottom.is_none_or(|value| value > stage_height)
            {
                return Err("ASTRA_EMU_STAGE_SCISSOR_BOUNDS".into());
            }
            pass.set_scissor_rect(
                scissor.x as u32,
                scissor.y as u32,
                scissor.width as u32,
                scissor.height as u32,
            );
        } else {
            pass.set_scissor_rect(0, 0, stage_width, stage_height);
        }
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..4, 0..1);
        Ok(())
    }

    fn draw_live<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        draw: &RuntimeLiveDraw,
        draw_index: usize,
        stage_width: u32,
        stage_height: u32,
        compositing: RuntimeLiveSceneCompositing,
    ) -> Result<(), String> {
        let resource = self
            .textures
            .get(&draw.texture_id)
            .ok_or_else(|| "ASTRA_EMU_STAGE_LIVE_TEXTURE_MISSING".to_owned())?;
        let pipeline = match draw.blend {
            RuntimeLiveBlendMode::Alpha => self.pipeline(compositing, LegacyBlendMode::Alpha),
            RuntimeLiveBlendMode::Additive => self.pipeline(compositing, LegacyBlendMode::Add),
            RuntimeLiveBlendMode::Opaque => self.pipeline(compositing, LegacyBlendMode::Opaque),
            RuntimeLiveBlendMode::Multiply => self.pipeline(compositing, LegacyBlendMode::Multiply),
            RuntimeLiveBlendMode::Screen => self.pipeline(compositing, LegacyBlendMode::Screen),
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(
            0,
            match draw.texture_filter {
                RuntimeLiveTextureFilter::Nearest => &resource.nearest_bind_group,
                RuntimeLiveTextureFilter::Linear => &resource.linear_bind_group,
            },
            &[],
        );
        if let Some(scissor) = draw.scissor {
            if scissor.width == 0
                || scissor.height == 0
                || scissor
                    .x
                    .checked_add(scissor.width)
                    .is_none_or(|right| right > stage_width)
                || scissor
                    .y
                    .checked_add(scissor.height)
                    .is_none_or(|bottom| bottom > stage_height)
            {
                return Err("ASTRA_EMU_STAGE_LIVE_SCISSOR_BOUNDS".into());
            }
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        } else {
            pass.set_scissor_rect(0, 0, stage_width, stage_height);
        }
        let first_vertex = u32::try_from(draw_index)
            .ok()
            .and_then(|index| index.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_STAGE_LIVE_DRAW_INDEX".to_owned())?;
        pass.draw(first_vertex..first_vertex + 4, 0..1);
        Ok(())
    }

    fn pipeline(
        &self,
        compositing: RuntimeLiveSceneCompositing,
        blend: LegacyBlendMode,
    ) -> &wgpu::RenderPipeline {
        match (compositing, blend) {
            (RuntimeLiveSceneCompositing::LinearSrgb, LegacyBlendMode::Alpha) => {
                &self.alpha_pipeline
            }
            (RuntimeLiveSceneCompositing::LinearSrgb, LegacyBlendMode::Add) => &self.add_pipeline,
            (RuntimeLiveSceneCompositing::LinearSrgb, LegacyBlendMode::Opaque) => {
                &self.opaque_pipeline
            }
            (RuntimeLiveSceneCompositing::LinearSrgb, LegacyBlendMode::Multiply) => {
                &self.multiply_pipeline
            }
            (RuntimeLiveSceneCompositing::LinearSrgb, LegacyBlendMode::Screen) => {
                &self.screen_pipeline
            }
            (RuntimeLiveSceneCompositing::EncodedSrgb, LegacyBlendMode::Alpha) => {
                &self.encoded_alpha_pipeline
            }
            (RuntimeLiveSceneCompositing::EncodedSrgb, LegacyBlendMode::Add) => {
                &self.encoded_add_pipeline
            }
            (RuntimeLiveSceneCompositing::EncodedSrgb, LegacyBlendMode::Opaque) => {
                &self.encoded_opaque_pipeline
            }
            (RuntimeLiveSceneCompositing::EncodedSrgb, LegacyBlendMode::Multiply) => {
                &self.encoded_multiply_pipeline
            }
            (RuntimeLiveSceneCompositing::EncodedSrgb, LegacyBlendMode::Screen) => {
                &self.encoded_screen_pipeline
            }
        }
    }
}

fn to_rgba<'a>(format: LegacyTextureFormat, pixels: &'a [u8]) -> Result<Cow<'a, [u8]>, String> {
    match format {
        LegacyTextureFormat::Rgba8 => Ok(Cow::Borrowed(pixels)),
        LegacyTextureFormat::LumaAlpha8 => {
            if !pixels.len().is_multiple_of(2) {
                return Err("ASTRA_EMU_STAGE_TEXTURE_LENGTH".into());
            }
            let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(2));
            for pair in pixels.as_chunks::<2>().0.iter() {
                rgba.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
            }
            Ok(Cow::Owned(rgba))
        }
    }
}

fn legacy_texture_format(format: RuntimeLiveTextureFormat) -> LegacyTextureFormat {
    match format {
        RuntimeLiveTextureFormat::Rgba8 => LegacyTextureFormat::Rgba8,
        RuntimeLiveTextureFormat::LumaAlpha8 => LegacyTextureFormat::LumaAlpha8,
    }
}

fn texture_byte_len(width: u32, height: u32, bytes_per_pixel: usize) -> Result<usize, String> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
        .ok_or_else(|| "ASTRA_EMU_STAGE_TEXTURE_BOUNDS".to_owned())
}

fn texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn validate_runtime_filter_graph(graph: &RuntimeLiveFilterGraph) -> Result<FilterGraph, String> {
    let mut nodes = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        let mut params = BTreeMap::new();
        for param in &node.params {
            let value = match &param.value {
                RuntimeLiveFilterParam::Float(value) => {
                    if !value.is_finite() {
                        return Err("ASTRA_EMU_STAGE_FILTER_PARAM".into());
                    }
                    FilterParam::Float(*value)
                }
                RuntimeLiveFilterParam::Int(value) => FilterParam::Int(*value),
                RuntimeLiveFilterParam::Bool(value) => FilterParam::Bool(*value),
                RuntimeLiveFilterParam::Text(value) => FilterParam::Text(value.clone()),
            };
            if params.insert(param.key.clone(), value).is_some() {
                return Err("ASTRA_EMU_STAGE_FILTER_PARAM_DUPLICATE".into());
            }
        }
        nodes.push(FilterNode {
            id: node.id.clone(),
            kind: node.kind.clone(),
            input: runtime_filter_target(node.input),
            output: runtime_filter_target(node.output),
            params,
            deterministic: node.deterministic,
            allow_cpu_fallback: node.allow_cpu_fallback,
        });
    }
    let graph = FilterGraph {
        schema: graph.schema.clone(),
        nodes,
    };
    if !FilterValidator
        .validate(&graph)
        .blocking_diagnostics()
        .is_empty()
    {
        return Err("ASTRA_EMU_STAGE_FILTER_GRAPH_INVALID".into());
    }
    Ok(graph)
}

fn runtime_filter_target(target: RuntimeLiveFilterTarget) -> FilterTarget {
    match target {
        RuntimeLiveFilterTarget::Background => FilterTarget::Background,
        RuntimeLiveFilterTarget::Character => FilterTarget::Character,
        RuntimeLiveFilterTarget::Ui => FilterTarget::Ui,
        RuntimeLiveFilterTarget::Text => FilterTarget::Text,
        RuntimeLiveFilterTarget::Video => FilterTarget::Video,
        RuntimeLiveFilterTarget::Final => FilterTarget::Final,
    }
}

fn filter_node_uniform(node: &FilterNode) -> Result<(u32, [f32; 4]), String> {
    let float = |name: &str| match node.params.get(name) {
        Some(FilterParam::Float(value)) => Ok(*value),
        _ => Err("ASTRA_EMU_STAGE_FILTER_PARAM_TYPE".to_owned()),
    };
    match node.kind.as_str() {
        "astra.filter.bloom" => Ok((4, [float("intensity")?, 0.0, 0.0, 0.0])),
        "astra.filter.color_matrix" => {
            Ok((5, [float("r")?, float("g")?, float("b")?, float("a")?]))
        }
        "astra.filter.fade" => Ok((4 + 2, [float("amount")?, 0.0, 0.0, 0.0])),
        _ => Err("ASTRA_EMU_STAGE_FILTER_KIND_UNSUPPORTED".into()),
    }
}

fn filter_uniform_bytes(mode: u32, width: u32, height: u32, values: [f32; 4]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(&mode.to_ne_bytes());
    bytes.extend_from_slice(&0_u32.to_ne_bytes());
    bytes.extend_from_slice(&(width as f32).to_ne_bytes());
    bytes.extend_from_slice(&(height as f32).to_ne_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn create_filter_target_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra.emu.stage.filtered-resource"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn layer_draw(layer: &RuntimeLiveLayerState, texture_id: u32) -> Result<RuntimeLiveDraw, String> {
    let point = |x: f32, y: f32| RuntimeLiveVertex {
        x: layer.transform.m11 * x + layer.transform.m21 * y + layer.transform.tx,
        y: layer.transform.m12 * x + layer.transform.m22 * y + layer.transform.ty,
        u: x / layer.width as f32,
        v: y / layer.height as f32,
        color: [
            255,
            255,
            255,
            (layer.opacity * 255.0).round().clamp(0.0, 255.0) as u8,
        ],
    };
    let vertices = [
        point(0.0, 0.0),
        point(layer.width as f32, 0.0),
        point(0.0, layer.height as f32),
        point(layer.width as f32, layer.height as f32),
    ];
    if vertices
        .iter()
        .any(|vertex| !vertex.x.is_finite() || !vertex.y.is_finite())
    {
        return Err("ASTRA_EMU_STAGE_LAYER_TRANSFORM_INVALID".into());
    }
    let scissor = layer.clip.map(|clip| RuntimeLiveScissor {
        x: clip.x,
        y: clip.y,
        width: clip.width,
        height: clip.height,
    });
    Ok(RuntimeLiveDraw {
        texture_id,
        vertices,
        blend: match layer.blend {
            RuntimeLiveLayerBlend::Opaque => RuntimeLiveBlendMode::Opaque,
            RuntimeLiveLayerBlend::Alpha => RuntimeLiveBlendMode::Alpha,
            RuntimeLiveLayerBlend::Add => RuntimeLiveBlendMode::Additive,
            RuntimeLiveLayerBlend::Multiply => RuntimeLiveBlendMode::Multiply,
            RuntimeLiveLayerBlend::Screen => RuntimeLiveBlendMode::Screen,
        },
        texture_filter: match layer.texture_filter {
            RuntimeLiveLayerFilter::Nearest => RuntimeLiveTextureFilter::Nearest,
            RuntimeLiveLayerFilter::Linear => RuntimeLiveTextureFilter::Linear,
        },
        scissor,
    })
}

fn runtime_live_scene_vertex_bytes(
    draws: &[RuntimeLiveDraw],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("ASTRA_EMU_STAGE_LIVE_DIMENSIONS".into());
    }
    let capacity = draws
        .len()
        .checked_mul(4 * 8 * size_of::<f32>())
        .ok_or_else(|| "ASTRA_EMU_STAGE_LIVE_VERTEX_BOUNDS".to_owned())?;
    let mut bytes = Vec::with_capacity(capacity);
    for draw in draws {
        for vertex in draw.vertices {
            let values = [
                vertex.x * 2.0 / width as f32 - 1.0,
                1.0 - vertex.y * 2.0 / height as f32,
                vertex.u,
                vertex.v,
                f32::from(vertex.color[0]) / 255.0,
                f32::from(vertex.color[1]) / 255.0,
                f32::from(vertex.color[2]) / 255.0,
                f32::from(vertex.color[3]) / 255.0,
            ];
            if values.iter().any(|value| !value.is_finite()) {
                return Err("ASTRA_EMU_STAGE_LIVE_VERTEX_INVALID".into());
            }
            for value in values {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
        }
    }
    Ok(bytes)
}

fn fullscreen_video_draw(
    texture_id: u32,
    width: u32,
    height: u32,
    mode: LegacyVideoMode,
) -> LegacyDrawV1 {
    let alpha = match mode {
        LegacyVideoMode::ModalWithAudio | LegacyVideoMode::LayerNoAudio => 1.0,
    };
    LegacyDrawV1 {
        texture_id,
        vertices: [
            LegacyVertexV1 {
                position: [0.0, 0.0],
                tex_coord: [0.0, 0.0],
                color: [1.0, 1.0, 1.0, alpha],
            },
            LegacyVertexV1 {
                position: [width as f32, 0.0],
                tex_coord: [1.0, 0.0],
                color: [1.0, 1.0, 1.0, alpha],
            },
            LegacyVertexV1 {
                position: [0.0, height as f32],
                tex_coord: [0.0, 1.0],
                color: [1.0, 1.0, 1.0, alpha],
            },
            LegacyVertexV1 {
                position: [width as f32, height as f32],
                tex_coord: [1.0, 1.0],
                color: [1.0, 1.0, 1.0, alpha],
            },
        ],
        blend: LegacyBlendMode::Alpha,
        texture_filter: LegacyTextureFilter::Linear,
        scissor: None,
    }
}

fn vertex_bytes(draw: &LegacyDrawV1, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(4 * 8 * 4);
    for vertex in &draw.vertices {
        let values = [
            vertex.position[0] * 2.0 / width as f32 - 1.0,
            1.0 - vertex.position[1] * 2.0 / height as f32,
            vertex.tex_coord[0],
            vertex.tex_coord[1],
            vertex.color[0],
            vertex.color[1],
            vertex.color[2],
            vertex.color[3],
        ];
        if values.iter().any(|value| !value.is_finite()) {
            return Err("ASTRA_EMU_STAGE_VERTEX_INVALID".into());
        }
        for value in values {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    Ok(bytes)
}

fn create_stage_texture(device: &wgpu::Device) -> wgpu::Texture {
    create_stage_texture_with_dimensions(device, STAGE_WIDTH, STAGE_HEIGHT)
}

fn create_stage_texture_with_dimensions(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra.emu.stage"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn scene_texture_format(compositing: RuntimeLiveSceneCompositing) -> wgpu::TextureFormat {
    match compositing {
        RuntimeLiveSceneCompositing::LinearSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        RuntimeLiveSceneCompositing::EncodedSrgb => wgpu::TextureFormat::Rgba8Unorm,
    }
}

fn create_scene_texture_with_dimensions(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    compositing: RuntimeLiveSceneCompositing,
) -> wgpu::Texture {
    let encoded_view_formats = [wgpu::TextureFormat::Rgba8UnormSrgb];
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra.emu.scene"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: scene_texture_format(compositing),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: match compositing {
            RuntimeLiveSceneCompositing::LinearSrgb => &[],
            RuntimeLiveSceneCompositing::EncodedSrgb => &encoded_view_formats,
        },
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    blend: wgpu::BlendState,
    name: &str,
) -> wgpu::RenderPipeline {
    create_pipeline_for_format(
        device,
        bind_group_layout,
        blend,
        name,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
}

fn create_pipeline_for_format(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    blend: wgpu::BlendState,
    name: &str,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra.emu.stage.shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra.emu.stage.pipeline-layout"),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(name),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 32,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_filter_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra.emu.filter.shader"),
        source: wgpu::ShaderSource::Wgsl(FILTER_SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra.emu.filter.pipeline-layout"),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("astra.emu.filter.pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn alpha_blend() -> wgpu::BlendState {
    wgpu::BlendState::ALPHA_BLENDING
}

fn add_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
}

fn multiply_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
}

fn screen_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
}

const SHADER: &str = r#"
struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) color: vec4<f32>,
};
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) color: vec4<f32>,
};
@vertex fn vs_main(input: VertexIn) -> VertexOut {
    var output: VertexOut;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.tex_coord = input.tex_coord;
    output.color = input.color;
    return output;
}
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, input.tex_coord) * input.color;
}
"#;

const FILTER_SHADER: &str = r#"
struct FilterParams {
    mode: u32,
    _padding: u32,
    dimensions: vec2<f32>,
    values: vec4<f32>,
};
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
@group(0) @binding(2) var<uniform> params: FilterParams;

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    var output: VertexOut;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.tex_coord = vec2<f32>(
        (positions[index].x + 1.0) * 0.5,
        (1.0 - positions[index].y) * 0.5
    );
    return output;
}

@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    var color = textureSample(image, image_sampler, input.tex_coord);
    if params.mode == 1u {
        let luminance = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        color = vec4<f32>(vec3<f32>(luminance), color.a);
    } else if params.mode == 2u {
        let scanline = 0.92 + 0.08 * sin(input.tex_coord.y * params.dimensions.y * 3.14159265);
        let edge = smoothstep(0.0, 0.035, input.tex_coord.x)
            * smoothstep(0.0, 0.035, 1.0 - input.tex_coord.x)
            * smoothstep(0.0, 0.035, input.tex_coord.y)
            * smoothstep(0.0, 0.035, 1.0 - input.tex_coord.y);
        color = vec4<f32>(color.rgb * scanline * mix(0.82, 1.0, edge), color.a);
    } else if params.mode == 3u {
        color = vec4<f32>(color.rgb * vec3<f32>(1.06, 1.0, 0.91), color.a);
    } else if params.mode == 4u {
        color = vec4<f32>(min(color.rgb + vec3<f32>(params.values.x), vec3<f32>(1.0)), color.a);
    } else if params.mode == 5u {
        color = color * params.values;
    } else if params.mode == 6u {
        color = vec4<f32>(color.rgb * params.values.x, color.a);
    }
    return color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{LegacyScissorV1, LegacyVertexV1};
    use astra_plugin_abi::{RuntimeLiveFilterNode, RuntimeLiveFilterParamEntry};

    fn filter_node(id: &str, kind: &str, params: Vec<(&str, f32)>) -> RuntimeLiveFilterNode {
        RuntimeLiveFilterNode {
            id: id.into(),
            kind: kind.into(),
            input: RuntimeLiveFilterTarget::Final,
            output: RuntimeLiveFilterTarget::Final,
            params: params
                .into_iter()
                .map(|(key, value)| RuntimeLiveFilterParamEntry {
                    key: key.into(),
                    value: RuntimeLiveFilterParam::Float(value),
                })
                .collect(),
            deterministic: true,
            allow_cpu_fallback: false,
        }
    }

    #[test]
    fn typed_filter_graph_is_validated_without_string_resolution() {
        let graph = RuntimeLiveFilterGraph {
            schema: "astra.filter_graph.v1".into(),
            nodes: vec![filter_node(
                "fade",
                "astra.filter.fade",
                vec![("amount", 0.5)],
            )],
        };
        let graph = validate_runtime_filter_graph(&graph).unwrap();
        assert_eq!(filter_node_uniform(&graph.nodes[0]).unwrap().0, 6);

        let mut invalid = graph.clone();
        invalid.nodes[0]
            .params
            .insert("unexpected".into(), FilterParam::Float(1.0));
        assert!(!FilterValidator
            .validate(&invalid)
            .blocking_diagnostics()
            .is_empty());
    }

    #[test]
    fn vertex_projection_uses_runtime_stage_dimensions() {
        let draw = LegacyDrawV1 {
            texture_id: 1,
            vertices: [
                LegacyVertexV1 {
                    position: [0.0, 0.0],
                    tex_coord: [0.0, 0.0],
                    color: [1.0; 4],
                },
                LegacyVertexV1 {
                    position: [1280.0, 0.0],
                    tex_coord: [1.0, 0.0],
                    color: [1.0; 4],
                },
                LegacyVertexV1 {
                    position: [0.0, 720.0],
                    tex_coord: [0.0, 1.0],
                    color: [1.0; 4],
                },
                LegacyVertexV1 {
                    position: [1280.0, 720.0],
                    tex_coord: [1.0, 1.0],
                    color: [1.0; 4],
                },
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor: Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            }),
        };
        let bytes = vertex_bytes(&draw, 1280, 720).unwrap();
        let floats = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|value| f32::from_ne_bytes(*value))
            .collect::<Vec<_>>();
        assert_eq!((floats[0], floats[1]), (-1.0, 1.0));
        assert_eq!((floats[24], floats[25]), (1.0, -1.0));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn final_frame_filter_pipelines_validate_on_wgpu_29() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("ASTRA_EMU_FILTER_TEST_ADAPTER");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("ASTRA_EMU_FILTER_TEST_DEVICE");
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut gpu = StageGpu::new(&device);
        let source = create_stage_texture_with_dimensions(&device, 64, 48);
        let target = create_stage_texture_with_dimensions(&device, 64, 48);
        let context = WgpuFrameContext {
            device: &device,
            queue: &queue,
        };
        for preset in ["none", "grayscale", "crt-soft", "warm"] {
            gpu.apply_final_filter(
                &context,
                &source,
                &target,
                preset,
                RuntimeLiveSceneCompositing::LinearSrgb,
            )
            .unwrap();
        }
        gpu.upload_live(
            &context,
            7,
            1,
            64,
            48,
            RuntimeLiveTextureFormat::Rgba8,
            vec![128_u8; 64 * 48 * 4].into(),
            RuntimeLiveSceneCompositing::LinearSrgb,
        )
        .unwrap();
        let graph = RuntimeLiveFilterGraph {
            schema: "astra.filter_graph.v1".into(),
            nodes: vec![
                filter_node("bloom", "astra.filter.bloom", vec![("intensity", 0.25)]),
                filter_node(
                    "matrix",
                    "astra.filter.color_matrix",
                    vec![("r", 1.0), ("g", 0.8), ("b", 0.6), ("a", 1.0)],
                ),
                filter_node("fade", "astra.filter.fade", vec![("amount", 0.75)]),
            ],
        };
        gpu.apply_layer_filter_graph(&context, 7, 8, 1, &graph)
            .unwrap();
        assert_eq!(gpu.textures.get(&8).unwrap().generation, 1);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("ASTRA_EMU_FILTER_TEST_POLL");
        let error = pollster::block_on(error_scope.pop());
        assert!(error.is_none(), "filter validation failed: {error:?}");
    }
}
