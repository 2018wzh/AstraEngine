use std::{cell::RefCell, rc::Rc};

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{
    covers, list_model::ListModel, notices, App, Appearance, AppearanceViewModel, BangumiProgress,
    BangumiViewModel, Choice, ChoiceViewModel, CompatInfo, ConfigField, ConfigFieldViewModel,
    CoreInfo, CoreViewModel, Cores, EngineAction, EngineActionViewModel, Excluded,
    ExcludedViewModel, Filters, GameCard, GameCardViewModel, GameSettings, GamepadBinding,
    GamepadBindingViewModel, Library, ManagerViewModel, ManagerWindow, MatchReview, MatchViewModel,
    Network, Notices, Onboarding, PatchState, PatchViewModel, PlaySession, PlaySessionViewModel,
    Recipe, RecipeViewModel, RenderState, RenderViewModel, SelectedGame, Selection, Session,
    Settings, TranslationForm, TranslationFormViewModel,
};

pub struct SlintManagerAdapter {
    window: ManagerWindow,
    games: Rc<covers::CoverModel>,
    notices: Rc<notices::NoticeModel>,
    emulator_choices: ListModel<ChoiceViewModel, Choice>,
    releases: ListModel<ChoiceViewModel, Choice>,
    history: ListModel<PlaySessionViewModel, PlaySession>,
    matches: ListModel<MatchViewModel, MatchReview>,
    game_emulator_choices: ListModel<ChoiceViewModel, Choice>,
    render_options: ListModel<ChoiceViewModel, Choice>,
    game_fields: Rc<VecModel<ConfigField>>,
    game_bindings: ListModel<GamepadBindingViewModel, GamepadBinding>,
    recipes: ListModel<RecipeViewModel, Recipe>,
    cores: ListModel<CoreViewModel, CoreInfo>,
    default_fields: Rc<VecModel<ConfigField>>,
    default_render_options: ListModel<ChoiceViewModel, Choice>,
    models: ListModel<String, SharedString>,
    bindings: ListModel<GamepadBindingViewModel, GamepadBinding>,
    pad_state: ListModel<String, SharedString>,
    roots: ListModel<String, SharedString>,
    excluded: ListModel<ExcludedViewModel, Excluded>,
    filter_fields: Rc<VecModel<ConfigField>>,
    actions: ListModel<EngineActionViewModel, EngineAction>,
    onboarding_roots: ListModel<String, SharedString>,
    cached_games: RefCell<Vec<GameCardViewModel>>,
    cached_game_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_default_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_filter_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_bangumi: RefCell<Option<(String, BangumiViewModel)>>,
    cached_translation: RefCell<Option<TranslationFormViewModel>>,
    cached_snackbar: RefCell<u32>,
    cached_launch: RefCell<String>,
}

