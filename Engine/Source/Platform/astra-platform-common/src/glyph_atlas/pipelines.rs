use super::*;
use astra_media_core::SceneCompositing2D;
use astra_platform::{PlatformError, PlatformErrorCode};

pub(crate) fn create_pipelines(
    device: &wgpu::Device,
) -> (
    wgpu::BindGroupLayout,
    wgpu::Sampler,
    wgpu::Sampler,
    BlendPipelines,
) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("astra-glyph-atlas-layout"),
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
        label: Some("astra-glyph-atlas-sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("astra-glyph-atlas-nearest-sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("astra-glyph-atlas-shader"),
        source: wgpu::ShaderSource::Wgsl(GLYPH_SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("astra-glyph-atlas-pipeline-layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipelines = BlendPipelines {
        linear: create_blend_pipeline_set(
            device,
            &pipeline_layout,
            &shader,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "linear",
        ),
        encoded: create_blend_pipeline_set(
            device,
            &pipeline_layout,
            &shader,
            wgpu::TextureFormat::Rgba8Unorm,
            "encoded",
        ),
    };
    (layout, linear_sampler, nearest_sampler, pipelines)
}

pub(crate) fn create_blend_pipeline_set(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
    compositing: &str,
) -> BlendPipelineSet {
    let pipeline = |blend: wgpu::BlendState, blend_name: &str, fragment: &str| {
        let label = format!("astra-glyph-atlas-{compositing}-{blend_name}-pipeline");
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Uint32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    BlendPipelineSet {
        alpha: pipeline(
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            "alpha",
            "fs_main",
        ),
        opaque: pipeline(wgpu::BlendState::REPLACE, "opaque", "fs_opaque"),
        add: pipeline(
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
            "add",
            "fs_main",
        ),
        multiply: pipeline(
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
            "multiply",
            "fs_main",
        ),
        screen: pipeline(
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
            "screen",
            "fs_main",
        ),
    }
}

pub(crate) fn scene_target_format(compositing: SceneCompositing2D) -> wgpu::TextureFormat {
    match compositing {
        SceneCompositing2D::LinearSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        SceneCompositing2D::EncodedSrgb => wgpu::TextureFormat::Rgba8Unorm,
    }
}

pub(crate) fn invalid(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "surface.present_scene",
        message,
    )
}

pub(crate) const GLYPH_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) pixel_mask: vec2<u32>,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) pixel_mask: vec2<u32>,
};
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    output.pixel_mask = input.pixel_mask;
    return output;
}
fn masked_sample(input: VertexOutput) -> vec4<f32> {
    let sample = textureSample(atlas, atlas_sampler, input.uv);
    let pixel = vec2<u32>(input.position.xy) % vec2<u32>(8u);
    let bit = 63u - (pixel.y * 8u + pixel.x);
    if ((input.pixel_mask[bit / 32u] >> (bit % 32u)) & 1u) == 0u {
        discard;
    }
    return sample;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sample = masked_sample(input);
    let alpha = sample.a * input.color.a;
    return vec4<f32>(sample.rgb * input.color.rgb * sample.a, alpha);
}
@fragment fn fs_opaque(input: VertexOutput) -> @location(0) vec4<f32> {
    let sample = masked_sample(input);
    return vec4<f32>(sample.rgb * input.color.rgb, sample.a * input.color.a);
}
"#;
