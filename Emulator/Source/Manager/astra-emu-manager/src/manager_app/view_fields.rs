use super::*;

impl AstraEmuManagerController {
    pub(super) fn engine_label(id: &str) -> &str {
        match id {
            "fvp" => "FVP",
            "siglus" => "SiglusEngine",
            "artemis" => "Artemis",
            "musica" => "Musica",
            "softpal" => "SoftPAL",
            "ethornell" => "Ethornell / BGI",
            "krkr" => "KiriKiri",
            _ => id,
        }
    }

    pub(super) fn emulator_label(descriptor: &FamilyPluginDescriptor) -> String {
        let name = match descriptor.plugin_id.as_str() {
            "astra.emu.fvp" => "RFVP",
            "astra.emu.siglus" => "siglus-rs",
            "astra.emu.artemis" => "Art3m1s",
            "astra.emu.musica" => "Astra Musica",
            "sena-native-koikake" => "Sena",
            "ethornell-native" => "ethornell-rs",
            _ => &descriptor.plugin_id,
        };
        format!("{name} · {}", descriptor.version)
    }

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
        let game = self.game(game_id).ok()?;
        self.launch_candidate_for_game(&game)
            .map(|candidate| candidate.descriptor.clone())
    }

    pub(super) fn family_fields(&self) -> Result<Vec<GenericConfigFieldViewModel>, String> {
        let Some(game_id) = self.selected_case_id.as_deref() else {
            return Ok(Vec::new());
        };
        let Some(choices) = self.probe_selection_for_game(game_id) else {
            return Ok(Vec::new());
        };
        let installed = self
            .candidates
            .get(game_id)
            .map(|candidate| candidate.report.plugin_id.as_str());
        let draft = self
            .family_options
            .get("manager.emulator_id")
            .map(String::as_str);
        let mut fields = vec![GenericConfigFieldViewModel {
            key: "manager.emulator_id".into(),
            label: "模拟器".into(),
            description: choices
                .iter()
                .map(|choice| {
                    format!(
                        "{} / {} ({})",
                        Self::engine_label(&choice.report.family_id),
                        Self::emulator_label(&choice.descriptor),
                        choice.report.plugin_id,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n更换模拟器后保存以加载其配置；选择按游戏保存。",
            kind: "enum".into(),
            value: draft.or(installed).unwrap_or_default().into(),
            enum_values: choices
                .iter()
                .map(|choice| choice.report.plugin_id.clone())
                .collect(),
            required: true,
            min: 0,
            max: 0,
        }];
        if installed.is_some() && draft.is_none_or(|id| Some(id) == installed) {
            fields.extend(self.typed_family_fields()?);
            fields.extend(self.musica_import_fields());
        }
        Ok(fields)
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
