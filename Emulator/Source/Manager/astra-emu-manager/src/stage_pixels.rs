use astra_emu_manager_core::model::{FrameAlpha, FrameFormat, FrameInfo, FrameRowOrder};

pub(super) fn input_format(format: FrameFormat) -> wgpu::TextureFormat {
    match format {
        FrameFormat::Rgba8Srgb { .. } | FrameFormat::Rgba8Linear { .. } => {
            wgpu::TextureFormat::Rgba8Unorm
        }
        FrameFormat::Bgra8Srgb { .. } | FrameFormat::Bgra8Linear { .. } => {
            wgpu::TextureFormat::Bgra8Unorm
        }
    }
}

fn flags(info: FrameInfo) -> u32 {
    u32::from(info.rows == FrameRowOrder::BottomToTop)
        | (u32::from(info.format.alpha() == FrameAlpha::Ignored) << 1)
        | (u32::from(matches!(
            info.format,
            FrameFormat::Rgba8Linear { .. } | FrameFormat::Bgra8Linear { .. }
        )) << 2)
}

#[derive(Default)]
pub(super) struct PixelPresentation {
    resources: Option<Resources>,
}

struct Resources {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    flags: Option<u32>,
    output: Option<(wgpu::Texture, wgpu::TextureView)>,
    input: Option<wgpu::Texture>,
    binding: Option<wgpu::BindGroup>,
}

impl PixelPresentation {
    pub(super) fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        info: FrameInfo,
    ) -> wgpu::Texture {
        let flags = flags(info);
        if flags == 0 && input.format() == wgpu::TextureFormat::Rgba8Unorm {
            return input.clone();
        }
        let resources = self.resources.get_or_insert_with(|| Resources::new(device));
        if resources.flags != Some(flags) {
            queue.write_buffer(&resources.uniform, 0, &flags.to_le_bytes());
            resources.flags = Some(flags);
        }
        if resources
            .output
            .as_ref()
            .is_none_or(|(texture, _)| texture.size() != input.size())
        {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("astra-emu-native-pixel-output"),
                size: input.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            resources.output = Some((texture, view));
        }
        if resources.input.as_ref() != Some(input) {
            let view = input.create_view(&Default::default());
            resources.binding = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("astra-emu-native-pixel-binding"),
                layout: &resources.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: resources.uniform.as_entire_binding(),
                    },
                ],
            }));
            resources.input = Some(input.clone());
        }
        let (texture, view) = resources.output.as_ref().expect("pixel output prepared");
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra-emu-native-pixel-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&resources.pipeline);
            pass.set_bind_group(
                0,
                resources.binding.as_ref().expect("pixel input bound"),
                &[],
            );
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        texture.clone()
    }
}

impl Resources {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra-emu-native-pixel-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("astra-emu-native-pixels"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var<uniform> options: vec4<u32>;
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4<f32>(p[index], 0.0, 1.0);
}
@fragment fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    var coordinate = vec2<i32>(position.xy);
    if (options.x & 1u) != 0u {
        coordinate.y = i32(textureDimensions(image).y) - 1 - coordinate.y;
    }
    var pixel = textureLoad(image, coordinate, 0);
    if (options.x & 4u) != 0u {
        pixel = vec4<f32>(select(
            pixel.rgb * 12.92,
            1.055 * pow(pixel.rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055),
            pixel.rgb > vec3<f32>(0.0031308)
        ), pixel.a);
    }
    if (options.x & 2u) != 0u { pixel.a = 1.0; }
    return pixel;
}
"#
                .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("astra-emu-native-pixel-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("astra-emu-native-pixel-pipeline"),
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
                    format: wgpu::TextureFormat::Rgba8Unorm,
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("astra-emu-native-pixel-options"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline,
            uniform,
            flags: None,
            output: None,
            input: None,
            binding: None,
        }
    }
}
