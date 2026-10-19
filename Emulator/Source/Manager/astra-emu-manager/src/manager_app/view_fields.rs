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

    pub(super) fn core_display_name(descriptor: &FamilyPluginDescriptor) -> String {
        match descriptor.plugin_id.as_str() {
            "astra.emu.fvp" => "RFVP".into(),
            "astra.emu.siglus" => "siglus-rs".into(),
            "astra.emu.artemis" => "Art3m1s".into(),
            "astra.emu.musica" => "Astra Musica".into(),
            "sena-native-koikake" => "Sena".into(),
            "ethornell-native" => "ethornell-rs".into(),
            _ => descriptor.plugin_id.clone(),
        }
    }

    pub(super) fn emulator_label(descriptor: &FamilyPluginDescriptor) -> String {
        format!("{} · {}", Self::core_display_name(descriptor), descriptor.version)
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

    pub(super) fn family_fields(
        &self,
    ) -> Result<
        (
            Vec<ConfigFieldViewModel>,
            Vec<ConfigFieldViewModel>,
        ),
        String,
    > {
        let Some(game_id) = self.selected_case_id.as_deref() else {
            return Ok((Vec::new(), Vec::new()));
        };
        let Some(choices) = self.probe_selection_for_game(game_id) else {
            return Ok((Vec::new(), Vec::new()));
        };
        let installed = self
            .candidates
            .get(game_id)
            .map(|candidate| candidate.report.plugin_id.as_str());
        let draft = self
            .family_options
            .get("manager.emulator_id")
            .map(String::as_str);
        let mut fields = vec![ConfigFieldViewModel {
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
        let mut common = Vec::new();
        if installed.is_some() && draft.is_none_or(|id| Some(id) == installed) {
            self.append_typed_family_fields(&mut common, &mut fields)?;
        }
        Ok((common, fields))
    }

    pub(super) fn input_view(&self) -> InputConfigViewModel {
        InputConfigViewModel {
            gamepad_enabled: self.input_mapping.gamepad_enabled,
            deadzone: self.input_mapping.deadzone.as_str().into(),
            bindings: GamepadInput::DISPLAY_ORDER
                .iter()
                .map(|button| GamepadBindingViewModel {
                    id: button.as_str().into(),
                    label: button.label().into(),
                    key: self
                        .input_mapping
                        .gamepad
                        .get(button)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect(),
        }
    }
}
