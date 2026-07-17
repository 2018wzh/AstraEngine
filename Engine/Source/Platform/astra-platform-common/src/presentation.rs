use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use astra_platform::{
    CapturedFrame, PlatformError, PlatformErrorCode, PlatformEventKind, RgbaFrame, SceneFrame,
};

use crate::glyph_atlas::WgpuGlyphAtlasRenderer;

/// Shared hardware-only WGPU presentation path. Backends own event loops and
/// surface creation; this core owns adapter policy, RGBA upload, presentation,
/// resize, loss classification, and padded readback layout.
pub struct WgpuPresentationCore {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    glyph_renderer: WgpuGlyphAtlasRenderer,
    last_upload: Option<UploadFrame>,
    last_frame: Option<RgbaFrame>,
    last_scene_frame: Option<SceneFrame>,
    last_sequence: Option<u64>,
    device_lost: Arc<AtomicBool>,
    #[cfg(feature = "platform-test-driver")]
    test_device_loss: AtomicBool,
}

pub struct WgpuReadback {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
}

struct UploadFrame {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl WgpuPresentationCore {
    pub async fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        hardware_only: bool,
    ) -> Result<Self, PlatformError> {
        if width == 0 || height == 0 {
            return Err(invalid(
                "surface.create",
                "surface dimensions must be non-zero",
            ));
        }
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| unavailable("surface.create", "hardware adapter is unavailable"))?;
        if hardware_only && adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return Err(unavailable(
                "surface.create",
                "software adapters are forbidden by the selected profile",
            ));
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|_| unavailable("surface.create", "wgpu device creation failed"))?;
        let device_lost = Arc::new(AtomicBool::new(false));
        install_device_lost_callback(&device, Arc::clone(&device_lost));
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| unavailable("surface.create", "surface configuration is unavailable"))?;
        if !surface
            .get_capabilities(&adapter)
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            return Err(unavailable(
                "surface.create",
                "required FIFO present mode is unavailable",
            ));
        }
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let (layout, sampler, pipeline) = pipeline(&device, config.format);
        let glyph_renderer = WgpuGlyphAtlasRenderer::new(&device);
        Ok(Self {
            _instance: instance,
            surface,
            adapter,
            device,
            queue,
            config,
            layout,
            sampler,
            pipeline,
            glyph_renderer,
            last_upload: None,
            last_frame: None,
            last_scene_frame: None,
            last_sequence: None,
            device_lost,
            #[cfg(feature = "platform-test-driver")]
            test_device_loss: AtomicBool::new(false),
        })
    }

    pub fn present(&mut self, frame: RgbaFrame) -> Result<(), PlatformError> {
        self.ensure_device_available("surface.present_rgba")?;
        if frame.width == 0 || frame.height == 0 {
            return Err(invalid(
                "surface.present_rgba",
                "frame dimensions must be non-zero",
            ));
        }
        let expected_bytes = frame
            .width
            .checked_mul(frame.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(|| invalid("surface.present_rgba", "frame byte size overflows"))?;
        if frame.rgba8.len() != expected_bytes {
            return Err(invalid(
                "surface.present_rgba",
                "frame byte size does not match its dimensions",
            ));
        }
        let expected_sequence = match self.last_sequence {
            Some(sequence) => sequence.checked_add(1).ok_or_else(|| {
                invalid("surface.present_rgba", "frame sequence counter overflowed")
            })?,
            None => 1,
        };
        if frame.sequence != expected_sequence {
            return Err(invalid(
                "surface.present_rgba",
                "frame sequence is duplicated, skipped, or out of order",
            ));
        }
        if frame.width != self.config.width || frame.height != self.config.height {
            tracing::info!(
                event = "platform.wgpu.surface.resized",
                width = frame.width,
                height = frame.height,
                "wgpu surface resized at frame boundary"
            );
            self.config.width = frame.width;
            self.config.height = frame.height;
            self.surface.configure(&self.device, &self.config);
        }
        let texture = upload_frame(&self.device, &self.queue, &frame);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-platform-frame-bind-group"),
            layout: &self.layout,
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
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(value)
            | wgpu::CurrentSurfaceTexture::Suboptimal(value) => value,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                return Err(PlatformError::new(
                    PlatformErrorCode::ContextLost,
                    "surface.present_rgba",
                    "wgpu surface was lost",
                ))
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                return Err(unavailable(
                    "surface.present_rgba",
                    "surface frame acquisition failed",
                ))
            }
        };
        let output_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra-platform-frame-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra-platform-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        output.present();
        self.last_upload = Some(UploadFrame {
            texture,
            width: frame.width,
            height: frame.height,
        });
        self.last_sequence = Some(frame.sequence);
        self.last_frame = Some(frame);
        self.last_scene_frame = None;
        Ok(())
    }

    pub fn present_scene(&mut self, frame: SceneFrame) -> Result<(), PlatformError> {
        self.ensure_device_available("surface.present_scene")?;
        let expected_sequence = match self.last_sequence {
            Some(sequence) => sequence.checked_add(1).ok_or_else(|| {
                invalid("surface.present_scene", "frame sequence counter overflowed")
            })?,
            None => 1,
        };
        if frame.sequence != expected_sequence || frame.width == 0 || frame.height == 0 {
            return Err(invalid(
                "surface.present_scene",
                "text frame sequence or dimensions are invalid",
            ));
        }
        if frame.width != self.config.width || frame.height != self.config.height {
            self.config.width = frame.width;
            self.config.height = frame.height;
            self.surface.configure(&self.device, &self.config);
        }
        let prepared = self
            .glyph_renderer
            .render(&self.device, &self.queue, &frame)?;
        self.present_texture(&prepared.texture, "surface.present_scene")?;
        let texture = self.glyph_renderer.commit(prepared);
        self.last_upload = Some(UploadFrame {
            texture,
            width: frame.width,
            height: frame.height,
        });
        self.last_sequence = Some(frame.sequence);
        self.last_frame = None;
        self.last_scene_frame = Some(frame);
        Ok(())
    }

    pub fn begin_capture(&self) -> Result<WgpuReadback, PlatformError> {
        self.ensure_device_available("surface.capture")?;
        let upload = self
            .last_upload
            .as_ref()
            .ok_or_else(|| invalid("surface.capture", "surface has not presented a frame"))?;
        let row = upload
            .width
            .checked_mul(4)
            .ok_or_else(|| invalid("surface.capture", "frame row overflows"))?;
        let padded_row =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("astra-platform-frame-readback"),
            size: u64::from(padded_row) * u64::from(upload.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra-platform-frame-readback-encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &upload.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(upload.height),
                },
            },
            wgpu::Extent3d {
                width: upload.width,
                height: upload.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        Ok(WgpuReadback {
            buffer,
            width: upload.width,
            height: upload.height,
            padded_row,
        })
    }

    pub fn reconfigure_after_loss(&mut self) -> Result<(), PlatformError> {
        self.ensure_device_available("surface.reconfigure")?;
        if self.config.width == 0 || self.config.height == 0 {
            return Err(invalid(
                "surface.reconfigure",
                "surface dimensions must be non-zero",
            ));
        }
        self.surface.configure(&self.device, &self.config);
        tracing::warn!(
            event = "platform.wgpu.surface.reconfigured",
            width = self.config.width,
            height = self.config.height,
            "wgpu surface was explicitly reconfigured after context loss"
        );
        Ok(())
    }

    pub async fn recover_device(&mut self) -> Result<bool, PlatformError> {
        if !self.device_lost.load(Ordering::Acquire) {
            return Ok(false);
        }
        #[cfg(feature = "platform-test-driver")]
        if self.test_device_loss.swap(false, Ordering::AcqRel) {
            self.glyph_renderer.recover(&self.device);
            self.last_upload = if let Some(frame) = self.last_frame.as_ref() {
                Some(UploadFrame {
                    texture: upload_frame(&self.device, &self.queue, frame),
                    width: frame.width,
                    height: frame.height,
                })
            } else if let Some(frame) = self.last_scene_frame.as_ref() {
                Some(UploadFrame {
                    texture: self.glyph_renderer.render_retained(
                        &self.device,
                        &self.queue,
                        frame,
                    )?,
                    width: frame.width,
                    height: frame.height,
                })
            } else {
                None
            };
            self.device_lost.store(false, Ordering::Release);
            tracing::info!(
                event = "platform.wgpu.device.test_recovered",
                resource_rebuilt = self.last_upload.is_some(),
                "test driver rebuilt retained resources without replacing the hardware device"
            );
            return Ok(true);
        }
        tracing::warn!(
            event = "platform.wgpu.device.recovery_started",
            "wgpu device recovery started"
        );
        let (device, queue) = self
            .adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::DeviceLost,
                    "surface.recover_device",
                    "wgpu device recreation failed",
                )
            })?;
        let device_lost = Arc::new(AtomicBool::new(false));
        install_device_lost_callback(&device, Arc::clone(&device_lost));
        let (layout, sampler, pipeline) = pipeline(&device, self.config.format);
        self.surface.configure(&device, &self.config);
        self.glyph_renderer.recover(&device);
        let last_upload = if let Some(frame) = self.last_frame.as_ref() {
            Some(UploadFrame {
                texture: upload_frame(&device, &queue, frame),
                width: frame.width,
                height: frame.height,
            })
        } else if let Some(frame) = self.last_scene_frame.as_ref() {
            Some(UploadFrame {
                texture: self
                    .glyph_renderer
                    .render_retained(&device, &queue, frame)?,
                width: frame.width,
                height: frame.height,
            })
        } else {
            None
        };
        self.device = device;
        self.queue = queue;
        self.layout = layout;
        self.sampler = sampler;
        self.pipeline = pipeline;
        self.last_upload = last_upload;
        self.device_lost = device_lost;
        tracing::info!(
            event = "platform.wgpu.device.recovered",
            resource_rebuilt = self.last_upload.is_some(),
            "wgpu device and retained frame resources were rebuilt"
        );
        Ok(true)
    }

    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(Ordering::Acquire)
    }

    #[cfg(feature = "platform-test-driver")]
    pub fn inject_device_loss_for_test(&self) {
        self.test_device_loss.store(true, Ordering::Release);
        self.device_lost.store(true, Ordering::Release);
    }

    fn ensure_device_available(&self, operation: &'static str) -> Result<(), PlatformError> {
        if self.is_device_lost() {
            return Err(PlatformError::new(
                PlatformErrorCode::DeviceLost,
                operation,
                "wgpu device is lost and must be explicitly recovered",
            ));
        }
        Ok(())
    }

    pub fn poll(&self, poll_type: wgpu::PollType) -> Result<(), PlatformError> {
        self.ensure_device_available("surface.capture")?;
        self.device
            .poll(poll_type)
            .map(|_| ())
            .map_err(|_| unavailable("surface.capture", "GPU readback poll failed"))
    }

    fn present_texture(
        &self,
        texture: &wgpu::Texture,
        operation: &'static str,
    ) -> Result<(), PlatformError> {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-platform-text-frame-bind-group"),
            layout: &self.layout,
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
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(value)
            | wgpu::CurrentSurfaceTexture::Suboptimal(value) => value,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                return Err(PlatformError::new(
                    PlatformErrorCode::ContextLost,
                    operation,
                    "wgpu surface was lost",
                ));
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                return Err(unavailable(operation, "surface frame acquisition failed"));
            }
        };
        let output_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra-platform-text-frame-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra-platform-text-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        output.present();
        Ok(())
    }
}

