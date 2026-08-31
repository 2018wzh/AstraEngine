use std::{cell::Cell, path::Path, rc::Rc};

use slint::{Model, ModelRc, SharedString, VecModel};

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
    /// The decoder selected for a text preview. Empty for non-text previews.
    pub encoding: String,
    pub text_content: String,
    pub hex_summary: String,
    pub image_uri: String,
    /// Decoded RGBA8 pixels for private/family-mounted resources. The UI
    /// creates the Slint image on its own thread; the manager never passes a
    /// path or native image handle across the view-model boundary.
    pub image_pixels: Vec<u8>,
    pub image_width: u32,
    pub image_height: u32,
    /// Bounded, payload-free summary for an explicitly bound audio/video
    /// decoder. The UI never receives PCM, frame buffers, or provider handles.
    pub media_summary: String,
    pub diagnostic: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemMenuItemViewModel {
    pub item_id: String,
    pub parent_id: String,
    pub label: String,
    pub order: i32,
    pub depth: i32,
    pub enabled: bool,
    pub checked: bool,
    pub separator: bool,
    pub submenu: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemMenuNavigation {
    None,
    Select(String),
    OpenSubmenu(String),
    Back,
    Dismiss,
}

fn reduce_system_menu_navigation(
    parent: &str,
    items: &[SystemMenuItem],
    current_focus: usize,
    control: &str,
) -> Result<(usize, SystemMenuNavigation), String> {
    if control == "arrow_left" {
        return Ok((
            current_focus,
            if parent.is_empty() {
                SystemMenuNavigation::None
            } else {
                SystemMenuNavigation::Back
            },
        ));
    }
    if control == "escape" {
        return Ok((
            current_focus,
            if parent.is_empty() {
                SystemMenuNavigation::Dismiss
            } else {
                SystemMenuNavigation::Back
            },
        ));
    }
    if items.is_empty() {
        return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_EMPTY".to_owned());
    }
    let mut focus = current_focus.min(items.len() - 1);
    let action = match control {
        "arrow_up" => {
            focus = (focus + items.len() - 1) % items.len();
            SystemMenuNavigation::None
        }
        "arrow_down" => {
            focus = (focus + 1) % items.len();
            SystemMenuNavigation::None
        }
        "arrow_right" => {
            let item = &items[focus];
            if item.enabled && item.submenu {
                SystemMenuNavigation::OpenSubmenu(item.item_id.to_string())
            } else {
                SystemMenuNavigation::None
            }
        }
        "enter" => {
            let item = &items[focus];
            if !item.enabled {
                // Win32 closes the popup when Enter is pressed on a
                // disabled row, without dispatching that row's command.
                SystemMenuNavigation::Dismiss
            } else if item.submenu {
                SystemMenuNavigation::OpenSubmenu(item.item_id.to_string())
            } else {
                SystemMenuNavigation::Select(item.item_id.to_string())
            }
        }
        "space" => {
            let item = &items[focus];
            if !item.enabled {
                // Space does not activate a disabled row and leaves the
                // transaction open for subsequent navigation.
                SystemMenuNavigation::None
            } else if item.submenu {
                SystemMenuNavigation::OpenSubmenu(item.item_id.to_string())
            } else {
                SystemMenuNavigation::Select(item.item_id.to_string())
            }
        }
        _ => return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_INPUT_UNSUPPORTED".to_owned()),
    };
    Ok((focus, action))
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
    pub filter_preset: String,
    pub diagnostics_summary: String,
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
    /// VNDB game/version identity: `vID / rID` (VNDB is the authoritative
    /// source for compatibility).
    pub selected_compatibility_vndb_id: String,
    /// Selectable VNDB releases (rID + human label) used to pin the local
    /// installation to a specific game version. `(release_id, label)`.
    pub selected_releases: Vec<(String, String)>,
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
    releases: Rc<VecModel<ReleaseOption>>,
    gamepad_bindings: Rc<VecModel<GamepadBinding>>,
    system_menu_items: Rc<VecModel<SystemMenuItem>>,
    system_menu_focus: Cell<usize>,
}

impl SlintManagerAdapter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = ManagerWindow::new()?;
        let games = Rc::new(VecModel::default());
        let reviews = Rc::new(VecModel::default());
        let vfs_entries = Rc::new(VecModel::default());
        let play_history = Rc::new(VecModel::default());
        let releases = Rc::new(VecModel::default());
        let gamepad_bindings = Rc::new(VecModel::default());
        let system_menu_items = Rc::new(VecModel::default());
        window.set_games(ModelRc::from(games.clone()));
        window.set_match_reviews(ModelRc::from(reviews.clone()));
        window.set_vfs_entries(ModelRc::from(vfs_entries.clone()));
        window.set_play_history(ModelRc::from(play_history.clone()));
        window.set_releases(ModelRc::from(releases.clone()));
        window.set_gamepad_bindings(ModelRc::from(gamepad_bindings.clone()));
        window.set_system_menu_items(ModelRc::from(system_menu_items.clone()));
        Ok(Self {
            window,
            games,
            reviews,
            vfs_entries,
            play_history,
            releases,
            gamepad_bindings,
            system_menu_items,
            system_menu_focus: Cell::new(0),
        })
    }

    pub fn show_system_menu(
        &self,
        menu_id: &str,
        items: &[SystemMenuItemViewModel],
        pointer_x: i32,
        pointer_y: i32,
    ) {
        self.system_menu_items.set_vec(
            items
                .iter()
                .map(|item| SystemMenuItem {
                    item_id: item.item_id.as_str().into(),
                    parent_id: item.parent_id.as_str().into(),
                    label: item.label.as_str().into(),
                    order: item.order,
                    depth: item.depth,
                    enabled: item.enabled,
                    checked: item.checked,
                    separator: item.separator,
                    submenu: item.submenu,
                })
                .collect::<Vec<_>>(),
        );
        self.window.set_system_menu_parent_id("".into());
        self.system_menu_focus.set(0);
        self.window.set_system_menu_x(pointer_x as f32);
        self.window.set_system_menu_y(pointer_y as f32);
        self.window.set_system_menu_id(menu_id.into());
        self.window.set_system_menu_active(true);
        self.set_system_menu_focus_for_current_parent();
    }

    pub fn hide_system_menu(&self) {
        self.window.set_system_menu_active(false);
        self.window.set_system_menu_id("".into());
        self.window.set_system_menu_parent_id("".into());
        self.window.set_system_menu_focus_id("".into());
        self.system_menu_focus.set(0);
        self.system_menu_items.set_vec(Vec::new());
    }

    /// Move the Manager's host-owned menu into an enabled submenu.  The
    /// Family transaction has already been validated before it reaches this
    /// adapter, but the UI transition still checks the current parent and
    /// item kind so a stale callback cannot expose an unrelated branch.
    pub fn open_system_menu_submenu(&self, item_id: &str) -> Result<(), String> {
        if !self.window.get_system_menu_active() {
            return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_NOT_ACTIVE".to_owned());
        }
        let current_parent = self.window.get_system_menu_parent_id().to_string();
        let item = self
            .menu_item(item_id)
            .ok_or_else(|| "ASTRA_EMU_MANAGER_SYSTEM_MENU_ITEM_UNKNOWN".to_owned())?;
        if !item.submenu || !item.enabled || item.parent_id.as_str() != current_parent {
            return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_SUBMENU_INVALID".to_owned());
        }
        self.window.set_system_menu_parent_id(item.item_id);
        self.system_menu_focus.set(0);
        self.set_system_menu_focus_for_current_parent();
        Ok(())
    }

    /// Return from the current submenu to its validated parent.  The root
    /// menu is represented by an empty parent id and cannot move further up.
    pub fn back_system_menu(&self) -> Result<(), String> {
        if !self.window.get_system_menu_active() {
            return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_NOT_ACTIVE".to_owned());
        }
        let current_parent = self.window.get_system_menu_parent_id().to_string();
        if current_parent.is_empty() {
            return Ok(());
        }
        let item = self
            .menu_item(&current_parent)
            .ok_or_else(|| "ASTRA_EMU_MANAGER_SYSTEM_MENU_PARENT_UNKNOWN".to_owned())?;
        if !item.submenu {
            return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_PARENT_INVALID".to_owned());
        }
        self.window.set_system_menu_parent_id(item.parent_id);
        self.system_menu_focus.set(0);
        self.set_system_menu_focus_for_current_parent();
        Ok(())
    }

    /// Apply one physical keyboard navigation operation to the active menu.
    /// The returned action is consumed by the Manager Host, which is the only
    /// layer allowed to resolve the Family transaction or dismiss it.
    pub fn navigate_system_menu(&self, control: &str) -> Result<SystemMenuNavigation, String> {
        if !self.window.get_system_menu_active() {
            return Err("ASTRA_EMU_MANAGER_SYSTEM_MENU_NOT_ACTIVE".to_owned());
        }
        let parent = self.window.get_system_menu_parent_id().to_string();
        let items = self.current_system_menu_items();
        let (focus, action) =
            reduce_system_menu_navigation(&parent, &items, self.system_menu_focus.get(), control)?;
        self.system_menu_focus.set(focus);
        self.window.set_system_menu_focus_id(
            items
                .get(focus)
                .map_or_else(|| "".into(), |item| item.item_id.clone()),
        );
        Ok(action)
    }

    fn current_system_menu_items(&self) -> Vec<SystemMenuItem> {
        let current_parent = self.window.get_system_menu_parent_id().to_string();
        let mut items = (0..self.system_menu_items.row_count())
            .filter_map(|row| self.system_menu_items.row_data(row))
            .filter(|item| item.parent_id.as_str() == current_parent && !item.separator)
            .collect::<Vec<_>>();
        items.sort_by_key(|item| item.order);
        items
    }

    fn set_system_menu_focus_for_current_parent(&self) {
        let item = self.current_system_menu_items().into_iter().next();
        self.window
            .set_system_menu_focus_id(item.map_or_else(|| "".into(), |item| item.item_id));
    }

    fn menu_item(&self, item_id: &str) -> Option<SystemMenuItem> {
        (0..self.system_menu_items.row_count()).find_map(|row| {
            self.system_menu_items
                .row_data(row)
                .filter(|item| item.item_id.as_str() == item_id)
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
        self.window.set_selected_compatibility_updated(
            model.selected_compatibility_updated.as_str().into(),
        );
        self.window.set_selected_compatibility_vndb_id(
            model.selected_compatibility_vndb_id.as_str().into(),
        );
        self.releases.set_vec(
            model
                .selected_releases
                .iter()
                .map(|(release_id, label)| ReleaseOption {
                    release_id: release_id.as_str().into(),
                    label: label.as_str().into(),
                })
                .collect::<Vec<_>>(),
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
            .set_filter_preset(model.filter_preset.as_str().into());
        self.window
            .set_diagnostics_summary(model.diagnostics_summary.as_str().into());
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
                    encoding: preview.encoding.as_str().into(),
                    text_content: preview.text_content.as_str().into(),
                    hex_summary: preview.hex_summary.as_str().into(),
                    image_data: if !preview.image_pixels.is_empty()
                        && preview.image_width != 0
                        && preview.image_height != 0
                    {
                        let buffer =
                            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                                &preview.image_pixels,
                                preview.image_width,
                                preview.image_height,
                            );
                        slint::Image::from_rgba8(buffer)
                    } else if preview.image_uri.is_empty() {
                        slint::Image::default()
                    } else {
                        slint::Image::load_from_path(Path::new(&preview.image_uri))
                            .unwrap_or_default()
                    },
                    media_summary: preview.media_summary.as_str().into(),
                    diagnostic: preview.diagnostic.as_str().into(),
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
        reduce_system_menu_navigation, AppearanceViewModel, GameCardViewModel,
        InputConfigViewModel, ManagerViewModel, MatchReviewViewModel, PlaySessionViewModel,
        SystemMenuItem, SystemMenuItemViewModel, SystemMenuNavigation, VfsEntryViewModel,
        VfsPreviewViewModel,
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
        assert_contract_is_send_sync::<SystemMenuItemViewModel>();
    }

    #[test]
    fn system_menu_navigation_keeps_submenus_host_owned() {
        let root_items = vec![SystemMenuItem {
            item_id: "game".into(),
            parent_id: "".into(),
            label: "Game".into(),
            order: 0,
            depth: 0,
            enabled: true,
            checked: false,
            separator: false,
            submenu: true,
        }];
        assert_eq!(
            reduce_system_menu_navigation("", &root_items, 0, "enter")
                .unwrap()
                .1,
            SystemMenuNavigation::OpenSubmenu("game".into())
        );
        let child_items = vec![SystemMenuItem {
            item_id: "exit".into(),
            parent_id: "game".into(),
            label: "Exit".into(),
            order: 0,
            depth: 1,
            enabled: true,
            checked: false,
            separator: false,
            submenu: false,
        }];
        assert_eq!(
            reduce_system_menu_navigation("game", &child_items, 0, "escape")
                .unwrap()
                .1,
            SystemMenuNavigation::Back
        );
        assert!(matches!(
            reduce_system_menu_navigation("", &root_items, 0, "escape")
                .unwrap()
                .1,
            SystemMenuNavigation::Dismiss
        ));
        assert_eq!(
            reduce_system_menu_navigation("", &root_items, 0, "unknown").unwrap_err(),
            "ASTRA_EMU_MANAGER_SYSTEM_MENU_INPUT_UNSUPPORTED"
        );
        assert_eq!(
            reduce_system_menu_navigation("", &[], 0, "arrow_down").unwrap_err(),
            "ASTRA_EMU_MANAGER_SYSTEM_MENU_EMPTY"
        );
        assert_eq!(
            reduce_system_menu_navigation("", &[], 0, "arrow_left")
                .unwrap()
                .1,
            SystemMenuNavigation::None
        );
    }

    #[test]
    fn system_menu_navigation_focuses_disabled_rows_without_selecting_them() {
        let items = vec![
            SystemMenuItem {
                item_id: "fullscreen".into(),
                parent_id: "".into(),
                label: "Fullscreen".into(),
                order: 0,
                depth: 0,
                enabled: true,
                checked: false,
                separator: false,
                submenu: false,
            },
            SystemMenuItem {
                item_id: "precision".into(),
                parent_id: "".into(),
                label: "Precision".into(),
                order: 1,
                depth: 0,
                enabled: false,
                checked: true,
                separator: false,
                submenu: false,
            },
            SystemMenuItem {
                item_id: "help".into(),
                parent_id: "".into(),
                label: "Help".into(),
                order: 2,
                depth: 0,
                enabled: true,
                checked: false,
                separator: false,
                submenu: true,
            },
        ];

        let (focus, action) = reduce_system_menu_navigation("", &items, 0, "arrow_down").unwrap();
        assert_eq!(focus, 1);
        assert_eq!(action, SystemMenuNavigation::None);
        assert_eq!(
            reduce_system_menu_navigation("", &items, focus, "enter")
                .unwrap()
                .1,
            SystemMenuNavigation::Dismiss
        );
        assert_eq!(
            reduce_system_menu_navigation("", &items, focus, "space")
                .unwrap()
                .1,
            SystemMenuNavigation::None
        );
        assert_eq!(
            reduce_system_menu_navigation("", &items, 2, "enter")
                .unwrap()
                .1,
            SystemMenuNavigation::OpenSubmenu("help".into())
        );
    }
}
