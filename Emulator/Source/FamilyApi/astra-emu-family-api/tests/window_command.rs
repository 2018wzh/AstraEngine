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

#[test]
fn icon_dimensions_and_exact_rgba_length_are_checked_without_overflow() {
    use astra_emu_family_api::FamilyWindowIcon;
    FamilyWindowCommand::SetIcon(FamilyWindowIcon {
        width: 16,
        height: 16,
        rgba: vec![255; 1024].into(),
    })
    .validate()
    .unwrap();
    for (width, height, length) in [
        (0, 1, 0),
        (1, 0, 0),
        (257, 1, 1028),
        (1, 1, 3),
        (1, 1, 5),
        (u32::MAX, u32::MAX, 0),
    ] {
        let error = FamilyWindowCommand::SetIcon(FamilyWindowIcon {
            width,
            height,
            rgba: vec![0; length].into(),
        })
        .validate()
        .unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_FAMILY_WINDOW_ICON");
    }
}
