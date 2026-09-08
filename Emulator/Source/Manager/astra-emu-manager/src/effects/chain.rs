use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Mutex,
};
use wgpu::util::DeviceExt;

use super::super::{
    compiler::shader_module,
    format4::{EffectSource, SamplerFilter, TextureDimension},
    generator::{generate_pass_shader, uniform_size},
};
use super::{
    FilterConfiguration, FilterEngine, FilterError, FilterPreset, RESTORE_SOURCE, SCALE_SOURCE,
    SHARPEN_SOURCE, UPSCALE_SOURCE,
};

pub(super) struct CompiledChain {
    passes: Vec<CompiledPass>,
    stages: Vec<EffectSource>,
    stage_pass_counts: Vec<usize>,
    preset: FilterPreset,
    config: FilterConfiguration,
    kind: ChainKind,
    output_mode: OutputMode,
    scratch: Mutex<BTreeMap<(String, u32, u32), wgpu::Texture>>,
}
#[derive(Clone, Copy)]
enum ChainKind {
    Single,
    Anime,
}
#[derive(Clone, Copy)]
enum OutputMode {
    Source,
    ConfigScale,
}
struct CompiledPass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    inputs: Vec<String>,
    output: String,
    sampler_filters: Vec<SamplerFilter>,
    block_size: u32,
}

