use super::*;

impl WgpuPresentationCore {
    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(Ordering::Acquire)
    }

    #[cfg(feature = "platform-test-driver")]
    pub fn inject_device_loss_for_test(&self) {
        self.test_device_loss.store(true, Ordering::Release);
        self.device_lost.store(true, Ordering::Release);
    }

    pub(super) fn ensure_device_available(
        &self,
        operation: &'static str,
    ) -> Result<(), PlatformError> {
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

    pub(super) fn present_texture(
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
        let Some(surface) = self.surface.as_ref() else {
            return Ok(());
        };
        let output = match surface.get_current_texture() {
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
            rgba8: rgba8.into(),
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

pub(super) fn upload_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &RgbaFrame,
) -> wgpu::Texture {
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

pub(super) fn pipeline(
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

pub(super) fn unavailable(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}

pub(super) fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

pub(super) const FRAME_SHADER: &str = r#"@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> { var p = array<vec2<f32>, 3>(vec2<f32>(-1.0,-3.0),vec2<f32>(3.0,1.0),vec2<f32>(-1.0,1.0)); return vec4<f32>(p[index],0.0,1.0); } @group(0) @binding(0) var t:texture_2d<f32>; @group(0) @binding(1) var s:sampler; @fragment fn fs_main(@builtin(position) p:vec4<f32>) -> @location(0) vec4<f32> { return textureSample(t,s,p.xy/vec2<f32>(textureDimensions(t))); }"#;
