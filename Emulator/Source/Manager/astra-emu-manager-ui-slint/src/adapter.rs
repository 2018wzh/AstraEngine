use std::{cell::RefCell, rc::Rc};

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{
    covers, notices, App, Appearance, AppearanceViewModel, BangumiProgress, BangumiViewModel,
    CompatInfo, ConfigFieldViewModel, CoreInfo, Cores, EngineAction, FilePatch, Filters, GameCard,
    GamepadBinding, Library, ManagerViewModel, ManagerWindow, MatchReview, Notices, PlaySession,
    ReleaseOption, SelectedGame, Selection, Session, Settings, TranslationProfile,
};

type GameCardSignature = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

pub struct SlintManagerAdapter {
    window: ManagerWindow,
    games: Rc<covers::CoverModel>,
    notices: Rc<notices::NoticeModel>,
    releases: Rc<VecModel<ReleaseOption>>,
    history: Rc<VecModel<PlaySession>>,
    matches: Rc<VecModel<MatchReview>>,
    family_fields: Rc<VecModel<crate::ConfigField>>,
    cores: Rc<VecModel<CoreInfo>>,
    bindings: Rc<VecModel<GamepadBinding>>,
    filter_fields: Rc<VecModel<crate::ConfigField>>,
    actions: Rc<VecModel<EngineAction>>,
    cached_games_signature: RefCell<Vec<GameCardSignature>>,
    cached_family_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_filter_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_bangumi: RefCell<Option<(String, String, i32, String)>>,
}

