use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use std::borrow::Cow;

use astra_byte_source::OwnedByteBuffer;
use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacySurfaceFormatV9, LegacyTextureFilter, LegacyTextureFormat,
    LegacyVertexV1, LegacyVideoMode,
};
use astra_emu_manager::{AstraUnderlayRenderer, WgpuFrameContext};
use astra_emu_manager_core::{legacy_texture_format, PublishedFamilySurface};
use astra_media_core::{
    BlendMode, FilterGraph as MediaFilterGraph, FilterParam, FilterValidator, Layer2DContent,
    Layer2DDamage, Layer2DOperation, Layer2DState, Layer2DTransaction, RetainedLayer2DState,
    Surface2DFormat, TextureFilter2D,
};
use astra_plugin_abi::{
    RuntimeLiveBlendMode, RuntimeLiveDraw, RuntimeLiveSceneCompositing,
    RuntimeLiveSceneResourceOperation, RuntimeLiveSceneTransaction, RuntimeLiveScissor,
    RuntimeLiveTextureFilter, RuntimeLiveTextureFormat, RuntimeLiveVertex,
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
    pub(crate) layer_state: RetainedLayer2DState,
    pub(crate) layer_texture_ids: BTreeMap<String, u32>,
    pub(crate) layer_filter_texture_ids: BTreeMap<String, [u32; 2]>,
    pub(crate) next_layer_texture_id: u32,
}

pub(crate) struct StageGpu {
    bind_group_layout: wgpu::BindGroupLayout,
    linear_sampler: wgpu::Sampler,
    nearest_sampler: wgpu::Sampler,
    alpha_pipeline: wgpu::RenderPipeline,
    premultiplied_alpha_pipeline: wgpu::RenderPipeline,
    opaque_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    premultiplied_add_pipeline: wgpu::RenderPipeline,
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
    layer_filters: BTreeMap<String, LayerFilterCache>,
    video_source: Option<Arc<[u8]>>,
}

struct LayerFilterCache {
    graph: MediaFilterGraph,
    width: u32,
    height: u32,
    source_format: Surface2DFormat,
    source_generation: u64,
    passes: Vec<LayerFilterPass>,
    output_texture_id: u32,
}

struct LayerFilterPass {
    bind_group: wgpu::BindGroup,
    _uniform: wgpu::Buffer,
    output_texture_id: u32,
}

