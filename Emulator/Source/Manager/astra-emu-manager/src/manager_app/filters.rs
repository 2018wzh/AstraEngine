use super::*;
use astra_emu_manager::effects::format4::EffectSource;
use astra_emu_manager_core::{FilterConfiguration, FilterPreset, FilterSettings};

const MAX_FILTER_SOURCE_BYTES: usize = 1_048_576;

use astra_emu_manager::FilterApply;

impl AstraEmuManagerController {
    pub(super) fn filter_fields(&self) -> Result<Vec<ConfigFieldViewModel>, String> {
        let config = &self.filter_settings.configuration;
        let field = |key: &str, label: &str, value: f32, range: (f32, f32, f32)| {
            ConfigFieldViewModel {
                key: key.into(),
                label: label.into(),
                kind: "float".into(),
                value: value.to_string(),
                display_value: value.to_string(),
                min: range.0.into(),
                max: range.1.into(),
                step: range.2.into(),
                ..ConfigFieldViewModel::default()
            }
        };
        let mut fields = vec![field("filter.scale", "缩放倍率", config.scale, (1.0, 4.0, 0.25))];
        fields[0].description = "Anime4K 固定为 2 倍".into();
        fields.push(field(
            "filter.strength",
            "锐化强度",
            config.strength,
            (0.0, 1.0, 0.05),
        ));
        if let Some(source) = &self.filter_settings.source {
            let effect = EffectSource::parse(source).map_err(|e| e.to_string())?;
            for parameter in effect.parameters {
                let value = config
                    .parameters
                    .get(&parameter.name)
                    .copied()
                    .unwrap_or(parameter.default);
                fields.push(field(
                    &format!("parameter.{}", parameter.name),
                    &parameter.label,
                    value,
                    (parameter.min, parameter.max, parameter.step),
                ));
            }
        }
        Ok(fields)
    }

    pub(super) fn filter_preset_id(&self) -> &'static str {
        if self.filter_settings.source.is_some() {
            return "external";
        }
        match self.filter_settings.configuration.preset {
            FilterPreset::None => "none",
            FilterPreset::Scale => "scale",
            FilterPreset::Sharpen => "sharpen",
            FilterPreset::Anime4kRestoreUpscale => "anime4k",
        }
    }

    /// Apply a candidate to the renderer, persist it, and restore the previous
    /// renderer state if either step fails.
    fn update_filter(
        &mut self,
        change: impl FnOnce(&mut FilterSettings) -> Result<(), String>,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        let mut candidate = self.filter_settings.clone();
        change(&mut candidate)?;
        if candidate.configuration.preset == FilterPreset::Anime4kRestoreUpscale {
            candidate.configuration.scale = 2.0;
        }
        validate_filter(&candidate)?;
        let result = apply(&candidate).and_then(|()| {
            self.library
                .save_filter_settings(&candidate)
                .map_err(|error| error.to_string())
        });
        if let Err(error) = result {
            return Err(match apply(&self.filter_settings) {
                Ok(()) => error,
                Err(rollback) => format!("{error}; {rollback}"),
            });
        }
        self.filter_settings = candidate;
        self.model()
    }

    pub(super) fn set_filter_preset(
        &mut self,
        id: &str,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        let preset = match id {
            "none" => FilterPreset::None,
            "scale" => FilterPreset::Scale,
            "sharpen" => FilterPreset::Sharpen,
            "anime4k" => FilterPreset::Anime4kRestoreUpscale,
            _ => return Err("ASTRA_EMU_FILTER_PRESET_UNSUPPORTED".into()),
        };
        self.update_filter(
            |settings| {
                if settings.configuration.preset == FilterPreset::Anime4kRestoreUpscale {
                    settings.configuration.scale = FilterConfiguration::default().scale;
                }
                settings.configuration.preset = preset;
                settings.configuration.parameters.clear();
                settings.source = None;
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn set_filter_parameter(
        &mut self,
        key: &str,
        value: &str,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        let value: f32 = value
            .trim()
            .parse()
            .map_err(|_| "ASTRA_EMU_FILTER_PARAMETER")?;
        self.update_filter(
            |settings| {
                match key {
                    "filter.scale" => settings.configuration.scale = value,
                    "filter.strength" => settings.configuration.strength = value,
                    _ => {
                        let name = key
                            .strip_prefix("parameter.")
                            .ok_or("ASTRA_EMU_FILTER_CONFIG_FIELD_UNKNOWN")?;
                        settings.configuration.parameters.insert(name.into(), value);
                    }
                }
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn reset_filter_parameters(
        &mut self,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        self.update_filter(
            |settings| {
                let defaults = FilterConfiguration::default();
                settings.configuration.scale = defaults.scale;
                settings.configuration.strength = defaults.strength;
                settings.configuration.parameters.clear();
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn load_filter_source(
        &mut self,
        path: &Path,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        use std::io::Read;
        let file = fs::File::open(path).map_err(|_| "ASTRA_EMU_FILTER_SOURCE_OPEN")?;
        let mut bytes = Vec::new();
        file.take(MAX_FILTER_SOURCE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "ASTRA_EMU_FILTER_SOURCE_READ")?;
        if bytes.len() > MAX_FILTER_SOURCE_BYTES {
            return Err("ASTRA_EMU_FILTER_SOURCE_SIZE".into());
        }
        let source = String::from_utf8(bytes).map_err(|_| "ASTRA_EMU_FILTER_SOURCE_UTF8")?;
        self.update_filter(
            |settings| {
                settings.configuration.parameters.clear();
                settings.source = Some(source);
                Ok(())
            },
            apply,
        )
    }
}

fn validate_filter(settings: &FilterSettings) -> Result<(), String> {
    settings.validate()?;
    let Some(source) = &settings.source else {
        return Ok(());
    };
    let effect = EffectSource::parse(source).map_err(|e| e.to_string())?;
    for (name, value) in &settings.configuration.parameters {
        let parameter = effect
            .parameters
            .iter()
            .find(|parameter| parameter.name == *name)
            .ok_or("ASTRA_EMU_FILTER_PARAMETER_UNKNOWN")?;
        if *value < parameter.min || *value > parameter.max {
            return Err("ASTRA_EMU_FILTER_PARAMETER_RANGE".into());
        }
    }
    Ok(())
}
