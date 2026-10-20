use super::*;
use astra_emu_manager_ui_slint::{ChoiceViewModel, EmulatorDefaultsViewModel};

impl AstraEmuManagerController {
    pub(super) fn engine_label(&self, family_id: &str) -> String {
        self.registry
            .descriptors()
            .chain(
                self.probe_choices
                    .values()
                    .flatten()
                    .map(|choice| &choice.descriptor),
            )
            .find(|descriptor| descriptor.family_id == family_id)
            .map_or_else(|| family_id.to_owned(), |descriptor| descriptor.family_name.clone())
    }

    pub(super) fn emulator_label(descriptor: &FamilyPluginDescriptor) -> String {
        format!("{} · {}", descriptor.display_name, descriptor.version)
    }

    pub(super) fn appearance_view(&self) -> AppearanceViewModel {
        AppearanceViewModel {
            theme_mode: self.appearance.theme_mode.clone(),
            density: self.appearance.density.clone(),
            cover_size: self.appearance.cover_size.clone(),
            view_mode: self.appearance.view_mode.clone(),
            reduce_motion: self.appearance.reduce_motion,
            cover_badges: self.appearance.cover_badges,
        }
    }

    pub(super) fn emulator_choices(&self, game_id: &str) -> Vec<ChoiceViewModel> {
        match self.probe_selection_for_game(game_id) {
            Some(choices) if choices.len() > 1 => choices
                .iter()
                .map(|choice| ChoiceViewModel {
                    id: choice.report.plugin_id.clone(),
                    label: Self::emulator_label(&choice.descriptor),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn input_view(&self) -> InputConfigViewModel {
        let (capture_button, capture_game_id, capture_hint) = self.key_capture_view();
        InputConfigViewModel {
            gamepad_enabled: self.input_mapping.gamepad_enabled,
            deadzone: self.input_mapping.deadzone.as_str().into(),
            bindings: binding_views(&self.input_mapping),
            capture_button,
            capture_game_id,
            capture_hint,
        }
    }

    pub(super) fn emulator_defaults_view(&self) -> Result<EmulatorDefaultsViewModel, String> {
        let Some(plugin) = self
            .defaults_plugin
            .as_deref()
            .filter(|plugin| self.registry.descriptor(plugin).is_some())
        else {
            return Ok(EmulatorDefaultsViewModel::default());
        };
        Ok(EmulatorDefaultsViewModel {
            plugin: plugin.into(),
            fields: self.emulator_default_fields(plugin)?,
            render: self.emulator_render_view(plugin)?,
        })
    }
}

pub(super) fn binding_views(
    mapping: &astra_emu_manager_core::InputMapping,
) -> Vec<GamepadBindingViewModel> {
    GamepadInput::DISPLAY_ORDER
        .iter()
        .map(|button| GamepadBindingViewModel {
            id: button.as_str().into(),
            label: button.label().into(),
            key: mapping.gamepad.get(button).cloned().unwrap_or_default(),
        })
        .collect()
}