struct TextureResource {
    _texture: wgpu::Texture,
    linear_bind_group: wgpu::BindGroup,
    nearest_bind_group: wgpu::BindGroup,
    generation: u64,
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    surface_format: Option<Surface2DFormat>,
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

    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        let (scene_commit, layer_commit, video, filter_preset) = {
            let mut runtime = self
                .runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?;
            (
                runtime.take_latest_live_scene(),
                runtime.take_latest_live_layers(),
                runtime.current_video_frame(),
                runtime.filter_preset().to_owned(),
            )
        };
        if scene_commit.is_some() && layer_commit.is_some() {
            return Err("ASTRA_EMU_PRESENTATION_LANE_MIXED".into());
        }
        if let Some(scene_commit) = scene_commit {
            if self.scene_compositing != Some(scene_commit.compositing) {
                if self.scene_initialized && !scene_commit.reset_resources {
                    return Err("ASTRA_EMU_STAGE_COMPOSITING_REQUIRES_RESOURCE_RESET".into());
                }
                self.scene_texture = Some(create_scene_texture_with_dimensions(
                    context.device,
                    scene_commit.width,
                    scene_commit.height,
                    scene_commit.compositing,
                ));
                self.scene_compositing = Some(scene_commit.compositing);
                self.scene_initialized = false;
            }
            if scene_commit.width != self.stage_width || scene_commit.height != self.stage_height {
                self.texture = Some(create_stage_texture_with_dimensions(
                    context.device,
                    scene_commit.width,
                    scene_commit.height,
                ));
                self.scene_texture = Some(create_scene_texture_with_dimensions(
                    context.device,
                    scene_commit.width,
                    scene_commit.height,
                    scene_commit.compositing,
                ));
                self.stage_width = scene_commit.width;
                self.stage_height = scene_commit.height;
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
                .render_scene_live(&context, texture, scene_commit)?;
            self.scene_initialized = true;
            self.runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .acknowledge_presentation();
        }
        if let Some(layer_commit) = layer_commit {
            self.render_layer_transaction(&context, layer_commit)?;
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
        self.layer_state = RetainedLayer2DState::default();
        self.layer_texture_ids.clear();
        self.layer_filter_texture_ids.clear();
        self.next_layer_texture_id = 1;
    }
}

impl ManagerStageRenderer {
    fn render_layer_transaction(
        &mut self,
        context: &WgpuFrameContext<'_>,
        transaction: Layer2DTransaction,
    ) -> Result<(), String> {
        let viewport_width = transaction.viewport_width;
        let viewport_height = transaction.viewport_height;
        let ordered = self
            .layer_state
            .apply(&transaction)
            .map_err(|error| error.to_string())?;

        for operation in &transaction.operations {
            let layer = match operation {
                Layer2DOperation::Create(layer) | Layer2DOperation::Update(layer) => layer,
                Layer2DOperation::Destroy(_) => continue,
            };
            let Layer2DContent::WritableSurface(reference) = &layer.content else {
                return Err("ASTRA_EMU_FAMILY_TEXTURE_RESOURCE_FORBIDDEN".into());
            };
            if matches!(reference.damage, Layer2DDamage::Unchanged) {
                continue;
            }
            let surface = self
                .runtime
                .try_borrow()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .take_published_surface(&reference.surface_id.0, reference.generation)?;
            let upload = self.upload_layer_surface(context, reference, &surface);
            let returned = self
                .runtime
                .try_borrow()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?
                .return_published_surface(surface);
            upload?;
            returned?;
        }

        let used_surfaces = ordered
            .iter()
            .filter_map(|layer| match &layer.content {
                Layer2DContent::WritableSurface(surface) => Some(surface.surface_id.0.clone()),
                Layer2DContent::TextureResource(_) => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let stale = self
            .layer_texture_ids
            .keys()
            .filter(|surface_id| !used_surfaces.contains(*surface_id))
            .cloned()
            .collect::<Vec<_>>();
        for surface_id in stale {
            if let Some(texture_id) = self.layer_texture_ids.remove(&surface_id) {
                self.gpu
                    .as_mut()
                    .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
                    .textures
                    .remove(&texture_id);
            }
            if let Some(texture_ids) = self.layer_filter_texture_ids.remove(&surface_id) {
                let gpu = self
                    .gpu
                    .as_mut()
                    .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
                gpu.layer_filters.remove(&surface_id);
                for texture_id in texture_ids {
                    gpu.textures.remove(&texture_id);
                }
            }
        }

        if viewport_width != self.stage_width || viewport_height != self.stage_height {
            self.texture = Some(create_stage_texture_with_dimensions(
                context.device,
                viewport_width,
                viewport_height,
            ));
            self.scene_texture = Some(create_scene_texture_with_dimensions(
                context.device,
                viewport_width,
                viewport_height,
                RuntimeLiveSceneCompositing::LinearSrgb,
            ));
            self.stage_width = viewport_width;
            self.stage_height = viewport_height;
            self.texture_dirty = true;
        }
        self.scene_compositing = Some(RuntimeLiveSceneCompositing::LinearSrgb);
        let mut resolved_texture_ids = BTreeMap::new();
        for layer in &ordered {
            let Layer2DContent::WritableSurface(surface) = &layer.content else {
                return Err("ASTRA_EMU_FAMILY_TEXTURE_RESOURCE_FORBIDDEN".into());
            };
            let base_texture_id = *self
                .layer_texture_ids
                .get(&surface.surface_id.0)
                .ok_or_else(|| "ASTRA_EMU_LAYER_TEXTURE_MISSING".to_owned())?;
            let texture_id = if let Some(graph) = &layer.filter_graph {
                let output_ids = if let Some(ids) = self
                    .layer_filter_texture_ids
                    .get(&surface.surface_id.0)
                    .copied()
                {
                    ids
                } else {
                    let ids = [
                        self.allocate_layer_texture_id()?,
                        self.allocate_layer_texture_id()?,
                    ];
                    self.layer_filter_texture_ids
                        .insert(surface.surface_id.0.clone(), ids);
                    ids
                };
                self.gpu
                    .as_mut()
                    .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
                    .apply_layer_filter_graph(
                        context,
                        &surface.surface_id.0,
                        base_texture_id,
                        output_ids,
                        surface,
                        graph,
                    )?
            } else {
                base_texture_id
            };
            resolved_texture_ids.insert(surface.surface_id.0.clone(), texture_id);
        }
        let draws = ordered
            .iter()
            .map(|layer| self.layer_draw(layer, &resolved_texture_ids))
            .collect::<Result<Vec<_>, _>>()?;
        let target = self
            .scene_texture
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
        self.gpu
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
            .render_layer_composition(context, target, &draws, viewport_width, viewport_height)?;
        self.scene_initialized = true;
        Ok(())
    }

    fn allocate_layer_texture_id(&mut self) -> Result<u32, String> {
        let texture_id = self.next_layer_texture_id;
        self.next_layer_texture_id = self
            .next_layer_texture_id
            .checked_add(1)
            .filter(|id| *id != VIDEO_TEXTURE_ID)
            .ok_or_else(|| "ASTRA_EMU_LAYER_TEXTURE_ID_EXHAUSTED".to_owned())?;
        Ok(texture_id)
    }

    fn upload_layer_surface(
        &mut self,
        context: &WgpuFrameContext<'_>,
        reference: &astra_media_core::WritableSurface2DRef,
        surface: &PublishedFamilySurface,
    ) -> Result<(), String> {
        let format = match surface.format {
            LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => {
                Surface2DFormat::Rgba8SrgbPremultiplied
            }
            LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => {
                Surface2DFormat::Bgra8SrgbPremultiplied
            }
        };
        if surface.surface_id != reference.surface_id.0
            || surface.generation != reference.generation
            || surface.width != reference.width
            || surface.height != reference.height
            || surface.stride != reference.stride
            || format != reference.format
        {
            return Err("ASTRA_EMU_SURFACE_PRESENTATION_METADATA_MISMATCH".into());
        }
        let expected = usize::try_from(surface.stride)
            .ok()
            .and_then(|stride| {
                usize::try_from(surface.height)
                    .ok()
                    .and_then(|height| stride.checked_mul(height))
            })
            .ok_or_else(|| "ASTRA_EMU_SURFACE_PRESENTATION_BOUNDS".to_owned())?;
        if surface.pixels.len() != expected {
            return Err("ASTRA_EMU_SURFACE_PRESENTATION_LENGTH".into());
        }
        let texture_id =
            if let Some(texture_id) = self.layer_texture_ids.get(&surface.surface_id).copied() {
                texture_id
            } else {
                let texture_id = self.allocate_layer_texture_id()?;
                self.layer_texture_ids
                    .insert(surface.surface_id.clone(), texture_id);
                texture_id
            };
        self.gpu
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
            .upload_layer_surface(context, texture_id, surface, format, &reference.damage)
    }

    fn layer_draw(
        &self,
        layer: &Layer2DState,
        resolved_texture_ids: &BTreeMap<String, u32>,
    ) -> Result<RuntimeLiveDraw, String> {
        let Layer2DContent::WritableSurface(surface) = &layer.content else {
            return Err("ASTRA_EMU_FAMILY_TEXTURE_RESOURCE_FORBIDDEN".into());
        };
        let texture_id = *self
            .layer_texture_ids
            .get(&surface.surface_id.0)
            .ok_or_else(|| "ASTRA_EMU_LAYER_TEXTURE_MISSING".to_owned())?;
        let texture_id = resolved_texture_ids
            .get(&surface.surface_id.0)
            .copied()
            .unwrap_or(texture_id);
        let transform = layer.transform;
        let point = |x: f32, y: f32| {
            (
                transform.m11 * x + transform.m21 * y + transform.tx,
                transform.m12 * x + transform.m22 * y + transform.ty,
            )
        };
        let width = surface.width as f32;
        let height = surface.height as f32;
        let positions = [
            point(0.0, 0.0),
            point(width, 0.0),
            point(0.0, height),
            point(width, height),
        ];
        let uvs = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
        let alpha = (layer.opacity * 255.0).round() as u8;
        let vertices = std::array::from_fn(|index| RuntimeLiveVertex {
            x: positions[index].0,
            y: positions[index].1,
            u: uvs[index].0,
            v: uvs[index].1,
            color: [alpha, alpha, alpha, alpha],
        });
        let scissor = layer.clip.map(|clip| RuntimeLiveScissor {
            x: clip.x as u32,
            y: clip.y as u32,
            width: clip.width,
            height: clip.height,
        });
        Ok(RuntimeLiveDraw {
            texture_id,
            vertices,
            blend: match layer.blend {
                BlendMode::Alpha => RuntimeLiveBlendMode::Alpha,
                BlendMode::Add => RuntimeLiveBlendMode::Additive,
                BlendMode::Opaque => RuntimeLiveBlendMode::Opaque,
                BlendMode::Multiply => RuntimeLiveBlendMode::Multiply,
                BlendMode::Screen => RuntimeLiveBlendMode::Screen,
            },
            texture_filter: match layer.texture_filter {
                TextureFilter2D::Nearest => RuntimeLiveTextureFilter::Nearest,
                TextureFilter2D::Linear => RuntimeLiveTextureFilter::Linear,
            },
            scissor,
        })
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
        let premultiplied_alpha_pipeline = create_pipeline(
            device,
            &bind_group_layout,
            premultiplied_alpha_blend(),
            "premultiplied-alpha",
        );
        let opaque_pipeline = create_pipeline(
            device,
            &bind_group_layout,
            wgpu::BlendState::REPLACE,
            "opaque",
        );
        let add_pipeline = create_pipeline(device, &bind_group_layout, add_blend(), "add");
        let premultiplied_add_pipeline = create_pipeline(
            device,
            &bind_group_layout,
            premultiplied_add_blend(),
            "premultiplied-add",
        );
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
            premultiplied_alpha_pipeline,
            opaque_pipeline,
            add_pipeline,
            premultiplied_add_pipeline,
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
            layer_filters: BTreeMap::new(),
            video_source: None,
        }
    }

    fn upload_layer_surface(
        &mut self,
        context: &WgpuFrameContext<'_>,
        texture_id: u32,
        surface: &PublishedFamilySurface,
        format: Surface2DFormat,
        damage: &Layer2DDamage,
    ) -> Result<(), String> {
        let gpu_format = match format {
            Surface2DFormat::Rgba8SrgbPremultiplied => wgpu::TextureFormat::Rgba8UnormSrgb,
            Surface2DFormat::Bgra8SrgbPremultiplied => wgpu::TextureFormat::Bgra8UnormSrgb,
        };
        let recreate = self.textures.get(&texture_id).is_none_or(|resource| {
            resource.width != surface.width
                || resource.height != surface.height
                || resource.surface_format != Some(format)
        });
        if recreate {
            let texture = context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("astra.emu.layer.surface"),
                size: wgpu::Extent3d {
                    width: surface.width,
                    height: surface.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: gpu_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let linear_bind_group = texture_bind_group(
                context.device,
                &self.bind_group_layout,
                &view,
                &self.linear_sampler,
                "astra.emu.layer.surface.linear",
            );
            let nearest_bind_group = texture_bind_group(
                context.device,
                &self.bind_group_layout,
                &view,
                &self.nearest_sampler,
                "astra.emu.layer.surface.nearest",
            );
            self.textures.insert(
                texture_id,
                TextureResource {
                    _texture: texture,
                    linear_bind_group,
                    nearest_bind_group,
                    generation: 0,
                    width: surface.width,
                    height: surface.height,
                    format: LegacyTextureFormat::Rgba8,
                    surface_format: Some(format),
                    compositing: RuntimeLiveSceneCompositing::LinearSrgb,
                },
            );
        }
        let resource = self
            .textures
            .get(&texture_id)
            .ok_or_else(|| "ASTRA_EMU_LAYER_TEXTURE_MISSING".to_owned())?;
        if surface.generation <= resource.generation {
            return Err("ASTRA_EMU_LAYER_SURFACE_GENERATION".into());
        }
        let regions = match damage {
            Layer2DDamage::Unchanged => return Ok(()),
            Layer2DDamage::Full => vec![(0, 0, surface.width, surface.height)],
            Layer2DDamage::Rects(rects) => rects
                .iter()
                .map(|rect| (rect.x as u32, rect.y as u32, rect.width, rect.height))
                .collect(),
        };
        for (x, y, width, height) in regions {
            let offset = u64::from(y)
                .checked_mul(u64::from(surface.stride))
                .and_then(|offset| offset.checked_add(u64::from(x) * 4))
                .ok_or_else(|| "ASTRA_EMU_LAYER_UPLOAD_OFFSET".to_owned())?;
            context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &resource._texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                surface.pixels.as_slice(),
                wgpu::TexelCopyBufferLayout {
                    offset,
                    bytes_per_row: Some(surface.stride),
                    rows_per_image: Some(surface.height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.textures
            .get_mut(&texture_id)
            .ok_or_else(|| "ASTRA_EMU_LAYER_TEXTURE_MISSING".to_owned())?
            .generation = surface.generation;
        Ok(())
    }

    fn apply_layer_filter_graph(
        &mut self,
        context: &WgpuFrameContext<'_>,
        surface_id: &str,
        source_texture_id: u32,
        output_texture_ids: [u32; 2],
        surface: &astra_media_core::WritableSurface2DRef,
        graph: &MediaFilterGraph,
    ) -> Result<u32, String> {
        let validation = FilterValidator.validate(graph);
        if !validation.blocking_diagnostics().is_empty() {
            return Err("ASTRA_EMU_LAYER_FILTER_GRAPH_INVALID".into());
        }
        if graph.nodes.is_empty() {
            return Ok(source_texture_id);
        }
        let rebuild = self.layer_filters.get(surface_id).is_none_or(|cache| {
            cache.graph != *graph
                || cache.width != surface.width
                || cache.height != surface.height
                || cache.source_format != surface.format
        });
        if rebuild {
            self.layer_filters.remove(surface_id);
            for texture_id in output_texture_ids {
                self.textures.insert(
                    texture_id,
                    create_filter_texture_resource(
                        context.device,
                        &self.bind_group_layout,
                        &self.linear_sampler,
                        &self.nearest_sampler,
                        surface.width,
                        surface.height,
                    ),
                );
            }
            let mut passes = Vec::with_capacity(graph.nodes.len());
            let mut input_texture_id = source_texture_id;
            for (index, node) in graph.nodes.iter().enumerate() {
                let output_texture_id = output_texture_ids[index % output_texture_ids.len()];
                let input = self
                    .textures
                    .get(&input_texture_id)
                    .ok_or_else(|| "ASTRA_EMU_LAYER_FILTER_INPUT_MISSING".to_owned())?;
                let input_view = input
                    ._texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let uniform_bytes = layer_filter_uniform_bytes(node)?;
                let uniform =
                    context
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("astra.emu.layer.filter.params"),
                            contents: &uniform_bytes,
                            usage: wgpu::BufferUsages::UNIFORM,
                        });
                let bind_group = context
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("astra.emu.layer.filter.bind-group"),
                        layout: &self.filter_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&input_view),
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
                passes.push(LayerFilterPass {
                    bind_group,
                    _uniform: uniform,
                    output_texture_id,
                });
                input_texture_id = output_texture_id;
            }
            self.layer_filters.insert(
                surface_id.to_owned(),
                LayerFilterCache {
                    graph: graph.clone(),
                    width: surface.width,
                    height: surface.height,
                    source_format: surface.format,
                    source_generation: 0,
                    passes,
                    output_texture_id: input_texture_id,
                },
            );
        }
        let cache = self
            .layer_filters
            .get(surface_id)
            .ok_or_else(|| "ASTRA_EMU_LAYER_FILTER_CACHE_MISSING".to_owned())?;
        if cache.source_generation == surface.generation {
            return Ok(cache.output_texture_id);
        }
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.layer.filter.encoder"),
            });
        for pass_resource in &cache.passes {
            let output = self
                .textures
                .get(&pass_resource.output_texture_id)
                .ok_or_else(|| "ASTRA_EMU_LAYER_FILTER_OUTPUT_MISSING".to_owned())?;
            let output_view = output
                ._texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.layer.filter.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
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
            pass.set_bind_group(0, &pass_resource.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        context.queue.submit([encoder.finish()]);
        let cache = self
            .layer_filters
            .get_mut(surface_id)
            .ok_or_else(|| "ASTRA_EMU_LAYER_FILTER_CACHE_MISSING".to_owned())?;
        cache.source_generation = surface.generation;
        Ok(cache.output_texture_id)
    }

    fn render_layer_composition(
        &mut self,
        context: &WgpuFrameContext<'_>,
        target: &wgpu::Texture,
        draws: &[RuntimeLiveDraw],
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let vertex_bytes = runtime_live_scene_vertex_bytes(draws, width, height)?;
        let vertex_buffer = (!vertex_bytes.is_empty()).then(|| {
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("astra.emu.layer.vertices"),
                    contents: &vertex_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.layer.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.layer.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            if let Some(vertex_buffer) = &vertex_buffer {
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            }
            for (draw_index, draw) in draws.iter().enumerate() {
                self.draw_layer(&mut pass, draw, draw_index, width, height)?;
            }
        }
        context.queue.submit([encoder.finish()]);
        Ok(())
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
        let mut params = Vec::with_capacity(32);
        params.extend_from_slice(&mode.to_ne_bytes());
        params.extend_from_slice(&0_u32.to_ne_bytes());
        params.extend_from_slice(&0_u32.to_ne_bytes());
        params.extend_from_slice(&0_u32.to_ne_bytes());
        params.extend_from_slice(&(source.width() as f32).to_ne_bytes());
        params.extend_from_slice(&(source.height() as f32).to_ne_bytes());
        params.extend_from_slice(&0_f32.to_ne_bytes());
        params.extend_from_slice(&0_f32.to_ne_bytes());
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
                surface_format: None,
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
                    surface_format: None,
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

    fn draw_layer<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        draw: &RuntimeLiveDraw,
        draw_index: usize,
        stage_width: u32,
        stage_height: u32,
    ) -> Result<(), String> {
        let resource = self
            .textures
            .get(&draw.texture_id)
            .filter(|resource| resource.surface_format.is_some())
            .ok_or_else(|| "ASTRA_EMU_LAYER_TEXTURE_MISSING".to_owned())?;
        let pipeline = match draw.blend {
            RuntimeLiveBlendMode::Alpha => &self.premultiplied_alpha_pipeline,
            RuntimeLiveBlendMode::Additive => &self.premultiplied_add_pipeline,
            RuntimeLiveBlendMode::Opaque => &self.opaque_pipeline,
            RuntimeLiveBlendMode::Multiply => &self.multiply_pipeline,
            RuntimeLiveBlendMode::Screen => &self.screen_pipeline,
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
                return Err("ASTRA_EMU_LAYER_SCISSOR_BOUNDS".into());
            }
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        } else {
            pass.set_scissor_rect(0, 0, stage_width, stage_height);
        }
        let first_vertex = u32::try_from(draw_index)
            .ok()
            .and_then(|index| index.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_LAYER_DRAW_INDEX".to_owned())?;
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

fn create_filter_texture_resource(
    device: &wgpu::Device,
    texture_layout: &wgpu::BindGroupLayout,
    linear_sampler: &wgpu::Sampler,
    nearest_sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
) -> TextureResource {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra.emu.layer.filter.output"),
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
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let linear_bind_group = texture_bind_group(
        device,
        texture_layout,
        &view,
        linear_sampler,
        "astra.emu.layer.filter.output.linear",
    );
    let nearest_bind_group = texture_bind_group(
        device,
        texture_layout,
        &view,
        nearest_sampler,
        "astra.emu.layer.filter.output.nearest",
    );
    TextureResource {
        _texture: texture,
        linear_bind_group,
        nearest_bind_group,
        generation: 0,
        width,
        height,
        format: LegacyTextureFormat::Rgba8,
        surface_format: Some(Surface2DFormat::Rgba8SrgbPremultiplied),
        compositing: RuntimeLiveSceneCompositing::LinearSrgb,
    }
}

fn layer_filter_uniform_bytes(node: &astra_media_core::FilterNode) -> Result<Vec<u8>, String> {
    let (mode, values) = match node.kind.as_str() {
        "astra.filter.bloom" => (10_u32, [filter_float(node, "intensity")?, 0.0, 0.0, 0.0]),
        "astra.filter.fade" => (11_u32, [filter_float(node, "amount")?, 0.0, 0.0, 0.0]),
        "astra.filter.color_matrix" => (
            12_u32,
            [
                filter_float(node, "r")?,
                filter_float(node, "g")?,
                filter_float(node, "b")?,
                filter_float(node, "a")?,
            ],
        ),
        _ => return Err("ASTRA_EMU_LAYER_FILTER_KIND_UNSUPPORTED".into()),
    };
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(&mode.to_ne_bytes());
    bytes.extend_from_slice(&0_u32.to_ne_bytes());
    bytes.extend_from_slice(&0_u32.to_ne_bytes());
    bytes.extend_from_slice(&0_u32.to_ne_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    Ok(bytes)
}

fn filter_float(node: &astra_media_core::FilterNode, name: &str) -> Result<f32, String> {
    match node.params.get(name) {
        Some(FilterParam::Float(value)) => Ok(*value),
        _ => Err("ASTRA_EMU_LAYER_FILTER_PARAM_INVALID".into()),
    }
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

fn premultiplied_alpha_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
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

fn premultiplied_add_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
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
    header: vec4<u32>,
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
    if params.header.x == 1u {
        let luminance = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        color = vec4<f32>(vec3<f32>(luminance), color.a);
    } else if params.header.x == 2u {
        let scanline = 0.92 + 0.08 * sin(input.tex_coord.y * params.values.y * 3.14159265);
        let edge = smoothstep(0.0, 0.035, input.tex_coord.x)
            * smoothstep(0.0, 0.035, 1.0 - input.tex_coord.x)
            * smoothstep(0.0, 0.035, input.tex_coord.y)
            * smoothstep(0.0, 0.035, 1.0 - input.tex_coord.y);
        color = vec4<f32>(color.rgb * scanline * mix(0.82, 1.0, edge), color.a);
    } else if params.header.x == 3u {
        color = vec4<f32>(color.rgb * vec3<f32>(1.06, 1.0, 0.91), color.a);
    } else if params.header.x == 10u {
        color = vec4<f32>(min(color.rgb + vec3<f32>(params.values.x), vec3<f32>(1.0)), color.a);
    } else if params.header.x == 11u {
        color = vec4<f32>(color.rgb * params.values.x, color.a);
    } else if params.header.x == 12u {
        color = color * params.values;
    }
    return color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{LegacyScissorV1, LegacyVertexV1};
    use astra_media_core::{FilterNode, FilterTarget};

    #[test]
    fn typed_layer_filter_uniforms_are_fixed_size_and_hash_free() {
        let node = FilterNode {
            id: "fade".into(),
            kind: "astra.filter.fade".into(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params: BTreeMap::from([("amount".into(), FilterParam::Float(0.5))]),
            deterministic: true,
            allow_cpu_fallback: false,
        };
        let bytes = layer_filter_uniform_bytes(&node).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(u32::from_ne_bytes(bytes[0..4].try_into().unwrap()), 11);
        assert_eq!(f32::from_ne_bytes(bytes[16..20].try_into().unwrap()), 0.5);
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
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("ASTRA_EMU_FILTER_TEST_POLL");
        let error = pollster::block_on(error_scope.pop());
        assert!(error.is_none(), "filter validation failed: {error:?}");
    }
}