impl CompiledChain {
    pub(super) fn build(
        device: &wgpu::Device,
        dxc_path: &Path,
        config: &FilterConfiguration,
        source_override: Option<&str>,
    ) -> Result<Self, String> {
        let external_source = source_override.is_some();
        let (source_text, kind) = if let Some(source) = source_override {
            (source, ChainKind::Single)
        } else {
            match config.preset {
                FilterPreset::None | FilterPreset::Scale => (SCALE_SOURCE, ChainKind::Single),
                FilterPreset::Sharpen => (SHARPEN_SOURCE, ChainKind::Single),
                FilterPreset::Anime4kRestoreUpscale => (RESTORE_SOURCE, ChainKind::Anime),
            }
        };
        let source = EffectSource::parse(source_text).map_err(|e| e.to_string())?;
        validate_parameter_values(&source, config)?;
        let source_is_upscale = source
            .textures
            .iter()
            .find(|texture| texture.name == "OUTPUT")
            .is_some_and(|texture| {
                matches!(texture.width, Some(TextureDimension::InputTimes(2)))
                    || matches!(texture.height, Some(TextureDimension::InputTimes(2)))
            });
        validate_texture_metadata(&source, source_is_upscale)?;
        let mut sources = vec![(source.clone(), false)];
        if matches!(kind, ChainKind::Anime) {
            let upscale = EffectSource::parse(UPSCALE_SOURCE).map_err(|e| e.to_string())?;
            validate_parameter_values(&upscale, config)?;
            validate_texture_metadata(&upscale, true)?;
            sources.push((upscale, true));
        }
        let mut passes = Vec::new();
        let mut stage_pass_counts = Vec::new();
        for (effect, anime_upscale) in &sources {
            stage_pass_counts.push(effect.passes.len());
            for pass in &effect.passes {
                let rgba8 = !anime_upscale
                    && pass.output == "OUTPUT"
                    && (external_source
                        || !matches!(config.preset, FilterPreset::Anime4kRestoreUpscale))
                    || *anime_upscale && pass.output == "OUTPUT";
                let hlsl = generate_pass_shader(effect, pass, rgba8)?;
                let module = shader_module(
                    device,
                    dxc_path,
                    &format!("astra_emu_effect_pass_{}", pass.number),
                    &hlsl,
                )?;
                let layout = create_layout(device, pass, rgba8, effect)?;
                let pipeline_layout =
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("astra.emu.effect.pipeline-layout"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("astra.emu.effect.pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
                passes.push(CompiledPass {
                    pipeline,
                    layout,
                    inputs: pass.inputs.clone(),
                    output: pass.output.clone(),
                    sampler_filters: effect
                        .samplers
                        .iter()
                        .map(|sampler| sampler.filter)
                        .collect(),
                    block_size: pass.block_size,
                });
            }
        }
        Ok(Self {
            passes,
            stages: sources.into_iter().map(|(effect, _)| effect).collect(),
            stage_pass_counts,
            preset: config.preset,
            config: config.clone(),
            kind,
            output_mode: if external_source || !matches!(config.preset, FilterPreset::Scale) {
                OutputMode::Source
            } else {
                OutputMode::ConfigScale
            },
            scratch: Mutex::new(BTreeMap::new()),
        })
    }
    pub(super) fn output_dimensions(
        &self,
        input_width: u32,
        input_height: u32,
    ) -> Result<(u32, u32), FilterError> {
        if input_width == 0 || input_height == 0 {
            return Err(FilterError::TextureDimensions);
        }
        match self.output_mode {
            OutputMode::ConfigScale => {
                FilterEngine::output_dimensions(input_width, input_height, &self.config)
            }
            OutputMode::Source => texture_dimensions(
                self.stages
                    .last()
                    .ok_or(FilterError::Compile("effect source has no stages".into()))?,
                "OUTPUT",
                input_width,
                input_height,
                None,
            ),
        }
    }
    pub(super) fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        config: &FilterConfiguration,
    ) -> Result<(), FilterError> {
        if matches!(self.kind, ChainKind::Anime)
            && !device
                .features()
                .contains(FilterEngine::required_features())
        {
            return Err(FilterError::CapabilityMissing);
        }
        if config != &self.config || config.preset != self.preset {
            return Err(FilterError::Compile(
                "active filter preset does not match configuration".into(),
            ));
        }
        if input.format() != wgpu::TextureFormat::Rgba8Unorm
            || output.format() != wgpu::TextureFormat::Rgba8Unorm
        {
            return Err(FilterError::TextureFormat);
        }
        if !input.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING)
            || !output
                .usage()
                .contains(wgpu::TextureUsages::STORAGE_BINDING)
        {
            return Err(FilterError::TextureUsage);
        }
        let (ow, oh) = self.output_dimensions(input.width(), input.height())?;
        if output.width() != ow || output.height() != oh {
            return Err(FilterError::TextureDimensions);
        }
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = self.apply_inner(device, queue, input, output, config, (ow, oh));
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(FilterError::GpuValidation(format!(
                "ASTRA_EMU_FILTER_WGPU_APPLY: {error}"
            )));
        }
        result
    }
    fn apply_inner(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        config: &FilterConfiguration,
        output_dimensions: (u32, u32),
    ) -> Result<(), FilterError> {
        let (ow, oh) = output_dimensions;
        let mut scratch = self
            .scratch
            .lock()
            .map_err(|_| FilterError::Compile("scratch resource lock poisoned".into()))?;
        let scratch_keys = self.scratch_keys(input.width(), input.height(), ow, oh)?;
        scratch.retain(|key, _| scratch_keys.contains(key));
        let mut current_input = input.clone();
        let mut pass_offset = 0;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("astra.emu.effect.encoder"),
        });
        let stage_count = self.stage_pass_counts.len();
        for stage in 0..stage_count {
            let _effect = &self.stages[stage];
            let pass_count = self.stage_pass_counts[stage];
            let mut names = BTreeMap::<String, wgpu::TextureView>::new();
            names.insert(
                "INPUT".into(),
                current_input.create_view(&wgpu::TextureViewDescriptor::default()),
            );
            for index in 0..pass_count {
                let pass = &self.passes[pass_offset + index];
                let is_last = index + 1 == pass_count;
                let final_target = !matches!(self.kind, ChainKind::Anime) || stage == 1;
                let (width, height) = texture_dimensions(
                    &self.stages[stage],
                    &pass.output,
                    current_input.width(),
                    current_input.height(),
                    if is_last && final_target {
                        Some((ow, oh))
                    } else {
                        None
                    },
                )?;
                let view = if is_last && final_target {
                    output.create_view(&wgpu::TextureViewDescriptor::default())
                } else {
                    let key = (pass.output.clone(), width, height);
                    let texture = scratch.entry(key).or_insert_with(|| {
                        device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("astra.emu.effect.intermediate"),
                            size: wgpu::Extent3d {
                                width,
                                height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba16Float,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::STORAGE_BINDING,
                            view_formats: &[],
                        })
                    });
                    texture.create_view(&wgpu::TextureViewDescriptor::default())
                };
                names.insert(pass.output.clone(), view);
            }
            for index in 0..pass_count {
                let pass = &self.passes[pass_offset + index];
                let is_last = index + 1 == pass_count;
                let (width, height) = texture_dimensions(
                    &self.stages[stage],
                    &pass.output,
                    current_input.width(),
                    current_input.height(),
                    if is_last && (!matches!(self.kind, ChainKind::Anime) || stage == 1) {
                        Some((ow, oh))
                    } else {
                        None
                    },
                )?;
                let output_view = names
                    .get(&pass.output)
                    .ok_or(FilterError::Compile("output view missing".into()))?;
                let mut entries = Vec::new();
                let mut uniform = vec![0_u8; uniform_size(&self.stages[stage])];
                write_u32(&mut uniform, 0, current_input.width());
                write_u32(&mut uniform, 4, current_input.height());
                write_u32(&mut uniform, 8, width);
                write_u32(&mut uniform, 12, height);
                write_f32(&mut uniform, 16, 1.0 / current_input.width() as f32);
                write_f32(&mut uniform, 20, 1.0 / current_input.height() as f32);
                write_f32(&mut uniform, 24, 1.0 / width as f32);
                write_f32(&mut uniform, 28, 1.0 / height as f32);
                write_f32(
                    &mut uniform,
                    32,
                    width as f32 / current_input.width() as f32,
                );
                write_f32(
                    &mut uniform,
                    36,
                    height as f32 / current_input.height() as f32,
                );
                write_f32(&mut uniform, 40, config.strength);
                for (index, parameter) in self.stages[stage].parameters.iter().enumerate() {
                    let value = config
                        .parameters
                        .get(&parameter.name)
                        .copied()
                        .unwrap_or(parameter.default);
                    if !value.is_finite() || !(parameter.min..=parameter.max).contains(&value) {
                        return Err(FilterError::Parameter("effect"));
                    }
                    write_f32(&mut uniform, 48 + index * 4, value);
                }
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("astra.emu.effect.params"),
                    contents: &uniform,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                entries.push(wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                });
                for (slot, name) in pass.inputs.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 1 + slot as u32,
                        resource: wgpu::BindingResource::TextureView(
                            names
                                .get(name)
                                .ok_or(FilterError::Compile("input view missing".into()))?,
                        ),
                    });
                }
                let output_binding = 1 + pass.inputs.len() as u32;
                entries.push(wgpu::BindGroupEntry {
                    binding: output_binding,
                    resource: wgpu::BindingResource::TextureView(output_view),
                });
                if pass.sampler_filters.is_empty() {
                    return Err(FilterError::Compile("sampler missing".into()));
                }
                let samplers = pass
                    .sampler_filters
                    .iter()
                    .map(|sampler_filter| {
                        let filter = match sampler_filter {
                            SamplerFilter::Linear => wgpu::FilterMode::Linear,
                            SamplerFilter::Point => wgpu::FilterMode::Nearest,
                        };
                        device.create_sampler(&wgpu::SamplerDescriptor {
                            label: Some("astra.emu.effect.sampler"),
                            mag_filter: filter,
                            min_filter: filter,
                            address_mode_u: wgpu::AddressMode::ClampToEdge,
                            address_mode_v: wgpu::AddressMode::ClampToEdge,
                            ..Default::default()
                        })
                    })
                    .collect::<Vec<_>>();
                for (index, sampler) in samplers.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry {
                        binding: output_binding + 1 + index as u32,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    });
                }
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("astra.emu.effect.bind-group"),
                    layout: &pass.layout,
                    entries: &entries,
                });
                let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("astra.emu.effect.pass"),
                    timestamp_writes: None,
                });
                compute.set_pipeline(&pass.pipeline);
                compute.set_bind_group(0, &bind, &[]);
                compute.dispatch_workgroups(
                    width.div_ceil(pass.block_size),
                    height.div_ceil(pass.block_size),
                    1,
                );
            }
            pass_offset += pass_count;
            if !matches!(self.kind, ChainKind::Anime) {
                break;
            }
            if stage == 0 {
                current_input = scratch
                    .get(&(
                        "OUTPUT".to_owned(),
                        current_input.width(),
                        current_input.height(),
                    ))
                    .cloned()
                    .ok_or(FilterError::Compile("restore output missing".into()))?;
            }
        }
        queue.submit([encoder.finish()]);
        Ok(())
    }

    fn scratch_keys(
        &self,
        input_width: u32,
        input_height: u32,
        ow: u32,
        oh: u32,
    ) -> Result<BTreeSet<(String, u32, u32)>, FilterError> {
        let mut keys = BTreeSet::new();
        let mut stage_input = (input_width, input_height);
        for (stage, effect) in self.stages.iter().enumerate() {
            let pass_count = self.stage_pass_counts[stage];
            for (index, pass) in self.passes[self.stage_pass_counts[..stage].iter().sum::<usize>()
                ..self.stage_pass_counts[..=stage].iter().sum::<usize>()]
                .iter()
                .enumerate()
            {
                let is_last = index + 1 == pass_count;
                let final_target = !matches!(self.kind, ChainKind::Anime) || stage == 1;
                let dimensions = texture_dimensions(
                    effect,
                    &pass.output,
                    stage_input.0,
                    stage_input.1,
                    if is_last && final_target {
                        Some((ow, oh))
                    } else {
                        None
                    },
                )?;
                if !(is_last && final_target) {
                    keys.insert((pass.output.clone(), dimensions.0, dimensions.1));
                }
                if is_last && matches!(self.kind, ChainKind::Anime) && stage == 0 {
                    stage_input = dimensions;
                }
            }
        }
        Ok(keys)
    }
}

