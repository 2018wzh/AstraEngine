use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacyRenderFrameV1, LegacyTextureFormat, LegacyVertexV1,
    LegacyVideoMode,
};
use astra_emu_manager::{AstraUnderlayRenderer, TranslationOverlayView, WgpuFrameContext};
use wgpu::util::DeviceExt;

use crate::{video_executor::HostVideoFrame, RuntimeBridge};

const STAGE_WIDTH: u32 = 1024;
const STAGE_HEIGHT: u32 = 768;

pub(crate) struct ManagerStageRenderer {
    pub(crate) texture: Option<wgpu::Texture>,
    pub(crate) scene_texture: Option<wgpu::Texture>,
    pub(crate) runtime: Rc<RefCell<RuntimeBridge>>,
    pub(crate) gpu: Option<StageGpu>,
    pub(crate) stage_width: u32,
    pub(crate) stage_height: u32,
    pub(crate) texture_dirty: bool,
    pub(crate) scene_initialized: bool,
}

pub(crate) struct StageGpu {
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    alpha_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    multiply_pipeline: wgpu::RenderPipeline,
    filter_bind_group_layout: wgpu::BindGroupLayout,
    filter_pipeline: wgpu::RenderPipeline,
    textures: BTreeMap<u32, TextureResource>,
    video_source: Option<Arc<[u8]>>,
}