pub fn wgpu_recovery_events(provider: &str, recovered: bool) -> Vec<PlatformEventKind> {
    let mut events = vec![PlatformEventKind::ContextLost {
        provider: provider.to_string(),
    }];
    if recovered {
        events.push(PlatformEventKind::ContextRestored {
            provider: provider.to_string(),
        });
    }
    events
}

pub fn wgpu_device_recovery_events(provider: &str, recovered: bool) -> Vec<PlatformEventKind> {
    let mut events = vec![PlatformEventKind::DeviceLost {
        provider: provider.to_string(),
    }];
    if recovered {
        events.push(PlatformEventKind::DeviceRestored {
            provider: provider.to_string(),
        });
    }
    events
}

impl WgpuReadback {
    pub fn map_async(
        &self,
        callback: impl FnOnce(Result<(), wgpu::BufferAsyncError>) + Send + 'static,
    ) {
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, callback);
    }
    pub fn finish(self) -> Result<CapturedFrame, PlatformError> {
        let row = usize::try_from(
            self.width
                .checked_mul(4)
                .ok_or_else(|| invalid("surface.capture", "frame row overflows"))?,
        )
        .map_err(|_| invalid("surface.capture", "frame row is too large"))?;
        let padded = usize::try_from(self.padded_row)
            .map_err(|_| invalid("surface.capture", "padded frame row is too large"))?;
        let mapped = self.buffer.slice(..).get_mapped_range();
        let mut rgba8 = Vec::with_capacity(row * self.height as usize);
        for value in mapped.chunks_exact(padded).take(self.height as usize) {
            rgba8.extend_from_slice(&value[..row]);
        }
        drop(mapped);
        self.buffer.unmap();
        Ok(CapturedFrame {
            width: self.width,
            height: self.height,
            rgba8,
        })
    }
}

