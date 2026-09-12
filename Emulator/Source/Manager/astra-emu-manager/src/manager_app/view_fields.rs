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

    pub(super) fn family_fields(&self) -> Result<Vec<GenericConfigFieldViewModel>, String> {
        let Some(game_id) = self.selected_case_id.as_deref() else {
            return Ok(Vec::new());
        };
        if let Some(choices) = self.probe_selection_for_game(game_id) {
            return Ok(vec![GenericConfigFieldViewModel {
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
            }]);
        }
        if self.candidate_descriptor(game_id).is_none() {
            return Ok(Vec::new());
        }
        self.typed_family_fields()
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
