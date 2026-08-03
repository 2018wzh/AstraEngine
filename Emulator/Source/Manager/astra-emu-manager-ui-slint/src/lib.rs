use std::{path::Path, rc::Rc};

use slint::{ModelRc, SharedString, VecModel};

slint::include_modules!();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameCardViewModel {
    pub case_id: String,
    pub title: String,
    pub family: String,
    pub cover_uri: String,
    pub diagnostic: String,
    pub play_time: String,
    pub last_played: String,
    /// Compatibility grade: "" | "perfect" | "completable" | "flawed" |
    /// "boot_only" | "unplayable".
    pub compatibility_status: String,
}

/// One finished play session row shown in the inspector history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaySessionViewModel {
    pub start_time: String,
    pub duration: String,
    pub ended_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchReviewViewModel {
    pub candidate_id: String,
    pub case_id: String,
    pub provider: String,
    pub remote_id: String,
    pub title: String,
    pub aliases: String,
    pub release_date: String,
    pub developer: String,
    pub evidence: String,
    pub score_millis: i32,
    pub diagnostic: String,
}

/// One row of the VFS file tree (read-only view over the mount set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsEntryViewModel {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_display: String,
    pub source_layer: String,
    pub expanded: bool,
    pub depth: i32,
}

/// Content preview for the selected VFS file.
#[derive(Debug, Clone, PartialEq)]
pub struct VfsPreviewViewModel {
    pub path: String,
    /// "text" | "image" | "binary"
    pub kind: String,
    pub text_content: String,
    pub hex_summary: String,
    pub image_uri: String,
    pub size_display: String,
    pub source_layer: String,
    pub resolve_path: String,
}

/// Keyboard / gamepad / touch input configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct InputConfigViewModel {
    pub confirm_key: String,
    pub cancel_key: String,
    pub touch_sensitivity: f32,
    pub gamepad_enabled: bool,
    pub gamepad_deadzone: String,
    pub gamepad_bindings: Vec<GamepadBindingViewModel>,
}

/// A single gamepad-input -> key-name binding row for the settings UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamepadBindingViewModel {
    pub button_id: String,
    pub button_label: String,
    pub key_name: String,
}

impl Default for InputConfigViewModel {
    fn default() -> Self {
        Self {
            confirm_key: "return".into(),
            cancel_key: "escape".into(),
            touch_sensitivity: 50.0,
            gamepad_enabled: true,
            gamepad_deadzone: "medium".into(),
            gamepad_bindings: Vec::new(),
        }
    }
}

/// Theme / layout appearance preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceViewModel {
    pub theme_dark: bool,
    pub grid_columns: i32,
}

