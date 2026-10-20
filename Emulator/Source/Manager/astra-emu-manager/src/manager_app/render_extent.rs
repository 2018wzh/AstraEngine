use super::*;
use astra_emu_manager_core::{
    GameSettings, RenderPreference, RenderSupport, SurfaceRecord, RENDER_PRESET_HEIGHTS,
};
use astra_emu_manager_ui_slint::{ChoiceViewModel, RenderViewModel};

const RENDER_UNSUPPORTED: &str = "ASTRA_EMU_RENDER_EXTENT_UNSUPPORTED";

pub(super) fn render_default_key(plugin_id: &str) -> String {
    format!("render.default.{plugin_id}")
}

fn aspect(support: Option<&RenderSupport>, logical_hint: Option<(u32, u32)>) -> f64 {
    logical_hint
        .or_else(|| support.and_then(|support| support.default))
        .filter(|&(width, height)| width > 0 && height > 0)
        .map_or(16.0 / 9.0, |(width, height)| {
            f64::from(width) / f64::from(height)
        })
}

fn scaled(width: f64, height: f64, scale: f64) -> (u32, u32) {
    (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    )
}

fn fit_into(support: &RenderSupport, (width, height): (u32, u32)) -> (u32, u32) {
    let (w, h) = (f64::from(width), f64::from(height));
    let (width, height) = if width > support.max.0 || height > support.max.1 {
        let scale = (f64::from(support.max.0) / w).min(f64::from(support.max.1) / h);
        scaled(w, h, scale)
    } else if width < support.min.0 || height < support.min.1 {
        let scale = (f64::from(support.min.0) / w).max(f64::from(support.min.1) / h);
        scaled(w, h, scale)
    } else {
        (width, height)
    };
    (
        width.clamp(support.min.0, support.max.0),
        height.clamp(support.min.1, support.max.1),
    )
}

pub(super) fn resolve_render_extent(
    support: Option<&RenderSupport>,
    pref: &RenderPreference,
    window: (u32, u32),
    logical_hint: Option<(u32, u32)>,
) -> Result<Option<(u32, u32)>, String> {
    let Some(support) = support else {
        return Ok(None);
    };
    let ratio = aspect(Some(support), logical_hint);
    let extent = match *pref {
        RenderPreference::Default => return Ok(None),
        RenderPreference::Custom { width, height } => {
            return if support.contains((width, height)) {
                Ok(Some((width, height)))
            } else {
                Err(RENDER_UNSUPPORTED.into())
            };
        }
        RenderPreference::Preset { height } => {
            ((f64::from(height) * ratio).round().max(1.0) as u32, height)
        }
        RenderPreference::Adaptive => {
            let (width, height) = (f64::from(window.0), f64::from(window.1));
            if width / height > ratio {
                ((height * ratio).floor().max(1.0) as u32, window.1.max(1))
            } else {
                (window.0.max(1), (width / ratio).floor().max(1.0) as u32)
            }
        }
    };
    Ok(Some(fit_into(support, extent)))
}

fn validate_for(support: Option<&RenderSupport>, pref: &RenderPreference) -> Result<(), String> {
    pref.validate()?;
    if let RenderPreference::Custom { width, height } = *pref {
        if !support.is_some_and(|support| support.contains((width, height))) {
            return Err(RENDER_UNSUPPORTED.into());
        }
    }
    Ok(())
}

fn render_mode(pref: Option<&RenderPreference>) -> String {
    match pref {
        None | Some(RenderPreference::Default) => "default".into(),
        Some(RenderPreference::Adaptive) => "adaptive".into(),
        Some(RenderPreference::Preset { height }) => format!("preset-{height}"),
        Some(RenderPreference::Custom { .. }) => "custom".into(),
    }
}

/// `default` means "follow the emulator default" in game scope and "core native" in emulator scope.
pub(super) fn render_view(
    support: Option<&RenderSupport>,
    pref: Option<&RenderPreference>,
    last_surface: Option<&SurfaceRecord>,
    game_scope: bool,
) -> RenderViewModel {
    let note = last_surface
        .map(|surface| {
            format!(
                "上次实际生效 {}×{}",
                surface.raster_width, surface.raster_height
            )
        })
        .unwrap_or_default();
    let Some(support) = support else {
        return RenderViewModel {
            note,
            ..RenderViewModel::default()
        };
    };
    let ratio = aspect(
        Some(support),
        last_surface.map(|surface| (surface.logical_width, surface.logical_height)),
    );
    let choice = |id: String, label: String| ChoiceViewModel { id, label };
    let mut options = vec![
        choice(
            "default".into(),
            if game_scope {
                "默认（跟随模拟器设置）"
            } else {
                "默认（核心原生）"
            }
            .into(),
        ),
        choice("adaptive".into(), "适应窗口".into()),
    ];
    for height in RENDER_PRESET_HEIGHTS {
        let width = (f64::from(height) * ratio).round() as u32;
        if width <= support.max.0 && height <= support.max.1 {
            options.push(choice(
                format!("preset-{height}"),
                format!("{height}p · {width}×{height}"),
            ));
        }
    }
    options.push(choice("custom".into(), "自定义…".into()));
    let (custom_width, custom_height) = match pref {
        Some(RenderPreference::Custom { width, height }) => (*width as i32, *height as i32),
        _ => (0, 0),
    };
    RenderViewModel {
        supported: true,
        options,
        mode: render_mode(pref),
        custom_width,
        custom_height,
        min: format!("{}×{}", support.min.0, support.min.1),
        max: format!("{}×{}", support.max.0, support.max.1),
        note,
        error: String::new(),
    }
}