impl SlintManagerAdapter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = ManagerWindow::new()?;
        let games = Rc::new(covers::CoverModel::new(window.as_weak())?);
        let notices = Rc::new(notices::NoticeModel::new());
        let releases = Rc::new(VecModel::default());
        let history = Rc::new(VecModel::default());
        let matches = Rc::new(VecModel::default());
        let family_fields = Rc::new(VecModel::default());
        let cores = Rc::new(VecModel::default());
        let bindings = Rc::new(VecModel::default());
        let filter_fields = Rc::new(VecModel::default());
        let actions = Rc::new(VecModel::default());

        window
            .global::<Library>()
            .set_games(ModelRc::from(games.clone()));
        window
            .global::<Notices>()
            .set_items(ModelRc::from(notices.clone()));
        window
            .global::<Selection>()
            .set_releases(ModelRc::from(releases.clone()));
        window
            .global::<Selection>()
            .set_history(ModelRc::from(history.clone()));
        window
            .global::<Selection>()
            .set_matches(ModelRc::from(matches.clone()));
        window
            .global::<Selection>()
            .set_family_fields(ModelRc::from(family_fields.clone()));
        window
            .global::<Cores>()
            .set_items(ModelRc::from(cores.clone()));
        window
            .global::<Settings>()
            .set_bindings(ModelRc::from(bindings.clone()));
        window
            .global::<Filters>()
            .set_fields(ModelRc::from(filter_fields.clone()));
        window
            .global::<Session>()
            .set_actions(ModelRc::from(actions.clone()));

        let cover_window = window.as_weak();
        window
            .global::<Library>()
            .on_cover_demand(move |row, case_id, needed, view| {
                if let Some(window) = cover_window.upgrade() {
                    let games = window.global::<Library>().get_games();
                    if let Some(covers) = games.as_any().downcast_ref::<covers::CoverModel>() {
                        covers.demand(row, case_id.as_str(), needed, view.as_str(), &window);
                    }
                }
            });

        let notice_window = window.as_weak();
        window.global::<Notices>().on_dismiss(move |id| {
            if let Some(window) = notice_window.upgrade() {
                let items = window.global::<Notices>().get_items();
                if let Some(model) = items.as_any().downcast_ref::<notices::NoticeModel>() {
                    model.dismiss(id.as_str());
                }
            }
        });

        Ok(Self {
            window,
            games,
            notices,
            releases,
            history,
            matches,
            family_fields,
            cores,
            bindings,
            filter_fields,
            actions,
            cached_games_signature: RefCell::new(Vec::new()),
            cached_family_fields: RefCell::default(),
            cached_filter_fields: RefCell::default(),
            cached_bangumi: RefCell::new(None),
        })
    }

    pub fn apply(&self, model: &ManagerViewModel) {
        let app = self.window.global::<App>();
        app.set_version(model.about.version.as_str().into());
        app.set_build(model.about.build.as_str().into());
        app.set_appearance(appearance_from(&model.appearance));

        let library = self.window.global::<Library>();
        library.set_query(model.library.query.as_str().into());
        library.set_sort(model.library.sort.as_str().into());
        library.set_compat_filter(model.library.compat_filter.as_str().into());
        library.set_cores_ready(model.library.cores_ready);
        library.set_can_load_cores(model.library.can_load_cores);
        library.set_busy(model.library.busy);
        library.set_status(model.library.status.as_str().into());
        library.set_selected_id(model.library.selected_id.as_str().into());
        self.apply_games(model);

        self.apply_selection(model);
        self.apply_cores(model);
        self.apply_settings(model);
        let patch = self.window.global::<FilePatch>();
        patch.set_running(model.patch.running);
        patch.set_can_start(model.patch.can_start);
        patch.set_phase(model.patch.phase.as_str().into());
        patch.set_completed(model.patch.completed.to_string().into());
        patch.set_total(
            model
                .patch
                .total
                .map(|total| total.to_string())
                .unwrap_or_default()
                .into(),
        );
        patch.set_output(model.patch.output.as_str().into());
        patch.set_error(model.patch.error.as_str().into());
        self.notices.set_model(model.notices.clone());

        let session = self.window.global::<Session>();
        session.set_action_message(model.session.action_message.as_str().into());
        session.set_action_serial(model.session.action_serial as i32);
        self.actions.set_vec(
            model
                .session
                .actions
                .iter()
                .map(|action| EngineAction {
                    control: action.control,
                    label: action.label.as_str().into(),
                    icon: action.icon.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
    }

    fn apply_games(&self, model: &ManagerViewModel) {
        let current_signature: Vec<GameCardSignature> = model
            .library
            .games
            .iter()
            .map(|game| {
                (
                    game.id.clone(),
                    game.title.clone(),
                    game.family.clone(),
                    game.cover_uri.clone(),
                    game.problem.clone(),
                    game.play_time.clone(),
                    game.last_played.clone(),
                    game.compat.clone(),
                )
            })
            .collect();
        if *self.cached_games_signature.borrow() != current_signature {
            let cards = model
                .library
                .games
                .iter()
                .map(|game| GameCard {
                    id: game.id.as_str().into(),
                    title: game.title.as_str().into(),
                    family: game.family.as_str().into(),
                    cover_uri: game.cover_uri.as_str().into(),
                    cover: slint::Image::default(),
                    tint: slint::Color::default(),
                    problem: game.problem.as_str().into(),
                    play_time: game.play_time.as_str().into(),
                    last_played: game.last_played.as_str().into(),
                    compat: game.compat.as_str().into(),
                })
                .collect::<Vec<_>>();
            self.games.replace(cards, &self.window);
            *self.cached_games_signature.borrow_mut() = current_signature;
        } else {
            self.games.refresh(&self.window);
        }
    }

    fn apply_selection(&self, model: &ManagerViewModel) {
        let selection = self.window.global::<Selection>();
        let game = &model.selection;
        selection.set_game(SelectedGame {
            id: game.id.as_str().into(),
            title: game.title.as_str().into(),
            family: game.family.as_str().into(),
            emulator: game.emulator.as_str().into(),
            launchable: game.launchable,
            problem: game.problem.as_str().into(),
            play_time: game.play_time.as_str().into(),
            last_played: game.last_played.as_str().into(),
            developer: game.developer.as_str().into(),
            release_date: game.release_date.as_str().into(),
            platforms: game.platforms.as_str().into(),
            engine: game.engine.as_str().into(),
            description: game.description.as_str().into(),
            tags: game.tags.as_str().into(),
            aliases: game.aliases.as_str().into(),
        });
        selection.set_compat(CompatInfo {
            status: game.compat.status.as_str().into(),
            notes: game.compat.notes.as_str().into(),
            updated: game.compat.updated.as_str().into(),
            vndb_id: game.compat.vndb_id.as_str().into(),
        });
        self.releases.set_vec(
            game.releases
                .iter()
                .map(|item| ReleaseOption {
                    id: item.id.as_str().into(),
                    label: item.label.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        self.history.set_vec(
            game.history
                .iter()
                .map(|item| PlaySession {
                    started: item.started.as_str().into(),
                    duration: item.duration.as_str().into(),
                    ended_by: item.ended_by.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        self.matches.set_vec(
            game.matches
                .iter()
                .map(|item| MatchReview {
                    id: item.id.as_str().into(),
                    provider: item.provider.as_str().into(),
                    title: item.title.as_str().into(),
                    aliases: item.aliases.as_str().into(),
                    release_date: item.release_date.as_str().into(),
                    developer: item.developer.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        selection.set_metadata_busy(game.metadata_busy);
        selection.set_metadata_status(game.metadata_status.as_str().into());
        let bangumi_key = (
            game.id.clone(),
            game.bangumi.status.clone(),
            game.bangumi.rating,
            game.bangumi.note.clone(),
        );
        if self.cached_bangumi.borrow().as_ref() != Some(&bangumi_key) {
            selection.set_bangumi(bangumi_from(&game.bangumi));
            *self.cached_bangumi.borrow_mut() = Some(bangumi_key);
        }
        selection.set_bangumi_summary(game.bangumi.summary.as_str().into());
        selection.set_musica_key_available(game.musica_key_available);
        crate::config_model::update(
            &self.family_fields,
            &self.cached_family_fields,
            &game.family_fields,
        );
    }

    fn apply_cores(&self, model: &ManagerViewModel) {
        self.window
            .global::<Cores>()
            .set_restart_required(model.cores.restart_required);
        self.cores.set_vec(
            model
                .cores
                .items
                .iter()
                .map(|core| CoreInfo {
                    key: core.key.as_str().into(),
                    name: core.name.as_str().into(),
                    monogram: core.monogram.as_str().into(),
                    version: core.version.as_str().into(),
                    capabilities: ModelRc::new(VecModel::from(
                        core.capabilities
                            .iter()
                            .map(|cap| SharedString::from(cap.as_str()))
                            .collect::<Vec<_>>(),
                    )),
                    state: core.state.as_str().into(),
                    source: core.source.as_str().into(),
                    file: core.file.as_str().into(),
                    message: core.message.as_str().into(),
                    removable: core.removable,
                })
                .collect::<Vec<_>>(),
        );
    }

    fn apply_settings(&self, model: &ManagerViewModel) {
        let settings = self.window.global::<Settings>();
        let translation = &model.settings.translation;
        settings.set_translation(TranslationProfile {
            provider: translation.provider.as_str().into(),
            base_url: translation.base_url.as_str().into(),
            openai_api: translation.openai_api.as_str().into(),
            model: translation.model.as_str().into(),
            language: translation.language.as_str().into(),
            timeout_ms: translation.timeout_ms,
            custom_prompt: translation.custom_prompt.as_str().into(),
            context_segments: translation.context_segments,
            context_chars: translation.context_chars,
        });
        settings.set_translation_enabled(model.settings.translation_enabled);
        settings.set_translation_status(model.settings.translation_status.as_str().into());
        let models = settings.get_models();
        if !models.iter().map(|value| value.to_string()).eq(model
            .settings
            .translation_models
            .iter()
            .cloned())
        {
            settings.set_models(ModelRc::new(VecModel::from(
                model
                    .settings
                    .translation_models
                    .iter()
                    .map(|value| SharedString::from(value.as_str()))
                    .collect::<Vec<_>>(),
            )));
        }
        settings.set_network_allowed(model.settings.network_allowed);
        settings.set_sensitive_covers(model.settings.sensitive_covers);
        settings.set_gamepad_enabled(model.settings.input.gamepad_enabled);
        settings.set_deadzone(model.settings.input.deadzone.as_str().into());
        self.bindings.set_vec(
            model
                .settings
                .input
                .bindings
                .iter()
                .map(|binding| GamepadBinding {
                    id: binding.id.as_str().into(),
                    label: binding.label.as_str().into(),
                    key: binding.key.as_str().into(),
                })
                .collect::<Vec<_>>(),
        );
        settings.set_compat_source(model.settings.compat_source.as_str().into());
        settings.set_compat_summary(model.settings.compat_summary.as_str().into());
        self.window
            .global::<Filters>()
            .set_preset(model.settings.filter_preset.as_str().into());
        crate::config_model::update(
            &self.filter_fields,
            &self.cached_filter_fields,
            &model.settings.filter_fields,
        );
    }

    pub fn window(&self) -> &ManagerWindow {
        &self.window
    }

    pub fn set_session_active(&self, active: bool) {
        self.window.global::<Session>().set_active(active);
    }
}

fn appearance_from(value: &AppearanceViewModel) -> Appearance {
    Appearance {
        theme_mode: value.theme_mode.as_str().into(),
        density: value.density.as_str().into(),
        cover_size: value.cover_size.as_str().into(),
        view_mode: value.view_mode.as_str().into(),
        reduce_motion: value.reduce_motion,
        cover_badges: value.cover_badges,
    }
}

fn bangumi_from(value: &BangumiViewModel) -> BangumiProgress {
    BangumiProgress {
        status: value.status.as_str().into(),
        rating: value.rating,
        note: value.note.as_str().into(),
    }
}