impl SlintManagerAdapter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = ManagerWindow::new()?;
        let adapter = Self {
            games: Rc::new(covers::CoverModel::new(window.as_weak())?),
            notices: Rc::new(notices::NoticeModel::new()),
            emulator_choices: ListModel::new(choice),
            releases: ListModel::new(choice),
            history: ListModel::new(play_session),
            matches: ListModel::new(match_review),
            game_emulator_choices: ListModel::new(choice),
            render_options: ListModel::new(choice),
            game_fields: Rc::new(VecModel::default()),
            game_bindings: ListModel::new(binding),
            recipes: ListModel::new(recipe),
            cores: ListModel::new(core_info),
            default_fields: Rc::new(VecModel::default()),
            default_render_options: ListModel::new(choice),
            models: ListModel::new(text),
            bindings: ListModel::new(binding),
            pad_state: ListModel::new(text),
            roots: ListModel::new(text),
            excluded: ListModel::new(excluded),
            filter_fields: Rc::new(VecModel::default()),
            actions: ListModel::new(engine_action),
            onboarding_roots: ListModel::new(text),
            cached_games: RefCell::default(),
            cached_game_fields: RefCell::default(),
            cached_default_fields: RefCell::default(),
            cached_filter_fields: RefCell::default(),
            cached_bangumi: RefCell::default(),
            cached_translation: RefCell::default(),
            cached_snackbar: RefCell::new(0),
            cached_launch: RefCell::default(),
            window,
        };
        adapter.bind_models();
        adapter.install_internal_callbacks();
        Ok(adapter)
    }

    fn bind_models(&self) {
        let window = &self.window;
        window
            .global::<Library>()
            .set_games(ModelRc::from(self.games.clone()));
        window
            .global::<Notices>()
            .set_items(ModelRc::from(self.notices.clone()));

        let selection = window.global::<Selection>();
        selection.set_emulator_choices(self.emulator_choices.model());
        selection.set_releases(self.releases.model());
        selection.set_history(self.history.model());
        selection.set_matches(self.matches.model());

        let game_settings = window.global::<GameSettings>();
        game_settings.set_emulator_choices(self.game_emulator_choices.model());
        game_settings.set_render_options(self.render_options.model());
        game_settings.set_fields(ModelRc::from(self.game_fields.clone()));
        game_settings.set_bindings(self.game_bindings.model());
        game_settings.set_recipes(self.recipes.model());

        let cores = window.global::<Cores>();
        cores.set_items(self.cores.model());
        cores.set_default_fields(ModelRc::from(self.default_fields.clone()));

        let settings = window.global::<Settings>();
        settings.set_models(self.models.model());
        settings.set_bindings(self.bindings.model());
        settings.set_pad_state(self.pad_state.model());
        settings.set_roots(self.roots.model());
        settings.set_excluded(self.excluded.model());

        window
            .global::<Filters>()
            .set_fields(ModelRc::from(self.filter_fields.clone()));
        window.global::<Session>().set_actions(self.actions.model());
        window
            .global::<Onboarding>()
            .set_roots(self.onboarding_roots.model());
    }

    fn install_internal_callbacks(&self) {
        let cover_window = self.window.as_weak();
        self.window
            .global::<Library>()
            .on_cover_demand(move |row, case_id, needed, view| {
                if let Some(window) = cover_window.upgrade() {
                    let games = window.global::<Library>().get_games();
                    if let Some(covers) = games.as_any().downcast_ref::<covers::CoverModel>() {
                        covers.demand(row, case_id.as_str(), needed, view.as_str(), &window);
                    }
                }
            });

        let notice_window = self.window.as_weak();
        self.window.global::<Notices>().on_dismiss(move |id| {
            if let Some(window) = notice_window.upgrade() {
                let items = window.global::<Notices>().get_items();
                if let Some(model) = items.as_any().downcast_ref::<notices::NoticeModel>() {
                    model.dismiss(id.as_str());
                }
            }
        });
    }

    pub fn window(&self) -> &ManagerWindow {
        &self.window
    }

    pub fn set_session_active(&self, active: bool) {
        self.window.global::<Session>().set_active(active);
    }

    pub fn apply(&self, model: &ManagerViewModel) {
        self.apply_app(model);
        self.apply_library(model);
        self.apply_selection(model);
        self.apply_game_settings(model);
        self.apply_cores(model);
        self.apply_notices(model);
        self.apply_settings(model);
        self.apply_filters(model);
        self.apply_session(model);
        self.onboarding_roots.update(&model.onboarding.roots);
    }

    fn apply_app(&self, model: &ManagerViewModel) {
        let app = self.window.global::<App>();
        app.set_version(model.app.version.as_str().into());
        app.set_build(model.app.build.as_str().into());
        app.set_appearance(appearance(&model.app.appearance));

        let network = self.window.global::<Network>();
        network.set_allowed(model.network.allowed);
        network.set_syncing(model.network.syncing);
        network.set_sync_status(model.network.sync_status.as_str().into());
        network.set_source(model.network.source.as_str().into());
    }

    fn apply_library(&self, model: &ManagerViewModel) {
        let library = self.window.global::<Library>();
        let view = &model.library;
        library.set_query(view.query.as_str().into());
        library.set_sort(view.sort.as_str().into());
        library.set_compat_filter(view.compat_filter.as_str().into());
        library.set_prepare_state(view.prepare_state.as_str().into());
        library.set_prepare_label(view.prepare_label.as_str().into());
        library.set_prepare_detail(view.prepare_detail.as_str().into());
        library.set_prepare_progress(view.prepare_progress.unwrap_or(-1.0));
        library.set_can_cancel_prepare(view.can_cancel_prepare);
        library.set_selected_id(view.selected_id.as_str().into());

        if *self.cached_games.borrow() != view.games {
            let cards = view
                .games
                .iter()
                .map(|game| (game_card(game), game.cover_uri.clone()))
                .collect::<Vec<_>>();
            self.games.replace(cards, &self.window);
            view.games.clone_into(&mut self.cached_games.borrow_mut());
        } else {
            self.games.refresh(&self.window);
        }
    }

    fn apply_selection(&self, model: &ManagerViewModel) {
        let selection = self.window.global::<Selection>();
        let view = &model.selection;
        let game = &view.game;
        selection.set_game(SelectedGame {
            id: game.id.as_str().into(),
            title: game.title.as_str().into(),
            engine: game.engine.as_str().into(),
            emulator: game.emulator.as_str().into(),
            emulator_id: game.emulator_id.as_str().into(),
            launchable: game.launchable,
            problem: game.problem.as_str().into(),
            play_time: game.play_time.as_str().into(),
            last_played: game.last_played.as_str().into(),
            developer: game.developer.as_str().into(),
            release_date: game.release_date.as_str().into(),
            platforms: game.platforms.as_str().into(),
            description: game.description.as_str().into(),
            tags: game.tags.as_str().into(),
            aliases: game.aliases.as_str().into(),
        });
        self.emulator_choices.update(&view.emulator_choices);
        let compat = &view.compat;
        selection.set_compat(CompatInfo {
            status: compat.status.as_str().into(),
            notes: compat.notes.as_str().into(),
            updated: compat.updated.as_str().into(),
            release_id: compat.release_id.as_str().into(),
            emulator_label: compat.emulator_label.as_str().into(),
            report_url: compat.report_url.as_str().into(),
            hint: compat.hint.as_str().into(),
        });
        self.releases.update(&view.releases);
        selection.set_pinned_release(view.pinned_release.as_str().into());
        self.history.update(&view.history);
        let bangumi_key = (game.id.clone(), view.bangumi.clone());
        if self.cached_bangumi.borrow().as_ref() != Some(&bangumi_key) {
            selection.set_bangumi(bangumi(&view.bangumi));
            *self.cached_bangumi.borrow_mut() = Some(bangumi_key);
        }
        selection.set_bangumi_summary(view.bangumi_summary.as_str().into());
        selection.set_metadata_busy(view.metadata_busy);
        selection.set_metadata_status(view.metadata_status.as_str().into());
        self.matches.update(&view.matches);
    }

    fn apply_game_settings(&self, model: &ManagerViewModel) {
        let settings = self.window.global::<GameSettings>();
        let view = &model.game_settings;
        settings.set_game_id(view.game_id.as_str().into());
        settings.set_title(view.title.as_str().into());
        self.game_emulator_choices.update(&view.emulator_choices);
        settings.set_emulator_id(view.emulator_id.as_str().into());
        let render = &view.render;
        settings.set_render_supported(render.supported);
        self.render_options.update(&render.options);
        settings.set_render_mode(render.mode.as_str().into());
        settings.set_custom_width(render.custom_width);
        settings.set_custom_height(render.custom_height);
        settings.set_render_min(render.min.as_str().into());
        settings.set_render_max(render.max.as_str().into());
        settings.set_render_note(render.note.as_str().into());
        settings.set_render_error(render.error.as_str().into());
        crate::config_model::update(&self.game_fields, &self.cached_game_fields, &view.fields);
        settings.set_recommended_available(view.recommended_available);
        settings.set_recommended_summary(view.recommended_summary.as_str().into());
        settings.set_input_override(view.input_override);
        self.game_bindings.update(&view.bindings);
        self.recipes.update(&view.recipes);
        settings.set_patch(patch_state(&view.patch));
    }

    fn apply_cores(&self, model: &ManagerViewModel) {
        let cores = self.window.global::<Cores>();
        let view = &model.cores;
        self.cores.update(&view.items);
        cores.set_restart_required(view.restart_required);
        cores.set_updates_available(view.updates_available);
        crate::config_model::update(
            &self.default_fields,
            &self.cached_default_fields,
            &view.default_fields,
        );
        cores.set_default_plugin(view.default_plugin.as_str().into());
        self.default_render_options
            .update(&view.default_render.options);
        cores.set_default_render(render_state(
            &view.default_render,
            self.default_render_options.model(),
        ));
    }

    fn apply_notices(&self, model: &ManagerViewModel) {
        self.notices.set_model(model.notices.items.clone());
        let snackbar = &model.notices.snackbar;
        if *self.cached_snackbar.borrow() != snackbar.serial {
            *self.cached_snackbar.borrow_mut() = snackbar.serial;
            let notices = self.window.global::<Notices>();
            notices.set_snackbar_text(snackbar.text.as_str().into());
            notices.set_snackbar_action(snackbar.action.as_str().into());
            notices.set_snackbar_serial(notices.get_snackbar_serial().wrapping_add(1));
        }
    }

    fn apply_settings(&self, model: &ManagerViewModel) {
        let settings = self.window.global::<Settings>();
        let view = &model.settings;
        if self.cached_translation.borrow().as_ref() != Some(&view.translation) {
            settings.set_translation(translation_form(&view.translation));
            *self.cached_translation.borrow_mut() = Some(view.translation.clone());
        }
        settings.set_translation_enabled(view.translation_enabled);
        settings.set_secret_saved(view.secret_saved);
        self.models.update(&view.models);
        settings.set_translation_status(view.translation_status.as_str().into());
        settings.set_sensitive_covers(view.sensitive_covers);
        settings.set_bangumi_token_saved(view.bangumi_token_saved);
        settings.set_compat_summary(view.compat_summary.as_str().into());
        settings.set_gamepad_enabled(view.gamepad_enabled);
        settings.set_deadzone(view.deadzone.as_str().into());
        self.bindings.update(&view.bindings);
        self.pad_state.update(&view.pad_state);
        self.roots.update(&view.roots);
        self.excluded.update(&view.excluded);
    }

    fn apply_filters(&self, model: &ManagerViewModel) {
        self.window
            .global::<Filters>()
            .set_preset(model.filters.preset.as_str().into());
        crate::config_model::update(
            &self.filter_fields,
            &self.cached_filter_fields,
            &model.filters.fields,
        );
    }

    fn apply_session(&self, model: &ManagerViewModel) {
        let session = self.window.global::<Session>();
        let view = &model.session;
        session.set_phase(view.phase.as_str().into());
        session.set_launch_title(view.launch_title.as_str().into());
        if *self.cached_launch.borrow() != view.launch_game_id {
            let (cover, _) = self
                .games
                .cover_of(view.launch_game_id.as_str())
                .unwrap_or_default();
            session.set_launch_cover(cover);
            view.launch_game_id
                .clone_into(&mut self.cached_launch.borrow_mut());
        }
        self.actions.update(&view.actions);
        session.set_action_message(view.action_message.as_str().into());
        session.set_action_serial(view.action_serial as i32);
    }
}

