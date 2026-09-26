use astra_emu_family_api::{FamilyWindowCommand, MAX_WINDOW_TITLE_BYTES};

#[test]
fn titles_are_bounded_utf8_and_empty_explicitly_clears() {
    for title in ["", "公開ゲーム", &"a".repeat(MAX_WINDOW_TITLE_BYTES)] {
        FamilyWindowCommand::SetTitle(title.into())
            .validate()
            .unwrap();
    }
    for title in [
        "secret\0caption".to_owned(),
        "line\nbreak".into(),
        "\u{0085}".into(),
        "界".repeat(1366),
    ] {
        let error = FamilyWindowCommand::SetTitle(title.into())
            .validate()
            .unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_FAMILY_WINDOW_TITLE");
        assert!(!error.message.contains("secret"));
    }
}
