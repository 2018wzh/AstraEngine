use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const RESTORE_SOURCE: &str =
    include_str!("../../../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl");
const UPSCALE_SOURCE: &str =
    include_str!("../../../../../Assets/Effects/Anime4K/upscale_cnn_x2_s.hlsl");
const SCALE_SOURCE: &str = include_str!("../../../../../Assets/Effects/Builtin/scale.hlsl");
const SHARPEN_SOURCE: &str = include_str!("../../../../../Assets/Effects/Builtin/sharpen.hlsl");

pub const MAGPIE_REVISION: &str = "3396e1e000bbab050d098032dac04ba0250683ad";
pub const DXC_VERSION: &str = "1.8.2502";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterPreset {
    None,
    Scale,
    Sharpen,
    Anime4kRestoreUpscale,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterConfiguration {
    pub preset: FilterPreset,
    pub scale: f32,
    pub strength: f32,
    pub parameters: BTreeMap<String, f32>,
}

impl Default for FilterConfiguration {
    fn default() -> Self {
        Self {
            preset: FilterPreset::None,
            scale: 1.0,
            strength: 0.35,
            parameters: BTreeMap::new(),
        }
    }
}

impl FilterConfiguration {
    pub fn validate(&self) -> Result<(), FilterError> {
        if !self.scale.is_finite() || !(1.0..=4.0).contains(&self.scale) {
            return Err(FilterError::Parameter("scale"));
        }
        if !self.strength.is_finite() || !(0.0..=1.0).contains(&self.strength) {
            return Err(FilterError::Parameter("strength"));
        }
        if matches!(self.preset, FilterPreset::Anime4kRestoreUpscale) && self.scale != 2.0 {
            return Err(FilterError::AnimeScale);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("ASTRA_EMU_FILTER_PARAMETER_{0}")]
    Parameter(&'static str),
    #[error("ASTRA_EMU_FILTER_ANIME_SCALE_MUST_BE_TWO")]
    AnimeScale,
    #[error("ASTRA_EMU_FILTER_FORMAT4: {0}")]
    Format4(String),
    #[error("ASTRA_EMU_FILTER_DXC_PATH_MISSING")]
    DxcPathMissing,
    #[error("ASTRA_EMU_FILTER_DXC_VERSION_MISMATCH")]
    DxcVersionMismatch,
    #[error("ASTRA_EMU_FILTER_CAPABILITY_MISSING")]
    CapabilityMissing,
    #[error("ASTRA_EMU_FILTER_TEXTURE_FORMAT")]
    TextureFormat,
    #[error("ASTRA_EMU_FILTER_TEXTURE_USAGE")]
    TextureUsage,
    #[error("ASTRA_EMU_FILTER_TEXTURE_DIMENSIONS")]
    TextureDimensions,
    #[error("ASTRA_EMU_FILTER_WGPU_VALIDATION: {0}")]
    GpuValidation(String),
    #[error("ASTRA_EMU_FILTER_COMPILE: {0}")]
    Compile(String),
}

pub struct FilterEngine {
    dxc_path: PathBuf,
    active: Option<CompiledChain>,
}

impl FilterEngine {
    pub fn new(dxc_path: impl Into<PathBuf>) -> Self {
        Self {
            dxc_path: dxc_path.into(),
            active: None,
        }
    }
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
    }
    pub fn output_dimensions(
        input_width: u32,
        input_height: u32,
        config: &FilterConfiguration,
    ) -> Result<(u32, u32), FilterError> {
        config.validate()?;
        let factor = if matches!(config.preset, FilterPreset::Anime4kRestoreUpscale) {
            2.0
        } else if matches!(config.preset, FilterPreset::Scale) {
            config.scale
        } else {
            1.0
        };
        let w = ((input_width as f32) * factor).round();
        let h = ((input_height as f32) * factor).round();
        if !(1.0..=(u32::MAX as f32)).contains(&w) || !(1.0..=(u32::MAX as f32)).contains(&h) {
            return Err(FilterError::TextureDimensions);
        }
        Ok((w as u32, h as u32))
    }
    /// Returns the dimensions produced by the currently compiled effect.
    ///
    /// External format-4 sources are allowed to declare their own output
    /// dimensions, so callers that are about to allocate a destination must
    /// query the active chain rather than infer dimensions from the preset.
    pub fn active_output_dimensions(
        &self,
        input_width: u32,
        input_height: u32,
    ) -> Result<(u32, u32), FilterError> {
        if input_width == 0 || input_height == 0 {
            return Err(FilterError::TextureDimensions);
        }
        self.active
            .as_ref()
            .map(|chain| chain.output_dimensions(input_width, input_height))
            .unwrap_or(Ok((input_width, input_height)))
    }
    pub fn reload(
        &mut self,
        device: &wgpu::Device,
        config: &FilterConfiguration,
    ) -> Result<(), FilterError> {
        self.reload_with_source(device, config, None)
    }

    pub fn reload_source(
        &mut self,
        device: &wgpu::Device,
        source: &str,
        config: &FilterConfiguration,
    ) -> Result<(), FilterError> {
        self.reload_with_source(device, config, Some(source))
    }

    fn reload_with_source(
        &mut self,
        device: &wgpu::Device,
        config: &FilterConfiguration,
        source_override: Option<&str>,
    ) -> Result<(), FilterError> {
        config.validate()?;
        if matches!(config.preset, FilterPreset::None) {
            self.active = None;
            return Ok(());
        }
        verify_dxc_path(&self.dxc_path)?;
        if matches!(config.preset, FilterPreset::Anime4kRestoreUpscale)
            && !device.features().contains(Self::required_features())
        {
            return Err(FilterError::CapabilityMissing);
        }
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let new_chain = CompiledChain::build(device, &self.dxc_path, config, source_override)
            .map_err(FilterError::Compile)?;
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(FilterError::Compile(format!(
                "ASTRA_EMU_FILTER_WGPU_BUILD: {error}"
            )));
        }
        self.active = Some(new_chain);
        Ok(())
    }
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        config: &FilterConfiguration,
    ) -> Result<(), FilterError> {
        config.validate()?;
        if matches!(config.preset, FilterPreset::None) {
            if input.format() != wgpu::TextureFormat::Rgba8Unorm
                || output.format() != wgpu::TextureFormat::Rgba8Unorm
            {
                return Err(FilterError::TextureFormat);
            }
            if input.width() != output.width() || input.height() != output.height() {
                return Err(FilterError::TextureDimensions);
            }
            if !input.usage().contains(wgpu::TextureUsages::COPY_SRC)
                || !output.usage().contains(wgpu::TextureUsages::COPY_DST)
            {
                return Err(FilterError::TextureUsage);
            }
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra.emu.effect.none-encoder"),
            });
            encoder.copy_texture_to_texture(
                input.as_image_copy(),
                output.as_image_copy(),
                wgpu::Extent3d {
                    width: input.width(),
                    height: input.height(),
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            return Ok(());
        }
        let chain = self
            .active
            .as_ref()
            .ok_or(FilterError::Compile("filter is not loaded".into()))?;
        chain.apply(device, queue, input, output, config)
    }
}

fn verify_dxc_path(path: &Path) -> Result<(), FilterError> {
    if !path.is_file()
        || !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("dxcompiler.dll"))
    {
        return Err(FilterError::DxcPathMissing);
    }
    Ok(())
}

#[path = "chain.rs"]
mod chain;

use chain::CompiledChain;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod engine_tests;
