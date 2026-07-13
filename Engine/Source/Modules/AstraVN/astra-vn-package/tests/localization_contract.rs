use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_vn_package::{
    load_localization, load_player_locale_config, PLAYER_LOCALE_CONFIG_SCHEMA,
    VN_LOCALIZATION_TABLE_SCHEMA,
};

fn package(localization: Vec<u8>, config: Vec<u8>) -> Vec<u8> {
    PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.localization",
        "classic",
        vec![
            SectionPayload::raw(
                "vn.localization.en",
                VN_LOCALIZATION_TABLE_SCHEMA,
                localization,
            ),
            SectionPayload::raw("player.locale_config", PLAYER_LOCALE_CONFIG_SCHEMA, config),
        ],
    ))
    .unwrap()
    .into_bytes()
}

#[test]
fn package_locale_config_validates_every_declared_localization() {
    let bytes = package(
        br#"{"schema":"astra.vn.localization_table.v1","locale":"en","strings":{"line.one":"Hello"}}"#.to_vec(),
        br#"{"schema":"astra.player_locale_config.v1","default_locale":"en","available_locales":["en"]}"#.to_vec(),
    );
    let reader = PackageReader::open(&bytes).unwrap();
    let config = load_player_locale_config(&reader).unwrap();
    assert_eq!(config.default_locale, "en");
    assert_eq!(
        load_localization(&reader, "en", 1024)
            .unwrap()
            .resolve("line.one")
            .unwrap(),
        "Hello"
    );
}

#[test]
fn localization_duplicate_keys_and_locale_order_are_blocking() {
    let duplicate = package(
        br#"{"schema":"astra.vn.localization_table.v1","locale":"en","strings":{"line.one":"A","line.one":"B"}}"#.to_vec(),
        br#"{"schema":"astra.player_locale_config.v1","default_locale":"en","available_locales":["en"]}"#.to_vec(),
    );
    let reader = PackageReader::open(&duplicate).unwrap();
    assert!(load_player_locale_config(&reader)
        .unwrap_err()
        .to_string()
        .contains("duplicate localization key"));

    let unordered = package(
        br#"{"schema":"astra.vn.localization_table.v1","locale":"en","strings":{"line.one":"A"}}"#.to_vec(),
        br#"{"schema":"astra.player_locale_config.v1","default_locale":"en","available_locales":["zh-Hans","en"]}"#.to_vec(),
    );
    let reader = PackageReader::open(&unordered).unwrap();
    assert!(load_player_locale_config(&reader)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_LOCALE_ORDER"));
}