impl AstraEmuManagerController {
    pub(super) fn emulator_render_default(
        &self,
        plugin_id: &str,
    ) -> Result<Option<RenderPreference>, String> {
        self.library
            .app_state::<RenderPreference>(&render_default_key(plugin_id))
            .map_err(|error| error.to_string())
    }

    /// Game preference, then the emulator default, then the core's native size.
    pub(super) fn effective_render_preference(
        &self,
        plugin_id: &str,
        settings: &GameSettings,
    ) -> Result<RenderPreference, String> {
        match settings.render {
            Some(pref) => Ok(pref),
            None => Ok(self
                .emulator_render_default(plugin_id)?
                .unwrap_or(RenderPreference::Default)),
        }
    }

    pub(super) fn set_render_preference(
        &mut self,
        game_id: &str,
        pref: Option<RenderPreference>,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        if let Some(pref) = &pref {
            match self.candidates.get(game_id) {
                Some(candidate) => validate_for(candidate.descriptor.render.as_ref(), pref)?,
                None => pref.validate()?,
            }
        }
        let mut settings = self
            .library
            .game_settings(game_id)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        settings.render = pref;
        self.library
            .set_game_settings(game_id, &settings)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn set_emulator_render_default(
        &mut self,
        plugin_id: &str,
        pref: RenderPreference,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let descriptor = self
            .registry
            .descriptor(plugin_id)
            .ok_or("ASTRA_EMU_FAMILY_PROVIDER_MISSING")?;
        validate_for(descriptor.render.as_ref(), &pref)?;
        self.library
            .set_app_state(&render_default_key(plugin_id), &pref)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn game_render_view(&self, game_id: &str) -> Result<RenderViewModel, String> {
        let settings = self
            .library
            .game_settings(game_id)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let support = self
            .candidates
            .get(game_id)
            .and_then(|candidate| candidate.descriptor.render.as_ref());
        Ok(render_view(
            support,
            settings.render.as_ref(),
            settings.last_surface.as_ref(),
            true,
        ))
    }

    pub(super) fn emulator_render_view(&self, plugin_id: &str) -> Result<RenderViewModel, String> {
        let support = self
            .registry
            .descriptor(plugin_id)
            .and_then(|descriptor| descriptor.render.as_ref());
        Ok(render_view(
            support,
            self.emulator_render_default(plugin_id)?.as_ref(),
            None,
            false,
        ))
    }

    pub(super) fn record_last_surface(
        &mut self,
        game_id: &str,
        surface: SurfaceRecord,
    ) -> Result<(), String> {
        let mut settings = self
            .library
            .game_settings(game_id)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        settings.last_surface = Some(surface);
        self.library
            .set_game_settings(game_id, &settings)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn support(min: (u32, u32), max: (u32, u32), preserve_aspect: bool) -> RenderSupport {
        RenderSupport {
            min,
            max,
            default: None,
            preserve_aspect,
        }
    }

    #[test]
    fn preset_uses_the_logical_aspect() {
        let support = support((320, 180), (3840, 2160), true);
        assert_eq!(
            resolve_render_extent(
                Some(&support),
                &RenderPreference::Preset { height: 1080 },
                (1280, 720),
                Some((1280, 720)),
            ),
            Ok(Some((1920, 1080)))
        );
    }

    #[test]
    fn adaptive_fits_the_largest_extent_inside_the_window() {
        let support = support((320, 180), (16384, 16384), false);
        assert_eq!(
            resolve_render_extent(
                Some(&support),
                &RenderPreference::Adaptive,
                (2560, 1600),
                Some((800, 600)),
            ),
            Ok(Some((2133, 1600)))
        );
    }

    #[test]
    fn preset_above_the_maximum_is_scaled_proportionally() {
        let support = support((320, 180), (1920, 1080), false);
        assert_eq!(
            resolve_render_extent(
                Some(&support),
                &RenderPreference::Preset { height: 2160 },
                (1280, 720),
                None,
            ),
            Ok(Some((1920, 1080)))
        );
    }

    #[test]
    fn custom_outside_support_is_rejected() {
        let support = support((320, 180), (3840, 2160), false);
        assert_eq!(
            resolve_render_extent(
                Some(&support),
                &RenderPreference::Custom {
                    width: 5000,
                    height: 5000,
                },
                (1280, 720),
                None,
            ),
            Err("ASTRA_EMU_RENDER_EXTENT_UNSUPPORTED".to_owned())
        );
    }

    #[test]
    fn default_or_unsupported_core_requests_no_extent() {
        let support = support((320, 180), (3840, 2160), false);
        assert_eq!(
            resolve_render_extent(
                Some(&support),
                &RenderPreference::Default,
                (1280, 720),
                None
            ),
            Ok(None)
        );
        assert_eq!(
            resolve_render_extent(
                None,
                &RenderPreference::Preset { height: 1080 },
                (1280, 720),
                None
            ),
            Ok(None)
        );
    }

    #[test]
    fn view_lists_presets_within_the_maximum() {
        let support = support((320, 180), (1920, 1080), false);
        let view = render_view(
            Some(&support),
            Some(&RenderPreference::Custom {
                width: 1600,
                height: 900,
            }),
            Some(&SurfaceRecord {
                logical_width: 1280,
                logical_height: 720,
                raster_width: 1600,
                raster_height: 900,
            }),
            true,
        );
        assert_eq!(
            view.options
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            ["default", "adaptive", "preset-720", "preset-1080", "custom"]
        );
        assert_eq!(view.options[3].label, "1080p · 1920×1080");
        assert_eq!(view.mode, "custom");
        assert_eq!((view.custom_width, view.custom_height), (1600, 900));
        assert_eq!(view.range, "320×180 – 1920×1080");
        assert_eq!(view.note, "上次实际生效 1600×900");
    }
}
