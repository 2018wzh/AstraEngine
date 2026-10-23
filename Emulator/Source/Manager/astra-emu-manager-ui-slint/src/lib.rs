mod adapter;
mod config_model;
mod covers;
mod list_model;
mod notices;

slint::include_modules!();

pub use adapter::SlintManagerAdapter;
pub use notices::{describe_error, report_error, ErrorText};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManagerViewModel {
    pub app: AppViewModel,
    pub network: NetworkViewModel,
    pub library: LibraryViewModel,
    pub selection: SelectionViewModel,
    pub game_settings: GameSettingsViewModel,
    pub patch: PatchViewModel,
    pub cores: CoresViewModel,
    pub notices: NoticesViewModel,
    pub settings: SettingsViewModel,
    pub filters: FiltersViewModel,
    pub session: SessionViewModel,
    pub onboarding: OnboardingViewModel,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppViewModel {
    pub version: String,
    pub build: String,
    pub appearance: AppearanceViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceViewModel {
    pub theme_mode: String,
    pub density: String,
    pub cover_size: String,
    pub view_mode: String,
    pub reduce_motion: bool,
    pub cover_badges: bool,
}

impl Default for AppearanceViewModel {
    fn default() -> Self {
        Self {
            theme_mode: "system".into(),
            density: "comfortable".into(),
            cover_size: "medium".into(),
            view_mode: "grid".into(),
            reduce_motion: false,
            cover_badges: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChoiceViewModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkViewModel {
    pub allowed: bool,
    pub syncing: bool,
    pub sync_status: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryViewModel {
    pub games: Vec<GameCardViewModel>,
    pub query: String,
    pub sort: String,
    pub compat_filter: String,
    pub prepare_state: String,
    pub prepare_label: String,
    pub prepare_detail: String,
    /// `None` means indeterminate progress.
    pub prepare_progress: Option<f32>,
    pub can_cancel_prepare: bool,
    pub selected_id: String,
}

impl Default for LibraryViewModel {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            query: String::new(),
            sort: "title".into(),
            compat_filter: "all".into(),
            prepare_state: "unprepared".into(),
            prepare_label: "准备".into(),
            prepare_detail: String::new(),
            prepare_progress: None,
            can_cancel_prepare: false,
            selected_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameCardViewModel {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover_uri: String,
    pub ready: bool,
    pub problem: String,
    pub play_time: String,
    pub last_played: String,
    pub compat: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionViewModel {
    pub game: SelectedGameViewModel,
    pub emulator_choices: Vec<ChoiceViewModel>,
    pub compat: CompatViewModel,
    pub releases: Vec<ChoiceViewModel>,
    pub pinned_release: String,
    pub history: Vec<PlaySessionViewModel>,
    pub bangumi: BangumiViewModel,
    pub bangumi_summary: String,
    pub metadata_busy: bool,
    pub metadata_status: String,
    pub matches: Vec<MatchViewModel>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectedGameViewModel {
    pub id: String,
    pub title: String,
    pub engine: String,
    pub emulator: String,
    pub emulator_id: String,
    pub launchable: bool,
    pub problem: String,
    pub play_time: String,
    pub last_played: String,
    pub developer: String,
    pub release_date: String,
    pub platforms: String,
    pub description: String,
    pub tags: String,
    pub aliases: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatViewModel {
    pub status: String,
    pub notes: String,
    pub updated: String,
    pub release_id: String,
    pub emulator_label: String,
    pub report_url: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaySessionViewModel {
    pub started: String,
    pub duration: String,
    pub ended_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchViewModel {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub aliases: String,
    pub release_date: String,
    pub developer: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BangumiViewModel {
    pub status: String,
    pub rating: i32,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigFieldViewModel {
    pub key: String,
    pub label: String,
    pub description: String,
    pub group: String,
    /// `bool | integer | float | string | enum | file | directory`
    pub kind: String,
    pub value: String,
    pub display_value: String,
    pub options: Vec<String>,
    pub option_values: Vec<String>,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: String,
    pub advanced: bool,
    pub overridden: bool,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamepadBindingViewModel {
    pub id: String,
    pub label: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderViewModel {
    pub supported: bool,
    pub options: Vec<ChoiceViewModel>,
    pub mode: String,
    pub custom_width: i32,
    pub custom_height: i32,
    pub min: String,
    pub max: String,
    pub note: String,
    pub error: String,
}

impl Default for RenderViewModel {
    fn default() -> Self {
        Self {
            supported: false,
            options: Vec::new(),
            mode: "default".into(),
            custom_width: 0,
            custom_height: 0,
            min: String::new(),
            max: String::new(),
            note: String::new(),
            error: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecipeViewModel {
    pub id: String,
    pub title: String,
    pub recommended: bool,
}

/// Conversion job state; `step` is 0 before a run, 4 while running or failed, 5 once imported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchStateViewModel {
    pub step: i32,
    pub running: bool,
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub output: String,
    pub error: String,
    pub can_start: bool,
    pub script: String,
    pub patch_directory: String,
    pub output_directory: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchGameViewModel {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover_uri: String,
    pub linked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchViewModel {
    pub games: Vec<PatchGameViewModel>,
    pub query: String,
    /// Empty `id` when no game is selected.
    pub game: PatchGameViewModel,
    pub recipes: Vec<RecipeViewModel>,
    pub hint: String,
    pub state: PatchStateViewModel,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameSettingsViewModel {
    pub game_id: String,
    pub title: String,
    pub emulator_choices: Vec<ChoiceViewModel>,
    pub emulator_id: String,
    pub render: RenderViewModel,
    pub fields: Vec<ConfigFieldViewModel>,
    pub recommended_available: bool,
    pub recommended_summary: String,
    pub input_override: bool,
    pub bindings: Vec<GamepadBindingViewModel>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoresViewModel {
    pub items: Vec<CoreViewModel>,
    pub restart_required: bool,
    pub updates_available: i32,
    pub default_fields: Vec<ConfigFieldViewModel>,
    pub default_plugin: String,
    pub default_render: RenderViewModel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreViewModel {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub version: String,
    /// `loaded | failed | pending-install | pending-update | pending-removal | available`
    pub state: String,
    /// `bundled | user | online`
    pub source: String,
    pub message: String,
    pub capabilities: Vec<String>,
    pub removable: bool,
    pub update_version: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoticesViewModel {
    pub items: Vec<NoticeViewModel>,
    pub snackbar: SnackbarViewModel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoticeViewModel {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub code: String,
    pub core: String,
}

/// A new `serial` shows `text` again even when it repeats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnackbarViewModel {
    pub serial: u32,
    pub text: String,
    pub action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranslationFormViewModel {
    pub provider: String,
    pub base_url: String,
    pub openai_api: String,
    pub model: String,
    pub language: String,
    pub timeout_s: i32,
    pub custom_prompt: String,
    pub context_segments: i32,
    pub context_chars: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludedViewModel {
    pub id: String,
    pub title: String,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsViewModel {
    pub translation: TranslationFormViewModel,
    pub translation_enabled: bool,
    pub secret_saved: bool,
    pub models: Vec<String>,
    pub translation_status: String,
    pub sensitive_covers: bool,
    pub bangumi_token_saved: bool,
    pub compat_summary: String,
    pub gamepad_enabled: bool,
    pub deadzone: String,
    pub bindings: Vec<GamepadBindingViewModel>,
    pub roots: Vec<String>,
    pub excluded: Vec<ExcludedViewModel>,
}

impl Default for SettingsViewModel {
    fn default() -> Self {
        Self {
            translation: TranslationFormViewModel::default(),
            translation_enabled: false,
            secret_saved: false,
            models: Vec::new(),
            translation_status: String::new(),
            sensitive_covers: false,
            bangumi_token_saved: false,
            compat_summary: String::new(),
            gamepad_enabled: true,
            deadzone: "medium".into(),
            bindings: Vec::new(),
            roots: Vec::new(),
            excluded: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FiltersViewModel {
    pub preset: String,
    pub fields: Vec<ConfigFieldViewModel>,
}

impl Default for FiltersViewModel {
    fn default() -> Self {
        Self {
            preset: "none".into(),
            fields: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionViewModel {
    /// `opening | running`
    pub phase: String,
    pub launch_title: String,
    /// Library card whose cached cover becomes `Session.launch-cover`.
    pub launch_game_id: String,
    pub actions: Vec<EngineActionViewModel>,
    pub action_message: String,
    pub action_serial: u32,
}

impl Default for SessionViewModel {
    fn default() -> Self {
        Self {
            phase: "opening".into(),
            launch_title: String::new(),
            launch_game_id: String::new(),
            actions: Vec::new(),
            action_message: String::new(),
            action_serial: 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineActionViewModel {
    pub control: i32,
    pub label: String,
    pub icon: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnboardingViewModel {
    pub roots: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contract_is_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_view_models_do_not_require_ui_thread_types() {
        assert_contract_is_send_sync::<ManagerViewModel>();
        assert_contract_is_send_sync::<ErrorText>();
    }
}
