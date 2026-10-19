use super::*;

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

    pub(super) fn family_fields(&self) -> Result<Vec<ConfigFieldViewModel>, String> {
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
        let mut fields = vec![ConfigFieldViewModel {
            key: "manager.emulator_id".into(),
            label: "模拟器".into(),
            description: choices
                .iter()
                .map(|choice| {
                    format!(
                        "{} / {} ({})",
                        choice.descriptor.family_name,
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
            self.append_typed_family_fields(&mut fields)?;
        }
        Ok(fields)
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
