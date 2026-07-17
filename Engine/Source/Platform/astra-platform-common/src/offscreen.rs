use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

use astra_headless_protocol::RendererExecutionIdentity;
use astra_media_core::{FilterParam, FilterValidator, SceneCommand};
use astra_platform::{CapturedFrame, PlatformError, PlatformErrorCode, SceneFrame};

use crate::{glyph_atlas::WgpuGlyphAtlasRenderer, presentation::install_device_lost_callback};

/// Surface-free WGPU owner shared by the native platform hosts and Headless.
pub struct WgpuOffscreenRenderer {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    scene_renderer: WgpuGlyphAtlasRenderer,
    identity: RendererExecutionIdentity,
    device_lost: Arc<AtomicBool>,
}

impl WgpuOffscreenRenderer {
    pub async fn new() -> Result<Self, PlatformError> {
        let instance = native_wgpu_instance()?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| unavailable("offscreen.adapter", "hardware GPU adapter is unavailable"))?;
        let info = adapter.get_info();
        if !matches!(
            info.device_type,
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
        ) {
            return Err(unavailable(
                "offscreen.adapter",
                "only integrated or discrete hardware adapters are allowed for --gpu",
            ));
        }
        validate_native_backend(info.backend)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| {
                tracing::error!(
                    event = "platform.offscreen.device_create.failed",
                    backend = backend_name(info.backend),
                    device_type = device_type_name(info.device_type),
                    diagnostic = %error,
                    "offscreen GPU device creation failed"
                );
                unavailable("offscreen.device", "GPU device creation failed")
            })?;
        let identity = RendererExecutionIdentity {
            provider: "wgpu_offscreen".into(),
            backend: backend_name(info.backend).into(),
            device_type: device_type_name(info.device_type).into(),
            vendor_id: info.vendor,
            device_id: info.device,
            adapter_name_hash: hash(info.name.as_bytes()),
            driver_identity_hash: hash(format!("{}:{}", info.driver, info.driver_info).as_bytes()),
        };
        identity
            .validate()
            .map_err(|_| unavailable("offscreen.identity", "GPU identity is invalid"))?;
        let scene_renderer = WgpuGlyphAtlasRenderer::new(&device);
        let device_lost = Arc::new(AtomicBool::new(false));
        install_device_lost_callback(&device, Arc::clone(&device_lost));
        Ok(Self {
            _instance: instance,
            device,
            queue,
            scene_renderer,
            identity,
            device_lost,
        })
    }

    pub fn identity(&self) -> &RendererExecutionIdentity {
        &self.identity
    }

    pub fn render(&mut self, frame: &SceneFrame) -> Result<CapturedFrame, PlatformError> {
        if self.device_lost.load(Ordering::Acquire) {
            return Err(PlatformError::new(
                PlatformErrorCode::DeviceLost,
                "offscreen.render",
                "GPU device is lost",
            ));
        }
        let mut base = frame.clone();
        let mut commands = Vec::with_capacity(base.commands.len());
        let mut graphs = Vec::new();
        let mut saw_filter = false;
        for command in std::mem::take(&mut base.commands) {
            match command {
                SceneCommand::FilterGraph { graph } => {
                    saw_filter = true;
                    let validation = FilterValidator.validate(&graph);
                    if !validation.blocking_diagnostics().is_empty() {
                        return Err(invalid(
                            "offscreen.filter",
                            "filter graph validation failed",
                        ));
                    }
                    graphs.push(graph);
                }
                _ if saw_filter => {
                    return Err(invalid(
                        "offscreen.filter",
                        "draw commands after a filter graph are not supported",
                    ));
                }
                command => commands.push(command),
            }
        }
        base.commands = commands;
        self.scene_renderer.reset_resources();
        let prepared = self
            .scene_renderer
            .render(&self.device, &self.queue, &base)?;
        let mut texture = self.scene_renderer.commit(prepared);
        for graph in graphs {
            for node in graph.nodes {
                texture = apply_filter(
                    &self.device,
                    &self.queue,
                    &texture,
                    frame.width,
                    frame.height,
                    &node.kind,
                    &node.params,
                )?;
            }
        }
        readback(
            &self.device,
            &self.queue,
            &texture,
            frame.width,
            frame.height,
        )
    }
}

