use super::*;
use crate::{storage::Storage, MusicaSystemPage};

fn game(root: &std::path::Path) {
    fixture::game(
        root,
        b".message 1   First\r\n.message 2   Second\r\n.end\r\n",
    );
    let mut assets = Vec::new();
    for (name, w, h, color) in [
        ("saveloadBase.png", 1280, 720, [20, 30, 40, 255]),
        ("saveloadSave.png", 352, 48, [200, 30, 40, 255]),
        ("saveloadLoad.png", 352, 48, [20, 200, 40, 255]),
        ("saveloadSelect.png", 344, 98, [200, 200, 40, 60]),
        ("saveloadButtons.png", 356, 48, [200, 30, 200, 255]),
        ("notsaved.png", 106, 60, [100, 100, 100, 255]),
    ] {
        assets.push((name.to_owned(), png(w, h, color)));
    }
    for page in 0..10 {
        assets.push((
            format!("saveload_Page{page}.png"),
            png(208, 48, [page * 20, 50, 100, 255]),
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
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba(color))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    png.into_inner()
}
fn click(x: f32, y: f32) -> [FamilyEvent; 2] {
    [
        FamilyEvent::PointerMove { x, y },
        FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Pressed,
        },
    ]
}
#[test]
fn native_gpu_save_pages_navigate_save_and_restore_gameplay() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    game(root.path());
    let mut provider = MusicaProvider::default();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    let first = session.scene.pixels.clone();
    session.advance(0, &[key(KeyCode::F6)]).unwrap();
    assert_eq!(session.vm.state().system_ui.focus_index, 20);
    assert!(first != session.scene.pixels);
    let tick = session.vm.state().fixed_tick;
    session
        .advance(1_000_000_000, &[key(KeyCode::ArrowRight)])
        .unwrap();
    assert_eq!(session.vm.state().fixed_tick, tick);
    assert_eq!(session.vm.state().system_ui.focus_index, 30);
    session.advance(0, &[key(KeyCode::ArrowLeft)]).unwrap();
    session.advance(0, &click(420.0, 100.0)).unwrap(); // Column gap must not save.
    assert!(session.save_cards.is_empty());
    session.advance(0, &click(470.0, 100.0)).unwrap();
    assert_eq!(session.vm.state().system_ui.focus_index, 21);
    let saved = Storage::new(root.path()).unwrap().read(21).unwrap();
    assert_eq!(saved.message.as_ref().unwrap().0, "First");
    assert_eq!(
        MusicaVm::decode_native_save(&saved.vm)
            .unwrap()
            .system_ui
            .page,
        MusicaSystemPage::None
    );
    let thumbnail = saved.card.texture().unwrap();
    let expected = image::imageops::resize(
        &image::RgbaImage::from_raw(1280, 720, first.to_vec()).unwrap(),
        96,
        54,
        image::imageops::FilterType::Triangle,
    );
    assert_eq!(thumbnail.rgba8.as_ref(), expected.as_raw());
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert!(session.scene.pixels == first);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Second");
    session.advance(0, &[key(KeyCode::F8)]).unwrap();
    session.advance(0, &[key(KeyCode::Enter)]).unwrap(); // Empty slot 20 stays on page.
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Load);
    session
        .advance(0, &[key(KeyCode::ArrowDown), key(KeyCode::Enter)])
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::None);
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    assert!(session.scene.pixels == first);
    Box::new(session).close().unwrap();
    let (_, mut reopened) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    reopened.advance(16_666_667, &[]).unwrap();
    reopened.advance(0, &[key(KeyCode::F8)]).unwrap();
    reopened.advance(0, &click(470.0, 100.0)).unwrap();
    assert!(reopened.scene.pixels == first);
    Box::new(reopened).close().unwrap();
}

#[test]
fn native_gpu_save_page_rejects_corrupt_cards_without_overwriting() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    game(root.path());
    let folder = root.path().join(".astra-musica/saves");
    std::fs::create_dir_all(&folder).unwrap();
    let slot = folder.join("slot-020.asav");
    std::fs::write(&slot, b"original corrupt slot").unwrap();
    let (_, mut session) = MusicaProvider::default()
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    let cause = session.advance(0, &[key(KeyCode::F6)]).unwrap_err();
    assert_eq!(cause.code(), "ASTRA_EMU_MUSICA_SAVE_FORMAT");
    assert_eq!(std::fs::read(&slot).unwrap(), b"original corrupt slot");
    Box::new(session).close().unwrap();
}

#[test]
fn native_gpu_quick_save_rotates_skips_same_line_and_survives_restart() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let script = (0..14)
        .map(|i| format!(".message {}   Line{}\r\n", i + 1, i))
        .collect::<String>()
        + ".end\r\n";
    fixture::game(root.path(), script.as_bytes());
    let mut provider = MusicaProvider::default();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.save(0).unwrap();
    session.save(20).unwrap();
    let storage = Storage::new(root.path()).unwrap();
    for i in 0..12 {
        if i > 0 {
            session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
        }
        session.advance(0, &[key(KeyCode::F5)]).unwrap();
        let cursor = (i + 1) % 10;
        assert_eq!(session.quick_cursor, cursor);
        assert_eq!(storage.quick_cursor(session.game).unwrap(), cursor);
        session.advance(0, &[key(KeyCode::F5)]).unwrap();
        assert_eq!(
            session.quick_cursor, cursor,
            "same source line must not consume another slot"
        );
    }
    assert_eq!(storage.read(10).unwrap().message.unwrap().0, "Line10");
    assert_eq!(storage.read(11).unwrap().message.unwrap().0, "Line11");
    assert_eq!(storage.read(12).unwrap().message.unwrap().0, "Line2");
    for slot in [0, 20] {
        assert_eq!(storage.read(slot).unwrap().message.unwrap().0, "Line0");
    }
    Box::new(session).close().unwrap();
    let (_, mut reopened) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    assert_eq!(reopened.quick_cursor, 2);
    reopened.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert_eq!(reopened.message.as_ref().unwrap().0, "Line11");
    reopened
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    reopened.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert_eq!(storage.read(12).unwrap().message.unwrap().0, "Line12");
    Box::new(reopened).close().unwrap();
}