fn text(value: &String) -> SharedString {
    value.as_str().into()
}

fn strings(values: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(values.iter().map(text).collect::<Vec<_>>()))
}

fn choice(value: &ChoiceViewModel) -> Choice {
    Choice {
        id: value.id.as_str().into(),
        label: value.label.as_str().into(),
    }
}

fn appearance(value: &AppearanceViewModel) -> Appearance {
    Appearance {
        theme_mode: value.theme_mode.as_str().into(),
        density: value.density.as_str().into(),
        cover_size: value.cover_size.as_str().into(),
        view_mode: value.view_mode.as_str().into(),
        reduce_motion: value.reduce_motion,
        cover_badges: value.cover_badges,
    }
}

fn game_card(value: &GameCardViewModel) -> GameCard {
    GameCard {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        subtitle: value.subtitle.as_str().into(),
        cover: slint::Image::default(),
        tint: slint::Color::default(),
        ready: value.ready,
        problem: value.problem.as_str().into(),
        play_time: value.play_time.as_str().into(),
        last_played: value.last_played.as_str().into(),
        compat: value.compat.as_str().into(),
    }
}

fn play_session(value: &PlaySessionViewModel) -> PlaySession {
    PlaySession {
        started: value.started.as_str().into(),
        duration: value.duration.as_str().into(),
        ended_by: value.ended_by.as_str().into(),
    }
}