fn apply_filter(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input: &wgpu::Texture,
    width: u32,
    height: u32,
    kind: &str,
    params: &BTreeMap<String, FilterParam>,
) -> Result<wgpu::Texture, PlatformError> {
    let expression = match kind {
        "astra.filter.bloom" => format!(
            "vec4<f32>(min(source.rgb + vec3<f32>({}), vec3<f32>(1.0)), source.a)",
            float_param(params, "intensity")?
        ),
        "astra.filter.fade" => format!(
            "vec4<f32>(source.rgb * {}, source.a)",
            float_param(params, "amount")?
        ),
        "astra.filter.color_matrix" => format!(
            "source * vec4<f32>({}, {}, {}, {})",
            float_param(params, "r")?,
            float_param(params, "g")?,
            float_param(params, "b")?,
            float_param(params, "a")?
        ),
        _ => {
            return Err(invalid(
                "offscreen.filter",
                "GPU filter kind is unsupported",
            ))
        }
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra-offscreen-filter"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "@group(0) @binding(0) var input_texture: texture_2d<f32>;\n\
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{\n\
                   let p = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));\n\
                   return vec4<f32>(p[i], 0.0, 1.0);\n\
                 }}\n\
                 @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{\n\
                   let source = textureLoad(input_texture, vec2<i32>(p.xy), 0);\n\
                   return {expression};\n\
                 }}"
            )
            .into(),
        ),
    });
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("astra-offscreen-filter-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra-offscreen-filter-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("astra-offscreen-filter-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra-offscreen-filter-output"),
        size: wgpu::Extent3d {
            width,
            height,
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
    });
    let input_view = input.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("astra-offscreen-filter-bind-group"),
        layout: &bind_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&input_view),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("astra-offscreen-filter-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
    Ok(output)
}

fn float_param(
    params: &BTreeMap<String, FilterParam>,
    name: &str,
) -> Result<String, PlatformError> {
    match params.get(name) {
        Some(FilterParam::Float(value)) if value.is_finite() => Ok(format!("{value:.9}")),
        _ => Err(invalid(
            "offscreen.filter",
            "validated filter parameter is unavailable",
        )),
    }
}

/// Reuses the backend policy of the corresponding native platform host.
pub fn native_wgpu_instance() -> Result<wgpu::Instance, PlatformError> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    #[cfg(target_os = "windows")]
    {
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::StaticDxc;
    }
    #[cfg(target_os = "linux")]
    {
        descriptor.backends = wgpu::Backends::VULKAN;
    }
    #[cfg(target_os = "macos")]
    {
        descriptor.backends = wgpu::Backends::METAL;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        return Err(unavailable(
            "offscreen.platform",
            "offscreen GPU is implemented only for Windows, Linux, and macOS",
        ));
    }
    Ok(wgpu::Instance::new(descriptor))
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<CapturedFrame, PlatformError> {
    let row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("offscreen.readback", "frame row overflows"))?;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("astra-offscreen-readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|_| unavailable("offscreen.readback", "GPU readback poll failed"))?;
    rx.recv()
        .map_err(|_| unavailable("offscreen.readback", "GPU readback callback was lost"))?
        .map_err(|_| unavailable("offscreen.readback", "GPU readback mapping failed"))?;
    let mapped = slice.get_mapped_range();
    let mut rgba8 = Vec::with_capacity(row as usize * height as usize);
    for source in mapped.chunks_exact(padded as usize).take(height as usize) {
        rgba8.extend_from_slice(&source[..row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(CapturedFrame {
        width,
        height,
        rgba8,
    })
}

fn validate_native_backend(backend: wgpu::Backend) -> Result<(), PlatformError> {
    let valid = cfg!(target_os = "windows") && backend == wgpu::Backend::Dx12
        || cfg!(target_os = "linux") && backend == wgpu::Backend::Vulkan
        || cfg!(target_os = "macos") && backend == wgpu::Backend::Metal;
    if valid {
        Ok(())
    } else {
        Err(unavailable(
            "offscreen.backend",
            "adapter backend does not match the native platform policy",
        ))
    }
}

fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        _ => "unsupported",
    }
}

fn device_type_name(device: wgpu::DeviceType) -> &'static str {
    match device {
        wgpu::DeviceType::DiscreteGpu => "discrete_gpu",
        wgpu::DeviceType::IntegratedGpu => "integrated_gpu",
        wgpu::DeviceType::VirtualGpu => "virtual_gpu",
        wgpu::DeviceType::Cpu => "cpu",
        _ => "other",
    }
}

fn hash(bytes: &[u8]) -> String {
    astra_core::Hash256::from_sha256(bytes).to_string()
}

fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

fn unavailable(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}
