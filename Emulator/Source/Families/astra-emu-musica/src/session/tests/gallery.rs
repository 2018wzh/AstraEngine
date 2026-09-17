use super::*;
use crate::{runtime::gallery, MusicaSystemPage as Page};

#[test]
fn native_gpu_gallery_pages_music_and_script_return_share_session_lifecycle() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    title::game(root.path());
    let mut assets = Vec::new();
    for name in [
        "topMenu0.png",
        "topMenu0Over.png",
        "topMenu2.png",
        "topMenu2Over.png",
        "memories.png",
        "musicPage1.png",
        "musicPage2.png",
        "musicPage3.png",
        "cgmode0.png",
        "cgmode0box.png",
        "flash0.png",
    ] {
        assets.push((name.to_owned(), title::png(1280, 720, [20, 30, 40, 255])));
    }
    for (name, w, h, color) in [
        ("musicNote.png", 32, 32, [200, 100, 50, 255]),
        ("cgpage001.png", 64, 32, [20, 30, 40, 255]),
        ("cgpage002.png", 64, 32, [20, 30, 40, 255]),
        ("cgmode0menu.png", 384, 64, [20, 30, 40, 255]),
        ("flash0menu.png", 384, 64, [20, 30, 40, 255]),
        ("cgthumb/test.png", 128, 72, [220, 30, 40, 255]),
    ] {
        assets.push((name.to_owned(), title::png(w, h, color)));
    }
    fixture::assets(
        root.path(),
        "sys",
        &assets
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect::<Vec<_>>(),
    );
    let wave = fixture::wave();
    fixture::assets(
        root.path(),
        "bgm",
        &[("BGM001.ogg", &wave), ("BGM084.ogg", &wave)],
    );
    let mut scripts = vec![("test.sc", b".message 1   Main\r\n.end\r\n".as_slice())];
    for name in gallery::REPLAYS.into_iter().chain(gallery::MOVIES) {
        scripts.push((name, b".message 1   Gallery\r\n.end\r\n".as_slice()));
    }
    fixture::assets(root.path(), "scr", &scripts);
    let mut provider = MusicaProvider::default();
    let mut req = title::title_request(root.path());
    let sink = Sink::default();
    req.host.audio_sink = ROption::RSome(AudioSink_TO::from_value(sink.clone(), TD_Opaque));
    let (_, mut session) = provider.open_session(req).unwrap();
    session
        .vm
        .merge_verified_gallery_unlocks(&[astra_core::Hash256::from_sha256(b"TOHKA_CLEAR")])
        .unwrap();
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::Memories);
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::GalleryCg);
    let pixel = ((97 * 1280 + 65) * 4) as usize;
    assert_eq!(&session.scene.pixels[pixel..pixel + 4], &[100, 30, 40, 255]);
    session
        .advance(500_000_000, &[key(KeyCode::ArrowDown)])
        .unwrap();
    assert_eq!(session.vm.state().instruction_count, 0);
    session
        .advance(
            0,
            &[
                key(KeyCode::Escape),
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::GalleryBgm);
    session
        .advance(0, &[key(KeyCode::ArrowUp), key(KeyCode::Enter)])
        .unwrap();
    assert_eq!(
        session.vm.state().audio[&0].resource_uri,
        "musica:/bgm/BGM084.ogg"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while !sink.nonzero.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(sink.nonzero.load(Ordering::Acquire));
    let click = FamilyEvent::PointerButton {
        button: PointerButton::Primary,
        state: KeyState::Pressed,
    };
    session
        .advance(
            0,
            &[
                FamilyEvent::PointerMove { x: 760.0, y: 590.0 },
                click.clone(),
            ],
        )
        .unwrap();
    assert!(!session.vm.state().audio[&0].playing);
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                FamilyEvent::PointerMove { x: 180.0, y: 100.0 },
                click.clone(),
            ],
        )
        .unwrap();
    assert_eq!(
        session.vm.state().audio[&0].resource_uri,
        "musica:/bgm/BGM001.ogg"
    );
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert!(!session.vm.state().audio[&0].playing);
    for (focus, expected) in [(1, gallery::REPLAYS[0]), (3, gallery::MOVIES[0])] {
        session.vm.set_gallery_page(Page::Memories, focus).unwrap();
        session.advance(0, &[key(KeyCode::Enter)]).unwrap();
        session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
        assert_eq!(
            session.vm.state().script_uri,
            format!("musica:/scr/{expected}")
        );
        assert_eq!(session.message.as_ref().unwrap().0, "Gallery");
        session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
        assert_eq!(session.vm.state().system_ui.page, Page::Title);
    }
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Main");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.vm.set_gallery_page(Page::GalleryBgm, 46).unwrap();
    let failure = session
        .advance(0, &[FamilyEvent::PointerMove { x: 180.0, y: 580.0 }, click])
        .unwrap_err();
    assert_eq!(failure.code(), "ASTRA_EMU_MUSICA_GALLERY_BGM_POINTER");
    assert_eq!(session.vm.state().system_ui.focus_index, 46);
    assert_eq!(
        session.advance(0, &[]).unwrap_err().code(),
        "ASTRA_EMU_MUSICA_POISONED"
    );
    Box::new(session).close().unwrap();
}