fn match_review(value: &MatchViewModel) -> MatchReview {
    MatchReview {
        id: value.id.as_str().into(),
        provider: value.provider.as_str().into(),
        title: value.title.as_str().into(),
        aliases: value.aliases.as_str().into(),
        release_date: value.release_date.as_str().into(),
        developer: value.developer.as_str().into(),
    }
}

fn bangumi(value: &BangumiViewModel) -> BangumiProgress {
    BangumiProgress {
        status: value.status.as_str().into(),
        rating: value.rating,
        note: value.note.as_str().into(),
    }
}

fn binding(value: &GamepadBindingViewModel) -> GamepadBinding {
    GamepadBinding {
        id: value.id.as_str().into(),
        label: value.label.as_str().into(),
        key: value.key.as_str().into(),
    }
}

fn recipe(value: &RecipeViewModel) -> Recipe {
    Recipe {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        recommended: value.recommended,
    }
}

fn patch_state(value: &PatchViewModel) -> PatchState {
    PatchState {
        step: value.step,
        running: value.running,
        phase: value.phase.as_str().into(),
        completed: value.completed as f32,
        total: value.total.map_or(-1.0, |total| total as f32),
        output: value.output.as_str().into(),
        error: value.error.as_str().into(),
        can_start: value.can_start,
        script: value.script.as_str().into(),
        patch_directory: value.patch_directory.as_str().into(),
        output_directory: value.output_directory.as_str().into(),
    }
}

