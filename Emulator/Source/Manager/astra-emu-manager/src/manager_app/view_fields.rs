use super::*;

impl AstraEmuManagerController {
    pub(super) fn appearance_view(&self) -> AppearanceViewModel {
        AppearanceViewModel {
            theme_dark: self.appearance.theme_dark,
            theme_mode: self.appearance.theme_mode.clone(),
            accent: self.appearance.accent.clone(),
            grid_columns: self.appearance.grid_columns,
            density: self.appearance.density.clone(),
            view_mode: self.appearance.view_mode.clone(),
        }
    }

    pub(super) fn candidate_descriptor(&self, game_id: &str) -> Option<FamilyPluginDescriptor> {
        self.candidates
            .get(game_id)
            .map(|candidate| candidate.descriptor.clone())
    }

    pub(super) fn family_fields(&self) -> Vec<GenericConfigFieldViewModel> {
        let Some(game_id) = self.selected_case_id.as_deref() else {
            return Vec::new();
        };
        if let Some(choices) = self.probe_selection_for_game(game_id) {
            return vec![GenericConfigFieldViewModel {
                key: "family.plugin_id".into(),
                label: "Family provider".into(),
                description:
                    "Multiple providers matched this game. Select one explicitly before launching."
                        .into(),
                kind: "enum".into(),
                value: self
                    .family_options
                    .get("family.plugin_id")
                    .cloned()
                    .unwrap_or_default(),
                enum_values: choices
                    .iter()
                    .map(|choice| choice.report.plugin_id.clone())
                    .collect(),
                required: true,
                min: 0,
                max: 0,
            }];
        }
        let Some(descriptor) = self.candidate_descriptor(game_id) else {
            return Vec::new();
        };
        vec![GenericConfigFieldViewModel {
            key: "family.plugin_id".into(),
            label: "Family provider".into(),
            description: "The provider selected by the probe result.".into(),
            kind: "string".into(),
            value: descriptor.plugin_id,
            enum_values: Vec::new(),
            required: true,
            min: 0,
            max: 0,
        }]
    }

    pub(super) fn input_view(&self) -> InputConfigViewModel {
        let mut view = self.input_config.clone();
        view.gamepad_enabled = self.input_mapping.gamepad_enabled;
        view.gamepad_deadzone = self.input_mapping.deadzone.as_str().into();
        view.gamepad_bindings = GamepadInput::DISPLAY_ORDER
            .iter()
            .map(|button| GamepadBindingViewModel {
                button_id: button.as_str().into(),
                button_label: button.label().into(),
                key_name: self
                    .input_mapping
                    .gamepad
                    .get(button)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        view
    }
}
