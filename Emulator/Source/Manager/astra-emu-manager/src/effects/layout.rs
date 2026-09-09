use super::super::super::{
    format4::{EffectSource, Pass, SamplerFilter, TextureDimension},
    generator::uniform_size,
};
use super::super::{FilterConfiguration, FilterError};

pub(super) fn create_layout(
    device: &wgpu::Device,
    pass: &Pass,
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

pub(super) fn texture_dimensions(
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

pub(super) fn validate_parameter_values(
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

pub(super) fn validate_texture_metadata(
    effect: &EffectSource,
    upscale: bool,
) -> Result<(), String> {
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

pub(super) fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
    write_u32(bytes, offset, value.to_bits());
}
