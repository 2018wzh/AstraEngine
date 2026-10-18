use super::*;

impl ManagerController for AstraEmuManagerController {
    fn refresh_cores(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::refresh_cores(self)
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

    fn set_system_theme(&mut self, dark: bool) {
        if self.appearance.theme_mode == "system" {
            self.appearance.theme_dark = dark;
        }
    }

    fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::add_game_directory(self, path)
    }
    fn test_translation_connection(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::test_translation_connection(self)
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

    #[allow(clippy::too_many_arguments)]
    fn save_translation_profile(
        &mut self,
        endpoint_kind: &str,
        endpoint: &str,
        protocol: &str,
        model: &str,
        target_language: &str,

        timeout_ms: i32,

        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::save_translation_profile(
            self,
            endpoint_kind,
            endpoint,
            protocol,
            model,
            target_language,
            timeout_ms,
            secret,
        )
    }

    fn grant_translation_consent(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::grant_translation_consent(self)
    }

    fn filter_settings(&self) -> astra_emu_manager_core::FilterSettings {
        self.filter_settings.clone()
    }
    fn pending_filter_settings(&self) -> Result<astra_emu_manager_core::FilterSettings, String> {
        self.pending_filter()
    }
    fn commit_filter_settings(
        &mut self,
        settings: astra_emu_manager_core::FilterSettings,
    ) -> Result<ManagerViewModel, String> {
        self.commit_filter(settings)
    }

    fn game_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String> {
        AstraEmuManagerController::game_input(self, control, pressed, value)
    }

    fn release_inputs(&mut self) -> Result<(), String> {
        AstraEmuManagerController::release_inputs(self)
    }

    fn rescan(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::rescan(self)
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

    fn set_metadata_consent(
        &mut self,
        provider: &str,
        enabled: bool,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_metadata_consent(self, provider, enabled, secret)
    }

    fn set_sensitive_cover_policy(
        &mut self,
        provider: &str,
        enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::set_sensitive_cover_policy(self, provider, enabled)
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

    fn set_theme(&mut self, dark: bool) -> Result<(), String> {
        AstraEmuManagerController::set_theme(self, dark)
    }

    fn set_grid_columns(&mut self, columns: i32) -> Result<(), String> {
        AstraEmuManagerController::set_grid_columns(self, columns)
    }

    fn set_theme_mode(&mut self, mode: &str) -> Result<(), String> {
        AstraEmuManagerController::set_theme_mode(self, mode)
    }

    fn set_accent(&mut self, accent: &str) -> Result<(), String> {
        AstraEmuManagerController::set_accent(self, accent)
    }

    fn set_density(&mut self, density: &str) -> Result<(), String> {
        AstraEmuManagerController::set_density(self, density)
    }

    fn set_view_mode(&mut self, view_mode: &str) -> Result<(), String> {
        AstraEmuManagerController::set_view_mode(self, view_mode)
    }

    fn family_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        AstraEmuManagerController::family_config_changed(self, key, value)
    }

    fn save_family_config(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::save_family_config(self)
    }

    fn reset_family_config(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::reset_family_config(self)
    }

    fn select_musica_key_file(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::select_musica_key_file(self)
    }

    fn filter_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        AstraEmuManagerController::filter_config_changed(self, key, value)
    }

    fn reset_filter_config(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::reset_filter_config(self)
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

    fn save_input_config(
        &mut self,

        gamepad_enabled: bool,
        gamepad_deadzone: &str,
    ) -> Result<(), String> {
        AstraEmuManagerController::save_input_config(self, gamepad_enabled, gamepad_deadzone)
    }

    fn input_mapping(&self) -> astra_emu_manager_core::InputMapping {
        AstraEmuManagerController::input_mapping(self)
    }

    fn set_gamepad_binding(&mut self, button_id: &str, key_name: &str) -> Result<(), String> {
        AstraEmuManagerController::set_gamepad_binding(self, button_id, key_name)
    }

    fn reset_gamepad_mapping(&mut self) -> Result<(), String> {
        AstraEmuManagerController::reset_gamepad_mapping(self)
    }

    fn save_per_game_input_mapping(
        &mut self,
        gamepad_enabled: bool,
        deadzone: &str,
    ) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::save_per_game_input_mapping(self, gamepad_enabled, deadzone)
    }

    fn clear_per_game_input_mapping(&mut self) -> Result<ManagerViewModel, String> {
        AstraEmuManagerController::clear_per_game_input_mapping(self)
    }
}