struct TextureResource {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    hash: String,
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
        let (frame, video, filter_preset) = {
            let mut runtime = self
                .runtime
                .try_borrow_mut()
                .map_err(|_| "ASTRA_EMU_RUNTIME_BORROW_CONFLICT".to_owned())?;
            if runtime.active.is_some() {
                runtime.step_if_due()?;
            }
            (
                runtime.take_latest_render_frame(),
                runtime.current_video_frame(),
                runtime.filter_preset().to_owned(),
            )
        };
        if let Some(frame) = frame {
            if frame.width != self.stage_width || frame.height != self.stage_height {
                self.texture = Some(create_stage_texture_with_dimensions(
                    context.device,
                    frame.width,
                    frame.height,
                ));
                self.scene_texture = Some(create_stage_texture_with_dimensions(
                    context.device,
                    frame.width,
                    frame.height,
                ));
                self.stage_width = frame.width;
                self.stage_height = frame.height;
                self.texture_dirty = true;
                self.scene_initialized = false;
            }
            let texture = self
                .scene_texture
                .as_ref()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
            let gpu = self
                .gpu
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?;
            gpu.render(&context, texture, frame)?;
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
            self.gpu
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_STAGE_RENDERER_NOT_SETUP".to_owned())?
                .render_video(&context, texture, video)?;
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
                .apply_final_filter(&context, source, target, &filter_preset)?;
        }
        Ok(())
    }

    fn teardown(&mut self) {
        self.gpu = None;
        self.texture = None;
        self.scene_texture = None;
        self.scene_initialized = false;
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("astra.emu.stage.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let alpha_pipeline = create_pipeline(device, &bind_group_layout, alpha_blend(), "alpha");
        let add_pipeline = create_pipeline(device, &bind_group_layout, add_blend(), "add");
        let multiply_pipeline =
            create_pipeline(device, &bind_group_layout, multiply_blend(), "multiply");
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
            sampler,
            alpha_pipeline,
            add_pipeline,
            multiply_pipeline,
            filter_bind_group_layout,
            filter_pipeline,
            textures: BTreeMap::new(),
            video_source: None,
        }
    }

    fn apply_final_filter(
        &mut self,
        context: &WgpuFrameContext<'_>,
        source: &wgpu::Texture,
        target: &wgpu::Texture,
        preset_id: &str,
    ) -> Result<(), String> {
        let mode = match preset_id {
            "none" => 0_u32,
            "grayscale" => 1,
            "crt-soft" => 2,
            "warm" => 3,
            _ => return Err("ASTRA_EMU_FILTER_PRESET_UNSUPPORTED".into()),
        };
        let mut params = Vec::with_capacity(16);
        params.extend_from_slice(&mode.to_ne_bytes());
        params.extend_from_slice(&0_u32.to_ne_bytes());
        params.extend_from_slice(&(source.width() as f32).to_ne_bytes());
        params.extend_from_slice(&(source.height() as f32).to_ne_bytes());
        let uniform = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("astra.emu.filter.params"),
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
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
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
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

    fn render(
        &mut self,
        context: &WgpuFrameContext<'_>,
        target: &wgpu::Texture,
        frame: LegacyRenderFrameV1,
    ) -> Result<(), String> {
        let stage_width = frame.width;
        let stage_height = frame.height;
        for update in frame.texture_updates {
            self.upload(context, update)?;
        }
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.stage.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra.emu.stage.pass"),
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
            for draw in &frame.draws {
                self.draw(context.device, &mut pass, draw, stage_width, stage_height)?;
            }
        }
        context.queue.submit([encoder.finish()]);
        Ok(())
    }

    fn upload(
        &mut self,
        context: &WgpuFrameContext<'_>,
        update: astra_emu_family_api::LegacyTextureUpdateV1,
    ) -> Result<(), String> {
        let rgba = match update.format {
            LegacyTextureFormat::Rgba8 => update.pixels,
            LegacyTextureFormat::LumaAlpha8 => {
                let mut rgba = Vec::with_capacity(update.pixels.len().saturating_mul(2));
                for pair in update.pixels.chunks_exact(2) {
                    rgba.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
                }
                rgba
            }
        };
        let expected = usize::try_from(update.width)
            .ok()
            .and_then(|width| {
                usize::try_from(update.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_STAGE_TEXTURE_BOUNDS".to_owned())?;
        if rgba.len() != expected {
            return Err("ASTRA_EMU_STAGE_TEXTURE_LENGTH".into());
        }
        let hash = update.content_hash.to_string();
        if self
            .textures
            .get(&update.texture_id)
            .is_some_and(|old| old.hash == hash)
        {
            return Ok(());
        }
        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("astra.emu.stage.resource"),
            size: wgpu::Extent3d {
                width: update.width,
                height: update.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
                bytes_per_row: Some(update.width * 4),
                rows_per_image: Some(update.height),
            },
            wgpu::Extent3d {
                width: update.width,
                height: update.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("astra.emu.stage.resource-bind-group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
        self.textures.insert(
            update.texture_id,
            TextureResource {
                _texture: texture,
                bind_group,
                hash,
            },
        );
        Ok(())
    }

    fn render_video(
        &mut self,
        context: &WgpuFrameContext<'_>,
        target: &wgpu::Texture,
        frame: HostVideoFrame,
    ) -> Result<(), String> {
        const VIDEO_TEXTURE_ID: u32 = u32::MAX;
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
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
            let bind_group = context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("astra.emu.video.frame-bind-group"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
            self.textures.insert(
                VIDEO_TEXTURE_ID,
                TextureResource {
                    _texture: texture,
                    bind_group,
                    hash: "host-video".into(),
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
    ) -> Result<(), String> {
        let resource = self
            .textures
            .get(&draw.texture_id)
            .ok_or_else(|| "ASTRA_EMU_STAGE_TEXTURE_MISSING".to_owned())?;
        let pipeline = match draw.blend {
            LegacyBlendMode::Alpha => &self.alpha_pipeline,
            LegacyBlendMode::Add => &self.add_pipeline,
            LegacyBlendMode::Multiply => &self.multiply_pipeline,
        };
        let bytes = vertex_bytes(draw, stage_width, stage_height)?;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("astra.emu.stage.vertices"),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &resource.bind_group, &[]);
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

fn create_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    blend: wgpu::BlendState,
    name: &str,
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
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
    }
    return color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{LegacyScissorV1, LegacyVertexV1};

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
            scissor: Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            }),
        };
        let bytes = vertex_bytes(&draw, 1280, 720).unwrap();
        let floats = bytes
            .chunks_exact(4)
            .map(|value| f32::from_ne_bytes(value.try_into().unwrap()))
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
            gpu.apply_final_filter(&context, &source, &target, preset)
                .unwrap();
        }
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("ASTRA_EMU_FILTER_TEST_POLL");
        let error = pollster::block_on(error_scope.pop());
        assert!(error.is_none(), "filter validation failed: {error:?}");
    }
}
