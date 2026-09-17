use super::*;
use crate::{MusicaLaunchMode, MusicaSystemPage};
fn game(root: &std::path::Path) {
    fixture::game(
        root,
        b".setglobal custom = custom + 1\r\n.message 1   First\r\n.setglobal AYAME_CLEAR = 1\r\n.end\r\n",
    );
    let mut assets = Vec::new();
    for (name, w, h) in [
        ("saveloadBase.png", 1280, 720),
        ("saveloadLoad.png", 352, 48),
        ("saveloadSelect.png", 344, 98),
        ("saveloadButtons.png", 356, 48),
        ("notsaved.png", 106, 60),
    ] {
        assets.push((name.to_owned(), png(w, h, [20, 30, 40, 255])));
    }
    for page in 0..10 {
        assets.push((
            format!("saveload_Page{page}.png"),
            png(208, 48, [30, 40, 50, 255]),
        ));
    }
    for variant in 0..3 {
        assets.push((
            format!("topMenu{variant}.png"),
            png(1280, 720, [20 + variant * 20, 30, 40, 255]),
        ));
        assets.push((
            format!("topMenu{variant}Over.png"),
            png(1280, 720, [100, 150, 200, 255]),
        ));
    }
    fixture::assets(
        root,
        "sys",
        &assets
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect::<Vec<_>>(),
    );
}
fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba(color))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
fn title_request(root: &std::path::Path) -> OpenRequest {
    let mut req = request(root, Sink::default());
    req.configuration.push(ConfigEntry {
        id: "launch_mode".into(),
        value: ConfigValue::Enum("title".into()),
    });
    req
}
#[test]
fn native_gpu_title_starts_loads_returns_and_exits_without_consuming_story_input() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    game(root.path());
    let mut provider = MusicaProvider::default();
    let (_, mut session) = provider.open_session(title_request(root.path())).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    assert_eq!(&session.scene.pixels[..4], &[20, 30, 40, 255]);
    session.advance(500_000_000, &[]).unwrap();
    assert_eq!(session.vm.state().instruction_count, 0);
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    session.advance(0, &[key(KeyCode::ArrowUp)]).unwrap();
    session
        .advance(0, &[FamilyEvent::PointerMove { x: 1100.0, y: 80.0 }])
        .unwrap();
    let pixel = ((80 * 1280 + 1100) * 4) as usize;
    assert_eq!(
        &session.scene.pixels[pixel..pixel + 4],
        &[100, 150, 200, 255]
    );
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Load);
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    session.save(20).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    assert!(!session.finished);
    assert_eq!(session.vm.state().gallery_unlocks.len(), 1);
    session
        .advance(0, &[key(KeyCode::ArrowDown), key(KeyCode::Enter)])
        .unwrap();
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowRight),
                key(KeyCode::ArrowRight),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::None);
    assert_eq!(session.vm.state().launch_mode, MusicaLaunchMode::Title);
    assert_eq!(session.vm.state().gallery_unlocks.len(), 1);
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    let previous_reads = session.vm.state().read_message_identities.clone();
    let previous_tick = session.vm.state().fixed_tick;
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().global_variables["custom"], 2);
    assert_eq!(session.vm.state().read_message_identities, previous_reads);
    assert!(session.vm.state().fixed_tick > previous_tick);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    session
        .advance(0, &[key(KeyCode::ArrowUp), key(KeyCode::Enter)])
        .unwrap();
    assert!(session.finished);
    Box::new(session).close().unwrap();
}
#[test]
fn native_gpu_title_unintegrated_pages_and_missing_art_fail_explicitly() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    game(root.path());
    let mut provider = MusicaProvider::default();
    let (_, mut session) = provider.open_session(title_request(root.path())).unwrap();
    let err = session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap_err();
    assert_eq!(err.code(), "ASTRA_EMU_MUSICA_CONFIG_PAGE_UNAVAILABLE");
    Box::new(session).close().unwrap();
    let missing = tempfile::tempdir().unwrap();
    fixture::game(missing.path(), b".end\r\n");
    assert!(provider
        .open_session(title_request(missing.path()))
        .is_err());
    let opened = provider
        .open(request(missing.path(), Sink::default()))
        .unwrap();
    opened.session.close().unwrap();
}
