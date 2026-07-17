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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

pub struct SlintManagerAdapter {
    window: ManagerWindow,
    games: Rc<VecModel<GameCard>>,
    reviews: Rc<VecModel<MatchReview>>,
}

impl SlintManagerAdapter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = ManagerWindow::new()?;
        let games = Rc::new(VecModel::default());
        let reviews = Rc::new(VecModel::default());
        window.set_games(ModelRc::from(games.clone()));
        window.set_match_reviews(ModelRc::from(reviews.clone()));
        Ok(Self {
            window,
            games,
            reviews,
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
    use super::{GameCardViewModel, ManagerViewModel, MatchReviewViewModel};

    fn assert_contract_is_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_view_models_do_not_require_ui_thread_types() {
        assert_contract_is_send_sync::<GameCardViewModel>();
        assert_contract_is_send_sync::<ManagerViewModel>();
        assert_contract_is_send_sync::<MatchReviewViewModel>();
    }
}
