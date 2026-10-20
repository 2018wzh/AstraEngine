use super::*;

impl ManagerController for AstraEmuManagerController {
    fn prepare_library(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::prepare_library(self)
    }

    fn cancel_prepare(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::cancel_prepare(self)
    }

    fn remove_from_library(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::remove_from_library(self, game_id)
    }

    fn restore_game(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::restore_game(self, game_id)
    }

    fn snackbar_action(&mut self) -> Result<astra_emu_manager::SnackbarOutcome, String> {
        AstraEmuManagerController::snackbar_action(self)
    }

    fn begin_key_capture(
        &mut self,
        button: &str,
        game_id: Option<&str>,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::begin_key_capture(self, button, game_id)
    }

    fn cancel_key_capture(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::cancel_key_capture(self)
    }

    fn key_capture_active(&self) -> bool {
        AstraEmuManagerController::key_capture_active(self)
    }

    fn capture_key(&mut self, code: KeyCode) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::capture_key(self, code)
    }

    fn take_modal_update(&mut self) -> Option<astra_emu_manager::modal_service::ModalUiUpdate> {
        self.registry.take_modal_update()
    }

    fn respond_modal(
        &mut self,
        id: astra_emu_manager::modal_service::ModalId,
        response: astra_emu_manager_core::model::ModalResponse,
    ) -> Result<(), String> {
        self.registry.respond_modal(id, response)
    }
    fn take_window_request(
        &mut self,
    ) -> Option<astra_emu_manager::window_service::WindowPresentation> {
        self.registry.take_window_request()
    }
    fn respond_window(
        &mut self,
        request: &astra_emu_manager::window_service::WindowPresentation,
        result: Result<astra_emu_manager::window_service::WindowOutcome, String>,
    ) -> Result<(), String> {
        if let Ok(outcome) = &result {
            if let Some(active) = self
                .active
                .as_mut()
                .filter(|active| active.epoch == request.session_epoch)
            {
                if let astra_emu_manager::window_service::WindowCommand::Title(title) =
                    &request.command
                {
                    active.title = Some(title.clone());
                }
                if outcome.valid_fields & astra_emu_abi::WINDOW_RESULT_MODE != 0 {
                    active.fullscreen = outcome.fullscreen != astra_emu_abi::FULLSCREEN_WINDOWED;
                }
            }
        }
        self.registry.respond_window(request, result)
    }
    fn native_failure(&self) -> Option<(&str, bool)> {
        self.active
            .as_ref()
            .filter(|active| active.closing.is_none())
            .and_then(|active| {
                active
                    .failure
                    .as_ref()
                    .map(|failure| (failure.message.as_str(), failure.can_retry))
            })
    }
    fn retry_native(&mut self) -> Result<(), String> {
        let active = self.active.as_mut().ok_or("ASTRA_EMU_CORE_CLOSED")?;
        if active
            .failure
            .as_ref()
            .is_none_or(|failure| !failure.can_retry)
        {
            return Err("ASTRA_EMU_CORE_RETRY_UNAVAILABLE".into());
        }
        self.registry.events(
            active.epoch,
            vec![FamilyEvent::Control {
                command: astra_emu_abi::CONTROL_RETRY,
            }],
        )?;
        active.failure = None;
        Ok(())
    }

    fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::add_game_directory(self, path)
    }
    fn start_file_patch(
        &mut self,
        script: &Path,
        patch: &Path,
        output: &Path,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::start_file_patch(self, script, patch, output)
    }
    fn cancel_file_patch(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::cancel_file_patch(self)
    }
    fn test_translation_connection(
        &mut self,
        form: &astra_emu_manager_ui_slint::TranslationProfileForm,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::test_translation_connection(self, form, secret)
    }
    fn set_window_state(&mut self, state: WindowState) -> Result<(), String> {
        self.update_window_state(state)
    }
    fn physical_event(&mut self, event: FamilyEvent) -> Result<(), String> {
        self.handle_physical_event(event)
    }
    fn is_game_active(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.closing.is_none())
    }
    fn model(&self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::model(self)
    }

    fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::select_case(self, case_id)
    }

    fn search(&mut self, query: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::search(self, query)
    }

    fn save_translation_profile(
        &mut self,
        form: &astra_emu_manager_ui_slint::TranslationProfileForm,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::save_translation_profile(self, form, secret)
    }

    fn set_translation_enabled(&mut self, enabled: bool) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_translation_enabled(self, enabled)
    }

    fn set_network_allowed(&mut self, enabled: bool) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_network_allowed(self, enabled)
    }

    fn set_sensitive_covers(&mut self, enabled: bool) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_sensitive_covers(self, enabled)
    }

    fn save_bangumi_token(&mut self, token: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::save_bangumi_token(self, token)
    }

    fn clear_bangumi_token(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::clear_bangumi_token(self)
    }

    fn filter_settings(&self) -> astra_emu_manager_core::FilterSettings {
        self.filter_settings.clone()
    }
    fn set_filter_preset(
        &mut self,
        id: &str,
        apply: astra_emu_manager::FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_filter_preset(self, id, apply)
    }
    fn set_filter_parameter(
        &mut self,
        key: &str,
        value: &str,
        apply: astra_emu_manager::FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_filter_parameter(self, key, value, apply)
    }
    fn reset_filter_parameters(
        &mut self,
        apply: astra_emu_manager::FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::reset_filter_parameters(self, apply)
    }
    fn load_filter_source(
        &mut self,
        path: &Path,
        apply: astra_emu_manager::FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::load_filter_source(self, path, apply)
    }

    fn game_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String> {
        AstraEmuManagerController::game_input(self, control, pressed, value)
    }

    fn release_inputs(&mut self) -> Result<(), String> {
        AstraEmuManagerController::release_inputs(self)
    }

    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::launch(self, case_id)
    }

    fn leave_game(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::leave_game(self)
    }

    fn search_metadata(&mut self, provider: &str, query: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::search_metadata(self, provider, query)
    }

    fn refresh_metadata(&mut self, provider: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::refresh_metadata(self, provider)
    }

    fn accept_match(&mut self, candidate_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::accept_match(self, candidate_id)
    }

    fn unlink_identity(&mut self, provider: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::unlink_identity(self, provider)
    }

    fn update_bangumi_play_status(
        &mut self,
        status: &str,
        rating: i32,
        note: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::update_bangumi_play_status(self, status, rating, note)
    }

    fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String> {
        AstraEmuManagerController::poll_platform(self)
    }

    fn set_host_wake(&mut self, wake: HostWake) {
        AstraEmuManagerController::set_host_wake(self, wake)
    }

    fn runtime_deadline(&self) -> Option<Instant> {
        AstraEmuManagerController::runtime_deadline(self)
    }

    fn advance_runtime(&mut self) -> Result<Option<ManagerViewModel>, String> {
        AstraEmuManagerController::advance_runtime(self)
    }

    fn game_fullscreen(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.closing.is_none() && active.fullscreen)
    }

    fn take_game_icon(&mut self) -> Option<astra_emu_manager_core::model::FamilyWindowIcon> {
        self.active
            .as_mut()
            .filter(|active| active.closing.is_none())
            .and_then(|active| active.pending_icon.take())
    }

    fn take_game_pointer(&mut self) -> Option<astra_emu_manager_core::model::FamilyPointerRequest> {
        self.active
            .as_mut()
            .filter(|active| active.closing.is_none())
            .and_then(|active| active.pending_pointer.take())
    }

    fn game_title(&self) -> Option<&str> {
        self.active
            .as_ref()
            .filter(|active| active.closing.is_none())
            .and_then(|active| active.title.as_deref())
    }

    fn set_game_emulator(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_game_emulator(self, game_id, plugin_id)
    }

    fn set_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_game_config(self, game_id, field_id, raw)
    }

    fn reset_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::reset_game_config(self, game_id, field_id)
    }

    fn set_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_emulator_default(self, plugin_id, field_id, raw)
    }

    fn reset_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::reset_emulator_default(self, plugin_id, field_id)
    }

    fn select_emulator_defaults(&mut self, plugin_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::select_emulator_defaults(self, plugin_id)
    }

    fn pick_config_path(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::pick_config_path(self, game_id, field_id)
    }

    fn set_render_preference(
        &mut self,
        game_id: &str,
        preference: Option<astra_emu_manager_core::RenderPreference>,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_render_preference(self, game_id, preference)
    }

    fn set_emulator_render_default(
        &mut self,
        plugin_id: &str,
        preference: astra_emu_manager_core::RenderPreference,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_emulator_render_default(self, plugin_id, preference)
    }

    fn set_library_sort(&mut self, mode: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_library_sort(self, mode)
    }

    fn set_compatibility_filter(&mut self, filter: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_compatibility_filter(self, filter)
    }

    fn refresh_compatibility(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::refresh_compatibility(self)
    }

    fn fetch_releases(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::fetch_releases(self)
    }

    fn pin_release(&mut self, release_id: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::pin_release(self, release_id)
    }

    fn input_mapping(&self) -> astra_emu_manager_core::InputMapping {
        AstraEmuManagerController::input_mapping(self)
    }

    fn set_gamepad_enabled(&mut self, enabled: bool) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_gamepad_enabled(self, enabled)
    }

    fn set_gamepad_deadzone(&mut self, deadzone: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_gamepad_deadzone(self, deadzone)
    }

    fn set_gamepad_binding(&mut self, button: &str, key: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_gamepad_binding(self, button, key)
    }

    fn reset_gamepad_bindings(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::reset_gamepad_bindings(self)
    }

    fn set_game_input_override(
        &mut self,
        game_id: &str,
        enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_game_input_override(self, game_id, enabled)
    }

    fn set_game_gamepad_binding(
        &mut self,
        game_id: &str,
        button: &str,
        key: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_game_gamepad_binding(self, game_id, button, key)
    }

    fn set_appearance(
        &mut self,
        appearance: astra_emu_manager_core::AppearanceSettings,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_appearance(self, appearance)
    }

    fn install_core(&mut self, source: &std::path::Path) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::install_core(self, source)
    }

    fn uninstall_core(&mut self, file: &str) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::uninstall_core(self, file)
    }

    fn log_text(&self, core: &str) -> Result<(String, usize), String> {
        AstraEmuManagerController::log_text(self, core)
    }

    fn engine_action(&mut self, control: u32) -> Result<(), String> {
        AstraEmuManagerController::engine_action(self, control)
    }
}
