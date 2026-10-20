mod adapter;
mod config_model;
mod covers;
mod notices;

slint::include_modules!();

pub use adapter::SlintManagerAdapter;
pub use notices::{describe_error, report_error, ErrorText};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManagerViewModel {
    pub library: LibraryViewModel,
    pub selection: SelectionViewModel,
    pub cores: CoresViewModel,
    pub settings: SettingsViewModel,
    pub appearance: AppearanceViewModel,
    pub notices: Vec<NoticeViewModel>,
    pub snackbar: SnackbarViewModel,
    pub about: AboutViewModel,
    pub session: SessionViewModel,
    pub patch: PatchViewModel,
}

/// A new `serial` shows `text` again even when it repeats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnackbarViewModel {
    pub serial: u32,
    pub text: String,
    pub action: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatchViewModel {
    pub running: bool,
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub output: String,
    pub error: String,
    pub can_start: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryViewModel {
    pub games: Vec<GameCardViewModel>,
    pub query: String,
    pub sort: String,
    pub compat_filter: String,
    pub cores_ready: bool,
    pub can_load_cores: bool,
    pub busy: bool,
    pub status: String,
    pub selected_id: String,
}

impl Default for LibraryViewModel {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            query: String::new(),
            sort: "title".into(),
            compat_filter: "all".into(),
            cores_ready: false,
            can_load_cores: false,
            busy: false,
            status: String::new(),
            selected_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameCardViewModel {
    pub id: String,
    pub title: String,
    pub family: String,
    pub cover_uri: String,
    pub problem: String,
    pub play_time: String,
    pub last_played: String,
    pub compat: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionViewModel {
    pub id: String,
    pub title: String,
    pub family: String,
    pub emulator: String,
    pub launchable: bool,
    pub problem: String,
    pub play_time: String,
    pub last_played: String,
    pub developer: String,
    pub release_date: String,
    pub platforms: String,
    pub engine: String,
    pub description: String,
    pub tags: String,
    pub aliases: String,
    pub compat: CompatViewModel,
    pub releases: Vec<ReleaseViewModel>,
    pub history: Vec<PlaySessionViewModel>,
    pub matches: Vec<MatchViewModel>,
    pub metadata_busy: bool,
    pub metadata_status: String,
    pub bangumi: BangumiViewModel,
    pub emulator_choices: Vec<ChoiceViewModel>,
    pub emulator_id: String,
    pub family_fields: Vec<ConfigFieldViewModel>,
    pub render: RenderViewModel,
    pub input_override: bool,
    pub game_bindings: Vec<GamepadBindingViewModel>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChoiceViewModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderViewModel {
    pub supported: bool,
    pub options: Vec<ChoiceViewModel>,
    pub mode: String,
    pub custom_width: i32,
    pub custom_height: i32,
    pub range: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompatViewModel {
    pub status: String,
    pub notes: String,
    pub updated: String,
    pub vndb_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReleaseViewModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaySessionViewModel {
    pub started: String,
    pub duration: String,
    pub ended_by: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchViewModel {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub aliases: String,
    pub release_date: String,
    pub developer: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BangumiViewModel {
    pub status: String,
    pub rating: i32,
    pub note: String,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigFieldViewModel {
    pub key: String,
    pub label: String,
    pub description: String,
    pub group: String,
    /// `bool|integer|float|string|enum|file|directory`.
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
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoresViewModel {
    pub items: Vec<CoreViewModel>,
    pub restart_required: bool,
    pub defaults: EmulatorDefaultsViewModel,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmulatorDefaultsViewModel {
    pub plugin: String,
    pub fields: Vec<ConfigFieldViewModel>,
    pub render: RenderViewModel,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreViewModel {
    pub key: String,
    pub name: String,
    pub monogram: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub state: String,
    pub source: String,
    pub file: String,
    pub message: String,
    pub removable: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsViewModel {
    pub translation: TranslationProfileForm,
    pub translation_enabled: bool,
    pub translation_status: String,
    pub translation_models: Vec<String>,
    pub network_allowed: bool,
    pub sensitive_covers: bool,
    pub input: InputConfigViewModel,
    pub filter_preset: String,
    pub filter_fields: Vec<ConfigFieldViewModel>,
    pub compat_source: String,
    pub compat_summary: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranslationProfileForm {
    pub provider: String,
    pub base_url: String,
    pub openai_api: String,
    pub model: String,
    pub language: String,
    pub timeout_ms: i32,
    pub custom_prompt: String,
    pub context_segments: i32,
    pub context_chars: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputConfigViewModel {
    pub gamepad_enabled: bool,
    pub deadzone: String,
    pub bindings: Vec<GamepadBindingViewModel>,
}

impl Default for InputConfigViewModel {
    fn default() -> Self {
        Self {
            gamepad_enabled: true,
            deadzone: "medium".into(),
            bindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamepadBindingViewModel {
    pub id: String,
    pub label: String,
    pub key: String,
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoticeViewModel {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub code: String,
    pub core: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AboutViewModel {
    pub version: String,
    pub build: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionViewModel {
    pub actions: Vec<EngineActionViewModel>,
    pub action_message: String,
    pub action_serial: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineActionViewModel {
    pub control: i32,
    pub label: String,
    pub icon: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contract_is_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_view_models_do_not_require_ui_thread_types() {
        assert_contract_is_send_sync::<ManagerViewModel>();
        assert_contract_is_send_sync::<LibraryViewModel>();
        assert_contract_is_send_sync::<GameCardViewModel>();
        assert_contract_is_send_sync::<SelectionViewModel>();
        assert_contract_is_send_sync::<CompatViewModel>();
        assert_contract_is_send_sync::<ReleaseViewModel>();
        assert_contract_is_send_sync::<PlaySessionViewModel>();
        assert_contract_is_send_sync::<MatchViewModel>();
        assert_contract_is_send_sync::<BangumiViewModel>();
        assert_contract_is_send_sync::<ConfigFieldViewModel>();
        assert_contract_is_send_sync::<CoresViewModel>();
        assert_contract_is_send_sync::<CoreViewModel>();
        assert_contract_is_send_sync::<SettingsViewModel>();
        assert_contract_is_send_sync::<TranslationProfileForm>();
        assert_contract_is_send_sync::<InputConfigViewModel>();
        assert_contract_is_send_sync::<GamepadBindingViewModel>();
        assert_contract_is_send_sync::<AppearanceViewModel>();
        assert_contract_is_send_sync::<NoticeViewModel>();
        assert_contract_is_send_sync::<AboutViewModel>();
        assert_contract_is_send_sync::<SessionViewModel>();
        assert_contract_is_send_sync::<EngineActionViewModel>();
        assert_contract_is_send_sync::<ErrorText>();
    }
}