fn create_layout(
    device: &wgpu::Device,
    pass: &super::super::format4::Pass,
    rgba8: bool,
    effect: &EffectSource,
) -> Result<wgpu::BindGroupLayout, String> {
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: Some(std::num::NonZeroU64::new(uniform_size(effect) as u64).unwrap()),
        },
        count: None,
    }];
    for index in 0..pass.inputs.len() {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1 + index as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float {
                    filterable: effect
                        .samplers
                        .iter()
                        .any(|sampler| sampler.filter == SamplerFilter::Linear),
                },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 1 + pass.inputs.len() as u32,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::ReadWrite,
            format: if rgba8 {
                wgpu::TextureFormat::Rgba8Unorm
            } else {
                wgpu::TextureFormat::Rgba16Float
            },
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    });
    for (index, sampler) in effect.samplers.iter().enumerate() {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 + pass.inputs.len() as u32 + index as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Sampler(if sampler.filter == SamplerFilter::Linear {
                wgpu::SamplerBindingType::Filtering
            } else {
                wgpu::SamplerBindingType::NonFiltering
            }),
            count: None,
        });
    }
    Ok(
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra.emu.effect.layout"),
            entries: &entries,
        }),
    )
}

fn texture_dimensions(
    effect: &EffectSource,
    name: &str,
    input_width: u32,
    input_height: u32,
    final_dimensions: Option<(u32, u32)>,
) -> Result<(u32, u32), FilterError> {
    let Some(texture) = effect.textures.iter().find(|texture| texture.name == name) else {
        return Ok(final_dimensions.unwrap_or((input_width, input_height)));
    };
    if texture.width.is_none() && texture.height.is_none() {
        return Ok(final_dimensions.unwrap_or((input_width, input_height)));
    }
    let dimension = |value: Option<TextureDimension>, input: u32| -> Result<u32, FilterError> {
        match value.unwrap_or(TextureDimension::Input) {
            TextureDimension::Input => Ok(input),
            TextureDimension::InputTimes(factor) => input
                .checked_mul(factor)
                .filter(|value| *value > 0)
                .ok_or(FilterError::TextureDimensions),
        }
    };
    Ok((
        dimension(texture.width, input_width)?,
        dimension(texture.height, input_height)?,
    ))
}

