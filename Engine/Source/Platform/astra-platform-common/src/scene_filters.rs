use std::collections::HashMap;

use astra_media_core::{FilterNode, FilterParam, FilterValidator, SceneCommand};
use astra_platform::{PlatformError, PlatformErrorCode, SceneFrame};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Bloom,
    Fade,
    ColorMatrix,
}

impl Kind {
    fn parameters(node: &FilterNode) -> Result<(Self, [f32; 4]), PlatformError> {
        let value = |name: &str| match node.params.get(name) {
            Some(FilterParam::Float(value)) if value.is_finite() => Ok(*value),
            _ => Err(invalid("validated filter parameter is unavailable")),
        };
        Ok(match node.kind.as_str() {
            "astra.filter.bloom" => (Self::Bloom, [value("intensity")?, 0.0, 0.0, 0.0]),
            "astra.filter.fade" => (Self::Fade, [value("amount")?, 0.0, 0.0, 0.0]),
            "astra.filter.color_matrix" => (
                Self::ColorMatrix,
                [value("r")?, value("g")?, value("b")?, value("a")?],
            ),
            _ => return Err(invalid("GPU filter kind is unsupported")),
        })
    }

    fn expression(self) -> &'static str {
        match self {
            Self::Bloom => {
                "vec4<f32>(min(source.rgb + vec3<f32>(params.x), vec3<f32>(1.0)), source.a)"
            }
            Self::Fade => "vec4<f32>(source.rgb * params.x, source.a)",
            Self::ColorMatrix => "source * params",
        }
    }
}

struct Stage {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    input: wgpu::Texture,
    binding: wgpu::BindGroup,
    parameters: Option<[f32; 4]>,
}

/// Device-owned filter resources, shared by surface and offscreen rendering.
#[derive(Default)]
pub(crate) struct SceneFilters {
    layout: Option<wgpu::BindGroupLayout>,
    pipelines: HashMap<(Kind, wgpu::TextureFormat), wgpu::RenderPipeline>,
    stages: Vec<Stage>,
    draw_calls: u64,
}

impl SceneFilters {
    pub(crate) fn validate(frame: &SceneFrame) -> Result<usize, PlatformError> {
        let mut saw_filter = false;
        let mut count = 0;
        for command in &frame.commands {
            match command {
                SceneCommand::FilterGraph { graph } => {
                    saw_filter = true;
                    if !FilterValidator
                        .validate(graph)
                        .blocking_diagnostics()
                        .is_empty()
                    {
                        return Err(invalid("filter graph validation failed"));
                    }
                    count += graph.nodes.len();
                }
                _ if saw_filter => {
                    return Err(invalid(
                        "draw commands after a filter graph are not supported",
                    ))
                }
                _ => {}
            }
        }
        Ok(count)
    }

    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        input: &wgpu::Texture,
        timestamps: Option<(&wgpu::QuerySet, u32)>,
    ) -> Result<wgpu::Texture, PlatformError> {
        let mut texture = input.clone();
        self.draw_calls = 0;
        for command in &frame.commands {
            let SceneCommand::FilterGraph { graph } = command else {
                continue;
            };
            for node in &graph.nodes {
                texture = self.apply(device, queue, &texture, node, timestamps)?;
                self.draw_calls += 1;
            }
        }
        self.stages.truncate(self.draw_calls as usize);
        Ok(texture)
    }

    fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        node: &FilterNode,
        timestamps: Option<(&wgpu::QuerySet, u32)>,
    ) -> Result<wgpu::Texture, PlatformError> {
        let (kind, parameters) = Kind::parameters(node)?;
        let format = input.format();
        let layout = self.layout.get_or_insert_with(|| create_layout(device));
        let pipeline = self
            .pipelines
            .entry((kind, format))
            .or_insert_with(|| create_pipeline(device, layout, kind, format));
        let index = self.draw_calls as usize;
        if self.stages.get(index).is_none_or(|stage| {
            stage.texture.size() != input.size() || stage.texture.format() != format
        }) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("astra-scene-filter-output"),
                size: input.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("astra-scene-filter-parameters"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let stage = Stage {
                view: texture.create_view(&Default::default()),
                texture,
                binding: create_binding(device, layout, input, &uniform),
                uniform,
                input: input.clone(),
                parameters: None,
            };
            if index == self.stages.len() {
                self.stages.push(stage);
            } else {
                self.stages[index] = stage;
            }
        }
        let stage = &mut self.stages[index];
        if stage.input != *input {
            stage.binding = create_binding(device, layout, input, &stage.uniform);
            stage.input = input.clone();
        }
        if stage.parameters != Some(parameters) {
            let mut bytes = [0; 16];
            for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(parameters) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            queue.write_buffer(&stage.uniform, 0, &bytes);
            stage.parameters = Some(parameters);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("astra-scene-filter-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &stage.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamps.map(|(query_set, start)| {
                    let begin = start + index as u32 * 2;
                    wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: Some(begin),
                        end_of_pass_write_index: Some(begin + 1),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &stage.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        Ok(stage.texture.clone())
    }

    pub(crate) fn resource_bytes(&self) -> u64 {
        self.stages
            .iter()
            .map(|stage| {
                u64::from(stage.texture.width()) * u64::from(stage.texture.height()) * 4 + 16
            })
            .sum()
    }

    pub(crate) fn draw_calls(&self) -> u64 {
        self.draw_calls
    }
    pub(crate) fn pipeline_count(&self) -> u64 {
        self.pipelines.len() as u64
    }
}

fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("astra-scene-filter-layout"),
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
    })
}

fn create_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    input: &wgpu::Texture,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("astra-scene-filter-binding"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &input.create_view(&Default::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    kind: Kind,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra-scene-filter"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "@group(0) @binding(0) var input_texture: texture_2d<f32>;\n\
             @group(0) @binding(1) var<uniform> params: vec4<f32>;\n\
             @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{\n\
               let p = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));\n\
               return vec4<f32>(p[i], 0.0, 1.0);\n\
             }}\n\
             @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{\n\
               let source = textureLoad(input_texture, vec2<i32>(p.xy), 0);\n\
               return {};\n\
             }}",
                kind.expression()
            )
            .into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra-scene-filter-pipeline-layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("astra-scene-filter-pipeline"),
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
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn invalid(message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, "scene.filter", message)
}
