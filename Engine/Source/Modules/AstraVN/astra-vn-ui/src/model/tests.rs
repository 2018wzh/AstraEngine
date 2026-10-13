use super::vn_ui_model_context::{config_bool, config_integer, speaker_localization_key};
use super::{SystemPageUnderlayViewModel, SystemPageViewModel, VnUiPageModel};
use astra_ui_core::UiValue;
use std::collections::BTreeMap;

#[test]
fn config_binding_fields_match_the_declared_flat_schema_and_current_selection() {
    use super::{ConfigViewModel, ReadingMode, TextInputViewModel};
    for mode in [
        ReadingMode::Hidden,
        ReadingMode::Manual,
        ReadingMode::FastForward,
    ] {
        let value = VnUiPageModel::Config {
            config: ConfigViewModel {
                reading_mode: mode,
                audio_enabled: false,
                master_volume: 50,
                text_speed: 50,
                auto_delay_ms: 1200,
                high_contrast: false,
                locale: "en".into(),
                available_locales: vec!["en".into()],
                player_name: TextInputViewModel {
                    input_id: "name".into(),
                    value: String::new(),
                    multiline: false,
                    max_graphemes: 32,
                    character_policy: "single_line".into(),
                },
            },
        }
        .to_ui_value()
        .unwrap();
        let UiValue::Map(fields) = value else {
            panic!("config fields");
        };
        assert_eq!(fields["master_volume"], UiValue::Integer(50));
        assert!(!fields.contains_key("config"));
        assert_eq!(fields["audio_enabled"], UiValue::Bool(false));
        assert_eq!(fields["audio_disabled"], UiValue::Bool(true));
        for (field, expected) in [
            ("reading_hidden", ReadingMode::Hidden),
            ("reading_manual", ReadingMode::Manual),
            ("reading_fast_forward", ReadingMode::FastForward),
        ] {
            assert_eq!(fields[field], UiValue::Bool(mode == expected));
        }
    }
}

#[test]
fn config_values_are_schema_checked_instead_of_silently_clamped() {
    let values = BTreeMap::from([
        ("volume".to_owned(), "101".to_owned()),
        ("contrast".to_owned(), "enabled".to_owned()),
    ]);

    let range = config_integer(&values, "volume", 50, 0, 100).unwrap_err();
    assert_eq!(range.code(), "ASTRA_VN_UI_CONFIG_RANGE");

    let boolean = config_bool(&values, "contrast", false).unwrap_err();
    assert_eq!(boolean.code(), "ASTRA_VN_UI_CONFIG_BOOL");
}

#[test]
fn missing_config_values_use_declared_schema_defaults() {
    let values = BTreeMap::new();
    assert_eq!(config_integer(&values, "volume", 50, 0, 100).unwrap(), 50);
    assert!(config_bool(&values, "contrast", true).unwrap());
}

#[test]
fn speaker_ids_are_resolved_through_the_localization_namespace() {
    assert_eq!(
        speaker_localization_key(Some("tsui.speaker.fixture")),
        Some("speaker.tsui.speaker.fixture".to_string())
    );
    assert_eq!(speaker_localization_key(None), None);
}

#[test]
fn system_page_model_exposes_a_typed_reconstructible_underlay() {
    let value = SystemPageViewModel {
        page: VnUiPageModel::Title { can_continue: true },
        underlay: SystemPageUnderlayViewModel {
            kind: "message".into(),
            title: false,
            message: true,
            choice: false,
            text_key: Some("line.fixture".into()),
            speaker_key: Some("speaker.fixture".into()),
        },
    }
    .to_ui_value()
    .expect("system page model");
    let UiValue::Map(values) = value else {
        panic!("system page model must be a map");
    };
    assert_eq!(values["underlay_kind"], UiValue::String("message".into()));
    assert_eq!(values["underlay_message"], UiValue::Bool(true));
    assert_eq!(
        values["underlay_text_key"],
        UiValue::String("line.fixture".into())
    );
}