fn validate_parameter_values(
    effect: &EffectSource,
    config: &FilterConfiguration,
) -> Result<(), String> {
    for (name, value) in &config.parameters {
        let Some(parameter) = effect
            .parameters
            .iter()
            .find(|parameter| parameter.name == *name)
        else {
            return Err(format!("ASTRA_EMU_EFFECT_PARAMETER_UNKNOWN_{name}"));
        };
        if !value.is_finite() || !(parameter.min..=parameter.max).contains(value) {
            return Err(format!("ASTRA_EMU_EFFECT_PARAMETER_OUT_OF_RANGE_{name}"));
        }
    }
    Ok(())
}

fn validate_texture_metadata(effect: &EffectSource, upscale: bool) -> Result<(), String> {
    for texture in &effect.textures {
        if let Some(format) = &texture.format {
            if format != "R16G16B16A16_FLOAT" {
                return Err(format!(
                    "ASTRA_EMU_EFFECT_TEXTURE_FORMAT_UNSUPPORTED_{}",
                    texture.name
                ));
            }
        }
        let expected_factor = if upscale && texture.name == "OUTPUT" {
            2
        } else {
            1
        };
        for dimension in [texture.width, texture.height].into_iter().flatten() {
            let factor = match dimension {
                TextureDimension::Input => 1,
                TextureDimension::InputTimes(factor) => factor,
            };
            if factor != expected_factor {
                return Err(format!(
                    "ASTRA_EMU_EFFECT_TEXTURE_DIMENSION_UNSUPPORTED_{}",
                    texture.name
                ));
            }
        }
    }
    Ok(())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
    write_u32(bytes, offset, value.to_bits());
}
