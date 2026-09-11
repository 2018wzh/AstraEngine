use super::*;
use astra_emu_manager::effects::format4::EffectSource;
use astra_emu_manager_core::{FilterPreset, FilterSettings};

impl AstraEmuManagerController {
    pub(super) fn filter_fields(&self) -> Result<Vec<GenericConfigFieldViewModel>, String> {
        let config = &self.filter_settings.configuration;
        let field = |key: &str, label: &str, value: String, description: String| {
            GenericConfigFieldViewModel {
                key: key.into(),
                label: label.into(),
                description,
                kind: "float".into(),
                value: self.filter_options.get(key).cloned().unwrap_or(value),
                enum_values: vec![],
                required: true,
                min: 0,
                max: 0,
            }
        };
        let mut fields = vec![
            field(
                "filter.scale",
                "缩放倍率",
                config.scale.to_string(),
                "1–4；Anime4K 固定为 2".into(),
            ),
            field(
                "filter.strength",
                "锐化强度",
                config.strength.to_string(),
                "0–1".into(),
            ),
        ];
        let source = self
            .filter_options
            .get("filter.source")
            .map(String::as_str)
            .or(self.filter_settings.source.as_deref());
        if let Some(source) = source.filter(|source| !source.is_empty()) {
            let effect = EffectSource::parse(source).map_err(|e| e.to_string())?;
            for parameter in effect.parameters {
                let key = format!("parameter.{}", parameter.name);
                let value = if self.filter_options.contains_key("filter.source") {
                    parameter.default
                } else {
                    config
                        .parameters
                        .get(&parameter.name)
                        .copied()
                        .unwrap_or(parameter.default)
                };
                fields.push(field(
                    &key,
                    &parameter.label,
                    value.to_string(),
                    format!(
                        "{}–{}，步长 {}",
                        parameter.min, parameter.max, parameter.step
                    ),
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

    pub(super) fn pending_filter(&self) -> Result<FilterSettings, String> {
        let mut settings = self.filter_settings.clone();
        if self.filter_options.contains_key("filter.source")
            || self.filter_options.contains_key("filter.preset")
        {
            settings.configuration.parameters.clear();
        }
        for (key, value) in &self.filter_options {
            match key.as_str() {
                "filter.preset" => {
                    settings.configuration.preset = match value.as_str() {
                        "none" => FilterPreset::None,
                        "scale" => FilterPreset::Scale,
                        "sharpen" => FilterPreset::Sharpen,
                        "anime4k" => FilterPreset::Anime4kRestoreUpscale,
                        _ => return Err("ASTRA_EMU_FILTER_PRESET_UNSUPPORTED".into()),
                    };
                }
                "filter.scale" => {
                    settings.configuration.scale =
                        value.parse().map_err(|_| "ASTRA_EMU_FILTER_SCALE")?
                }
                "filter.strength" => {
                    settings.configuration.strength =
                        value.parse().map_err(|_| "ASTRA_EMU_FILTER_STRENGTH")?
                }
                "filter.source" => settings.source = (!value.is_empty()).then(|| value.clone()),
                _ => {
                    let name = key
                        .strip_prefix("parameter.")
                        .ok_or("ASTRA_EMU_FILTER_CONFIG_FIELD_UNKNOWN")?;
                    settings.configuration.parameters.insert(
                        name.into(),
                        value.parse().map_err(|_| "ASTRA_EMU_FILTER_PARAMETER")?,
                    );
                }
            }
        }
        if settings.configuration.preset == FilterPreset::Anime4kRestoreUpscale {
            settings.configuration.scale = 2.0;
        }
        settings.validate()?;
        if let Some(source) = &settings.source {
            let effect = EffectSource::parse(source).map_err(|e| e.to_string())?;
            for (name, value) in &settings.configuration.parameters {
                let parameter = effect
                    .parameters
                    .iter()
                    .find(|p| p.name == *name)
                    .ok_or("ASTRA_EMU_FILTER_PARAMETER_UNKNOWN")?;
                if *value < parameter.min || *value > parameter.max {
                    return Err("ASTRA_EMU_FILTER_PARAMETER_RANGE".into());
                }
            }
        }
        Ok(settings)
    }

    pub(super) fn commit_filter(
        &mut self,
        settings: FilterSettings,
    ) -> Result<ManagerViewModel, String> {
        let previous = std::mem::replace(&mut self.filter_settings, settings);
        let pending = std::mem::take(&mut self.filter_options);
        let result = self.model().and_then(|model| {
            self.library
                .save_filter_settings(&self.filter_settings)
                .map_err(|e| e.to_string())?;
            Ok(model)
        });
        if result.is_err() {
            self.filter_settings = previous;
            self.filter_options = pending;
        }
        result
    }

    pub(super) fn filter_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !matches!(
            key,
            "filter.preset" | "filter.scale" | "filter.strength" | "filter.source"
        ) && !key.starts_with("parameter.")
        {
            return Err("ASTRA_EMU_FILTER_CONFIG_FIELD_UNKNOWN".into());
        }
        if key == "filter.source" && !value.is_empty() {
            if value.len() > 1_048_576 {
                return Err("ASTRA_EMU_FILTER_SOURCE_SIZE".into());
            }
            EffectSource::parse(value).map_err(|e| e.to_string())?;
        }
        if matches!(key, "filter.source" | "filter.preset") {
            self.filter_options
                .retain(|key, _| !key.starts_with("parameter."));
            if key == "filter.preset" {
                self.filter_options
                    .insert("filter.source".into(), String::new());
            }
        }
        self.filter_options.insert(key.into(), value.into());
        Ok(())
    }

    pub(super) fn reset_filter_config(&mut self) -> Result<ManagerViewModel, String> {
        self.filter_options.clear();
        self.model()
    }
}