impl Default for AppearanceViewModel {
    fn default() -> Self {
        Self {
            theme_dark: true,
            grid_columns: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManagerViewModel {
    pub games: Vec<GameCardViewModel>,
    pub match_reviews: Vec<MatchReviewViewModel>,
    pub selected_case_id: Option<String>,
    pub search_query: String,
    pub endpoint_identity: String,
    pub model_identity: String,
    pub global_diagnostic: String,
    pub selected_nls: String,
    pub translation_endpoint_kind: String,
    pub translation_endpoint: String,
    pub translation_protocol: String,
    pub translation_model: String,
    pub translation_target_language: String,
    pub translation_context_sentences: i32,
    pub translation_body_limit_bytes: i32,
    pub translation_timeout_ms: i32,
    pub translation_background: String,
    pub translation_glossary: String,
    pub translation_consent_present: bool,
    pub translation_persistent_cache: bool,
    pub filter_preset: String,
    pub diagnostics_summary: String,
    pub patches_summary: String,
    pub vndb_consent: bool,
    pub bangumi_consent: bool,
    pub sensitive_covers: bool,
    pub bangumi_play_status: String,
    pub bangumi_rating: i32,
    pub bangumi_note: String,
    pub bangumi_sync_summary: String,
    // ===== New fields (UI redesign) =====
    /// Inspector details for the selected game.
    pub selected_title: String,
    pub selected_family: String,
    pub selected_play_time: String,
    pub selected_last_played: String,
    pub selected_vfs_status: String,
    /// Finished play sessions for the selected game, most recent first.
    pub play_history: Vec<PlaySessionViewModel>,
    /// Library sort mode: "title" | "recent" | "play_time".
    pub library_sort: String,
    /// Compatibility filter: "all" | "perfect" | "completable" | "flawed" |
    /// "boot_only" | "unplayable" | "unknown".
    pub compatibility_filter: String,
    /// Configured compatibility database source URL (read-only in the UI).
    pub compatibility_source_url: String,
    /// Human-readable compatibility sync state (last fetch / entry count).
    pub compatibility_sync_summary: String,
    /// Compatibility details for the selected game.
    pub selected_compatibility_status: String,
    pub selected_compatibility_notes: String,
    pub selected_compatibility_updated: String,
    pub selected_compatibility_provider: String,
    /// Navigation. Empty `current_page` means "do not change the current page".
    pub current_page: String,
    /// VFS browser state.
    pub vfs_entries: Vec<VfsEntryViewModel>,
    pub vfs_preview: Option<VfsPreviewViewModel>,
    pub vfs_selected_path: String,
    pub vfs_current_dir: String,
    pub vfs_mount_summary: String,
    /// Input configuration.
    pub input_config: InputConfigViewModel,
    /// Appearance preferences.
    pub appearance: AppearanceViewModel,
    /// About page metadata.
    pub version: String,
    pub build_identity: String,
}

pub struct SlintManagerAdapter {
    window: ManagerWindow,
    games: Rc<VecModel<GameCard>>,
    reviews: Rc<VecModel<MatchReview>>,
    vfs_entries: Rc<VecModel<VfsEntry>>,
    play_history: Rc<VecModel<PlaySession>>,
    gamepad_bindings: Rc<VecModel<GamepadBinding>>,
}

impl SlintManagerAdapter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = ManagerWindow::new()?;
        let games = Rc::new(VecModel::default());
        let reviews = Rc::new(VecModel::default());
        let vfs_entries = Rc::new(VecModel::default());
        let play_history = Rc::new(VecModel::default());
        let gamepad_bindings = Rc::new(VecModel::default());
        window.set_games(ModelRc::from(games.clone()));
        window.set_match_reviews(ModelRc::from(reviews.clone()));
        window.set_vfs_entries(ModelRc::from(vfs_entries.clone()));
        window.set_play_history(ModelRc::from(play_history.clone()));
        window.set_gamepad_bindings(ModelRc::from(gamepad_bindings.clone()));
        Ok(Self {
            window,
            games,
            reviews,
            vfs_entries,
            play_history,
            gamepad_bindings,
        })
    }

    pub fn apply(&self, model: &ManagerViewModel) {
        let cards = model
            .games
            .iter()
            .map(|game| GameCard {
                case_id: SharedString::from(&game.case_id),
                title: SharedString::from(&game.title),
                family: SharedString::from(&game.family),
                cover_uri: SharedString::from(&game.cover_uri),
                cover: if game.cover_uri.is_empty() {
                    slint::Image::default()
                } else {
                    slint::Image::load_from_path(Path::new(&game.cover_uri)).unwrap_or_default()
                },
                diagnostic: SharedString::from(&game.diagnostic),
                play_time: SharedString::from(&game.play_time),
                last_played: SharedString::from(&game.last_played),
                compatibility_status: SharedString::from(&game.compatibility_status),
            })
            .collect::<Vec<_>>();
        self.games.set_vec(cards);
        self.reviews.set_vec(
            model
                .match_reviews
                .iter()
                .map(|item| MatchReview {
                    candidate_id: item.candidate_id.as_str().into(),
                    case_id: item.case_id.as_str().into(),
                    provider: item.provider.as_str().into(),
                    remote_id: item.remote_id.as_str().into(),
                    title: item.title.as_str().into(),
                    aliases: item.aliases.as_str().into(),
                    release_date: item.release_date.as_str().into(),
                    developer: item.developer.as_str().into(),
                    evidence: item.evidence.as_str().into(),
                    score_millis: item.score_millis,
                    diagnostic: item.diagnostic.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        self.apply_vfs(&model.vfs_entries, model.vfs_preview.as_ref());
        self.gamepad_bindings.set_vec(
            model
                .input_config
                .gamepad_bindings
                .iter()
                .map(|binding| GamepadBinding {
                    button_id: binding.button_id.as_str().into(),
                    button_label: binding.button_label.as_str().into(),
                    key_name: binding.key_name.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        self.window
            .set_vfs_selected_path(model.vfs_selected_path.as_str().into());
        self.window
            .set_vfs_current_dir(model.vfs_current_dir.as_str().into());
        self.window
            .set_vfs_mount_summary(model.vfs_mount_summary.as_str().into());
        self.window
            .set_selected_case_id(model.selected_case_id.as_deref().unwrap_or_default().into());
        self.window
            .set_search_query(model.search_query.as_str().into());
        self.window
            .set_endpoint_identity(model.endpoint_identity.as_str().into());
        self.window
            .set_model_identity(model.model_identity.as_str().into());
        self.window
            .set_global_diagnostic(model.global_diagnostic.as_str().into());
        self.window
            .set_selected_nls(model.selected_nls.as_str().into());
        self.window
            .set_selected_title(model.selected_title.as_str().into());
        self.window
            .set_selected_family(model.selected_family.as_str().into());
        self.window
            .set_selected_play_time(model.selected_play_time.as_str().into());
        self.window
            .set_selected_last_played(model.selected_last_played.as_str().into());
        self.window
            .set_library_sort(model.library_sort.as_str().into());
        self.window
            .set_compatibility_filter(model.compatibility_filter.as_str().into());
        self.window
            .set_compatibility_source_url(model.compatibility_source_url.as_str().into());
        self.window
            .set_compatibility_sync_summary(model.compatibility_sync_summary.as_str().into());
        self.window
            .set_selected_compatibility_status(model.selected_compatibility_status.as_str().into());
        self.window
            .set_selected_compatibility_notes(model.selected_compatibility_notes.as_str().into());
        self.window.set_selected_compatibility_updated(
            model.selected_compatibility_updated.as_str().into(),
        );
        self.window.set_selected_compatibility_provider(
            model.selected_compatibility_provider.as_str().into(),
        );
        self.play_history.set_vec(
            model
                .play_history
                .iter()
                .map(|session| PlaySession {
                    start_time: session.start_time.as_str().into(),
                    duration: session.duration.as_str().into(),
                    ended_by: session.ended_by.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        self.window
            .set_selected_vfs_status(model.selected_vfs_status.as_str().into());
        self.window
            .set_translation_endpoint_kind(model.translation_endpoint_kind.as_str().into());
        self.window
            .set_translation_profile_endpoint(model.translation_endpoint.as_str().into());
        self.window
            .set_translation_profile_protocol(model.translation_protocol.as_str().into());
        self.window
            .set_translation_profile_model(model.translation_model.as_str().into());
        self.window
            .set_translation_target_language(model.translation_target_language.as_str().into());
        self.window
            .set_translation_context_sentences(model.translation_context_sentences);
        self.window
            .set_translation_body_limit_bytes(model.translation_body_limit_bytes);
        self.window
            .set_translation_timeout_ms(model.translation_timeout_ms);
        self.window
            .set_translation_background(model.translation_background.as_str().into());
        self.window
            .set_translation_glossary(model.translation_glossary.as_str().into());
        self.window
            .set_translation_consent_present(model.translation_consent_present);
        self.window
            .set_translation_persistent_cache(model.translation_persistent_cache);
        self.window
            .set_filter_preset(model.filter_preset.as_str().into());
        self.window
            .set_diagnostics_summary(model.diagnostics_summary.as_str().into());
        self.window
            .set_patches_summary(model.patches_summary.as_str().into());
        self.window.set_vndb_consent(model.vndb_consent);
        self.window.set_bangumi_consent(model.bangumi_consent);
        self.window.set_sensitive_covers(model.sensitive_covers);
        self.window
            .set_bangumi_play_status(model.bangumi_play_status.as_str().into());
        self.window.set_bangumi_rating(model.bangumi_rating);
        self.window
            .set_bangumi_note(model.bangumi_note.as_str().into());
        self.window
            .set_bangumi_sync_summary(model.bangumi_sync_summary.as_str().into());
        self.window
            .set_confirm_key(model.input_config.confirm_key.as_str().into());
        self.window
            .set_cancel_key(model.input_config.cancel_key.as_str().into());
        self.window
            .set_touch_sensitivity(model.input_config.touch_sensitivity);
        self.window
            .set_gamepad_enabled(model.input_config.gamepad_enabled);
        self.window
            .set_gamepad_deadzone(model.input_config.gamepad_deadzone.as_str().into());
        self.window.set_theme_dark(model.appearance.theme_dark);
        self.window.set_grid_columns(model.appearance.grid_columns);
        self.window.set_version(model.version.as_str().into());
        self.window
            .set_build_identity(model.build_identity.as_str().into());
        // An empty current_page means "keep whatever the UI is showing".
        if !model.current_page.is_empty() {
            self.window
                .set_current_page(model.current_page.as_str().into());
        }
    }

    /// Targeted VFS update without a full model round-trip.
    pub fn apply_vfs(&self, entries: &[VfsEntryViewModel], preview: Option<&VfsPreviewViewModel>) {
        self.vfs_entries.set_vec(
            entries
                .iter()
                .map(|entry| VfsEntry {
                    path: entry.path.as_str().into(),
                    name: entry.name.as_str().into(),
                    is_dir: entry.is_dir,
                    size_display: entry.size_display.as_str().into(),
                    source_layer: entry.source_layer.as_str().into(),
                    expanded: entry.expanded,
                    depth: entry.depth,
                })
                .collect::<Vec<_>>(),
        );
        match preview {
            Some(preview) => {
                self.window.set_vfs_preview(VfsPreview {
                    path: preview.path.as_str().into(),
                    kind: preview.kind.as_str().into(),
                    text_content: preview.text_content.as_str().into(),
                    hex_summary: preview.hex_summary.as_str().into(),
                    image_data: if preview.image_uri.is_empty() {
                        slint::Image::default()
                    } else {
                        slint::Image::load_from_path(Path::new(&preview.image_uri))
                            .unwrap_or_default()
                    },
                    size_display: preview.size_display.as_str().into(),
                    source_layer: preview.source_layer.as_str().into(),
                    resolve_path: preview.resolve_path.as_str().into(),
                });
                self.window.set_vfs_has_preview(true);
            }
            None => {
                self.window.set_vfs_has_preview(false);
            }
        }
    }

    /// Switch the color theme directly (UI-initiated toggle).
    pub fn set_theme(&self, dark: bool) {
        self.window.set_theme_dark(dark);
    }

    pub fn window(&self) -> &ManagerWindow {
        &self.window
    }

    pub fn set_game_active(&self, active: bool) {
        self.window.set_game_active(active);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppearanceViewModel, GameCardViewModel, InputConfigViewModel, ManagerViewModel,
        MatchReviewViewModel, PlaySessionViewModel, VfsEntryViewModel, VfsPreviewViewModel,
    };

    fn assert_contract_is_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_view_models_do_not_require_ui_thread_types() {
        assert_contract_is_send_sync::<GameCardViewModel>();
        assert_contract_is_send_sync::<ManagerViewModel>();
        assert_contract_is_send_sync::<MatchReviewViewModel>();
        assert_contract_is_send_sync::<VfsEntryViewModel>();
        assert_contract_is_send_sync::<VfsPreviewViewModel>();
        assert_contract_is_send_sync::<InputConfigViewModel>();
        assert_contract_is_send_sync::<AppearanceViewModel>();
        assert_contract_is_send_sync::<PlaySessionViewModel>();
    }
}
