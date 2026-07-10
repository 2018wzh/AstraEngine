use astra_platform::{CapturedFrame, PlatformError, PlatformErrorCode, RgbaFrame};

/// Shared hardware-only WGPU presentation path. Backends own event loops and
/// surface creation; this core owns adapter policy, RGBA upload, presentation,
/// resize, loss classification, and padded readback layout.
pub struct WgpuPresentationCore {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    last_upload: Option<UploadFrame>,
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
                apply_limit_buckets: false,
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
        Ok(Self {
            _instance: instance,
            surface,
            _adapter: adapter,
            device,
            queue,
            config,
            layout,
            sampler,
            pipeline,
            last_upload: None,
        })
    }

    pub fn present(&mut self, frame: RgbaFrame) -> Result<(), PlatformError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(invalid(
                "surface.present_rgba",
                "frame dimensions must be non-zero",
            ));
        }
        if frame.width != self.config.width || frame.height != self.config.height {
            self.config.width = frame.width;
            self.config.height = frame.height;
            self.surface.configure(&self.device, &self.config);
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
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
        self.queue.write_texture(
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
        self.queue.present(output);
        self.last_upload = Some(UploadFrame {
            texture,
            width: frame.width,
            height: frame.height,
        });
        Ok(())
    }

    pub fn begin_capture(&self) -> Result<WgpuReadback, PlatformError> {
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

    pub fn poll(&self, poll_type: wgpu::PollType) -> Result<(), PlatformError> {
        self.device
            .poll(poll_type)
            .map(|_| ())
            .map_err(|_| unavailable("surface.capture", "GPU readback poll failed"))
    }
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
        let mapped = self
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|_| unavailable("surface.capture", "GPU readback range is unavailable"))?;
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
