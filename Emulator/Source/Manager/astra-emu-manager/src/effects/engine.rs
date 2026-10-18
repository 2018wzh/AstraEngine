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
    #[error("ASTRA_EMU_FILTER_OUTPUT_MISSING")]
    OutputMissing,
    #[error("ASTRA_EMU_FILTER_WGPU_VALIDATION: {0}")]
    GpuValidation(String),
    #[error("ASTRA_EMU_FILTER_COMPILE: {0}")]
    Compile(String),
}

/// Validated effect metadata, without loading DXC or creating GPU resources.
pub struct PreparedFilter {
    configuration: FilterConfiguration,
    chain: Option<PreparedChain>,
}

impl PreparedFilter {
    pub fn new(config: &FilterConfiguration, source: Option<&str>) -> Result<Self, FilterError> {
        config.validate()?;
        let chain = if config.preset == FilterPreset::None {
            None
        } else {
            Some(PreparedChain::new(config, source).map_err(FilterError::Compile)?)
        };
        Ok(Self {
            configuration: config.clone(),
            chain,
        })
    }

    pub fn configuration(&self) -> &FilterConfiguration {
        &self.configuration
    }

    pub fn output_dimensions(
        &self,
        input_width: u32,
        input_height: u32,
    ) -> Result<(u32, u32), FilterError> {
        if input_width == 0 || input_height == 0 {
            return Err(FilterError::TextureDimensions);
        }
        self.chain
            .as_ref()
            .map_or(Ok((input_width, input_height)), |chain| {
                chain.output_dimensions(input_width, input_height, &self.configuration)
            })
    }
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

    /// Compile a prepared request transactionally; failures keep the active chain.
    pub fn reload_prepared(
        &mut self,
        device: &wgpu::Device,
        prepared: PreparedFilter,
    ) -> Result<(), FilterError> {
        let PreparedFilter {
            configuration,
            chain,
        } = prepared;
        let Some(chain) = chain else {
            self.active = None;
            return Ok(());
        };
        verify_dxc_path(&self.dxc_path)?;
        if matches!(configuration.preset, FilterPreset::Anime4kRestoreUpscale)
            && !device.features().contains(Self::required_features())
        {
            return Err(FilterError::CapabilityMissing);
        }
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = CompiledChain::build(device, &self.dxc_path, configuration, chain)
            .map_err(FilterError::Compile);
        let validation_error = pollster::block_on(error_scope.pop());
        let new_chain = result?;
        if let Some(error) = validation_error {
            return Err(FilterError::Compile(format!(
                "ASTRA_EMU_FILTER_WGPU_BUILD: {error}"
            )));
        }
        self.active = Some(new_chain);
        Ok(())
    }
    /// Return the texture to present. None aliases input without encoding,
    /// submitting or requiring an unused destination texture.
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Texture,
        output: Option<&wgpu::Texture>,
        config: &FilterConfiguration,
    ) -> Result<wgpu::Texture, FilterError> {
        config.validate()?;
        if matches!(config.preset, FilterPreset::None) {
            return Ok(input.clone());
        }
        let output = output.ok_or(FilterError::OutputMissing)?;
        let chain = self
            .active
            .as_ref()
            .ok_or(FilterError::Compile("filter is not loaded".into()))?;
        chain.apply(device, queue, input, output, config)?;
        Ok(output.clone())
    }
}

fn verify_dxc_path(path: &Path) -> Result<(), FilterError> {
    let expected_name = format!(
        "{}dxcompiler.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    );
    if !path.is_file()
        || !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(&expected_name))
    {
        return Err(FilterError::DxcPathMissing);
    }
    Ok(())
}

#[path = "chain.rs"]
mod chain;

use chain::{CompiledChain, PreparedChain};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_dimensions_follow_the_selected_source() {
        let scale = FilterConfiguration {
            preset: FilterPreset::Scale,
            scale: 1.5,
            ..Default::default()
        };
        assert_eq!(
            PreparedFilter::new(&scale, None)
                .unwrap()
                .output_dimensions(721, 1280)
                .unwrap(),
            (1082, 1920)
        );
        assert_eq!(
            PreparedFilter::new(&scale, Some(SCALE_SOURCE))
                .unwrap()
                .output_dimensions(721, 1280)
                .unwrap(),
            (721, 1280)
        );
        assert_eq!(
            PreparedFilter::new(&scale, Some(UPSCALE_SOURCE))
                .unwrap()
                .output_dimensions(721, 1280)
                .unwrap(),
            (1442, 2560)
        );
        let anime = FilterConfiguration {
            preset: FilterPreset::Anime4kRestoreUpscale,
            scale: 2.0,
            ..Default::default()
        };
        assert_eq!(
            PreparedFilter::new(&anime, Some(SCALE_SOURCE))
                .unwrap()
                .output_dimensions(721, 1280)
                .unwrap(),
            (721, 1280)
        );
    }

    #[test]
    fn prepared_none_ignores_unused_source_and_scale() {
        let config = FilterConfiguration {
            scale: 4.0,
            ..Default::default()
        };
        let prepared = PreparedFilter::new(&config, Some("not an effect")).unwrap();
        assert_eq!(prepared.output_dimensions(1280, 720).unwrap(), (1280, 720));
        assert!(matches!(
            prepared.output_dimensions(0, 720),
            Err(FilterError::TextureDimensions)
        ));
    }

    #[test]
    fn preparation_rejects_invalid_source_metadata_and_parameters() {
        let mut config = FilterConfiguration {
            preset: FilterPreset::Scale,
            ..Default::default()
        };
        assert!(PreparedFilter::new(&config, Some("not an effect")).is_err());
        let invalid_format = UPSCALE_SOURCE.replace("R16G16B16A16_FLOAT", "R32_FLOAT");
        let error = PreparedFilter::new(&config, Some(&invalid_format))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("ASTRA_EMU_EFFECT_TEXTURE_FORMAT_UNSUPPORTED"));
        config.parameters.insert("unknown".into(), 1.0);
        let error = PreparedFilter::new(&config, None)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("ASTRA_EMU_EFFECT_PARAMETER_UNKNOWN_unknown"));
    }
}
