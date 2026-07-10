use astra_vn_system::{SystemPageKind, SystemUiAction, SystemUiModel, SystemUiSurface};

#[test]
fn system_ui_model_covers_all_pages_and_choice_hit_testing() {
    for page in [
        SystemPageKind::Title,
        SystemPageKind::Save,
        SystemPageKind::Load,
        SystemPageKind::Config,
        SystemPageKind::Backlog,
        SystemPageKind::Gallery,
        SystemPageKind::Replay,
        SystemPageKind::VoiceReplay,
        SystemPageKind::RouteChart,
        SystemPageKind::LocalizationPreview,
    ] {
        let model = SystemUiModel::system(page, 1280, 720).unwrap();
        assert_eq!(model.schema, "astra.vn.system_ui_model.v1");
        assert!(!model.controls.is_empty());
    }

    let choices = SystemUiModel::choice(1280, 720, 2);
    assert_eq!(choices.surface, SystemUiSurface::Choice);
    assert_eq!(
        choices.hit_test(640.0, 340.0),
        Some(&SystemUiAction::ChooseIndex { index: 0 })
    );
    assert_eq!(
        choices.hit_test(640.0, 396.0),
        Some(&SystemUiAction::ChooseIndex { index: 1 })
    );
}