fn render_state(value: &RenderViewModel, options: ModelRc<Choice>) -> RenderState {
    RenderState {
        supported: value.supported,
        options,
        mode: value.mode.as_str().into(),
        custom_width: value.custom_width,
        custom_height: value.custom_height,
        min: value.min.as_str().into(),
        max: value.max.as_str().into(),
        note: value.note.as_str().into(),
        error: value.error.as_str().into(),
    }
}

fn core_info(value: &CoreViewModel) -> CoreInfo {
    CoreInfo {
        id: value.id.as_str().into(),
        name: value.name.as_str().into(),
        engine: value.engine.as_str().into(),
        version: value.version.as_str().into(),
        state: value.state.as_str().into(),
        source: value.source.as_str().into(),
        message: value.message.as_str().into(),
        capabilities: strings(&value.capabilities),
        removable: value.removable,
        update_version: value.update_version.as_str().into(),
    }
}

fn translation_form(value: &TranslationFormViewModel) -> TranslationForm {
    TranslationForm {
        provider: value.provider.as_str().into(),
        base_url: value.base_url.as_str().into(),
        openai_api: value.openai_api.as_str().into(),
        model: value.model.as_str().into(),
        language: value.language.as_str().into(),
        timeout_s: value.timeout_s,
        custom_prompt: value.custom_prompt.as_str().into(),
        context_segments: value.context_segments,
        context_chars: value.context_chars,
    }
}

fn excluded(value: &ExcludedViewModel) -> Excluded {
    Excluded {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        location: value.location.as_str().into(),
    }
}

fn engine_action(value: &EngineActionViewModel) -> EngineAction {
    EngineAction {
        control: value.control,
        label: value.label.as_str().into(),
        icon: value.icon.as_str().into(),
    }
}