pub(crate) fn install_device_lost_callback(device: &wgpu::Device, lost: Arc<AtomicBool>) {
    device.set_device_lost_callback(move |_reason, _message| {
        lost.store(true, Ordering::Release);
        tracing::error!(
            event = "platform.wgpu.device.lost",
            "wgpu device loss callback fired"
        );
    });
}

fn upload_frame(device: &wgpu::Device, queue: &wgpu::Queue, frame: &RgbaFrame) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra-platform-frame-upload"),
        size: wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
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
    queue.write_texture(
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
    texture
}

fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::Sampler, wgpu::RenderPipeline) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("astra-platform-frame-layout"),
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
        label: Some("astra-platform-frame-sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra-platform-frame-shader"),
        source: wgpu::ShaderSource::Wgsl(FRAME_SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra-platform-frame-pipeline-layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("astra-platform-frame-pipeline"),
        layout: Some(&pipeline_layout),
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
                format,
                blend: Some(wgpu::BlendState::REPLACE),
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
fn unavailable(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}
fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}
const FRAME_SHADER: &str = r#"@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> { var p = array<vec2<f32>, 3>(vec2<f32>(-1.0,-3.0),vec2<f32>(3.0,1.0),vec2<f32>(-1.0,1.0)); return vec4<f32>(p[index],0.0,1.0); } @group(0) @binding(0) var t:texture_2d<f32>; @group(0) @binding(1) var s:sampler; @fragment fn fs_main(@builtin(position) p:vec4<f32>) -> @location(0) vec4<f32> { return textureSample(t,s,p.xy/vec2<f32>(textureDimensions(t))); }"#;
