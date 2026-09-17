use super::*;
use crate::MusicaSystemPage as Page;
fn click(x: f32, y: f32) -> [FamilyEvent; 3] {
    [
        FamilyEvent::PointerMove { x, y },
        FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Pressed,
        },
        FamilyEvent::PointerButton {
            button: PointerButton::Primary,
            state: KeyState::Released,
        },
    ]
}
#[test]
fn native_gpu_config_applies_cancels_persists_tests_audio_and_preserves_gameplay_wait() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    title::game(root.path());
    let mut provider = MusicaProvider::default();
    let sink = Sink::default();
    let mut req = title::title_request(root.path());
    req.host.audio_sink = ROption::RSome(AudioSink_TO::from_value(sink.clone(), TD_Opaque));
    let (_, mut session) = provider.open_session(req).unwrap();
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    session.advance(0, &click(91.0, 320.0)).unwrap();
    let pixel = ((310 * 1280 + 82) * 4) as usize;
    assert_eq!(
        &session.scene.pixels[pixel..pixel + 4],
        &[200, 100, 50, 255]
    );
    session
        .advance(
            0,
            &[
                FamilyEvent::PointerMove { x: 330.0, y: 300.0 },
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
                FamilyEvent::PointerMove { x: 331.0, y: 301.0 },
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                },
            ],
        )
        .unwrap();
    assert!(!session.vm.config_for_presentation().unwrap().text_shadow);
    assert_eq!(session.vm.config().message_speed_auto_play, 50);
    assert_eq!(
        session
            .vm
            .config_for_presentation()
            .unwrap()
            .message_speed_auto_play,
        20
    );
    session.advance(0, &click(760.0, 132.0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !sink.nonzero.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(sink.nonzero.load(Ordering::Acquire));
    session.advance(0, &click(760.0, 208.0)).unwrap();
    session.advance(0, &click(760.0, 280.0)).unwrap();
    assert!(session.vm.state().audio[&0xffff_ff01].playing);
    assert!(session.vm.state().audio[&0xffff_ff02].playing);
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::Title);
    assert_eq!(session.vm.config().message_speed_auto_play, 20);
    assert!(!session.scene.text_shadow());
    assert!(!session.vm.state().audio[&0xffff_ff00].playing);
    assert!(!session.vm.state().audio[&0xffff_ff01].playing);
    assert!(!session.vm.state().audio[&0xffff_ff02].playing);
    let saved = session
        .storage
        .configuration(session.game)
        .unwrap()
        .unwrap();
    assert_eq!(&saved, session.vm.config());
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    let wait = session.vm.state().wait.clone();
    session.advance(0, &[key(KeyCode::F7)]).unwrap();
    session.advance(0, &click(251.0, 320.0)).unwrap();
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(session.vm.state().wait, wait);
    assert_eq!(session.vm.config().message_speed_auto_play, 20);
    Box::new(session).close().unwrap();
    let (_, session) = provider
        .open_session(title::title_request(root.path()))
        .unwrap();
    assert_eq!(session.vm.config(), &saved);
    Box::new(session).close().unwrap();
    let mut req = title::title_request(root.path());
    req.configuration.push(ConfigEntry {
        id: "message_speed_auto_play".into(),
        value: ConfigValue::Integer(80),
    });
    let (_, session) = provider.open_session(req).unwrap();
    assert_eq!(session.vm.config().message_speed_auto_play, 20);
    assert!(!session.vm.config().text_shadow);
    Box::new(session).close().unwrap();
}

#[test]
fn native_gpu_disabled_animation_keeps_scroll_waits_bounded_and_finishes_at_target() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    for command in ["hscroll 8 1", "scroll 8 4 1"] {
        let root = tempfile::tempdir().unwrap();
        fixture::game(
            root.path(),
            format!(".stage * BG.png 0 0\r\n.{command}\r\n.endscroll false\r\n.end\r\n").as_bytes(),
        );
        let mut provider = MusicaProvider::default();
        let (_, mut session) = provider
            .open_session(request(root.path(), Sink::default()))
            .unwrap();
        let mut config = session.vm.config().clone();
        config.animation = false;
        session.vm.set_config(config).unwrap();
        session.advance(16_666_667, &[]).unwrap();
        let first = session.scene.pixels.clone();
        session.advance(16_666_667, &[]).unwrap();
        assert_eq!(
            session.advance(40_000_000, &[]).unwrap().status,
            FamilyStatus::Waiting
        );
        assert!(
            session.scene.pixels == first,
            "disabled scroll changed its intermediate frame"
        );
        assert_eq!(
            session.advance(100_000_000, &[]).unwrap().status,
            FamilyStatus::Finished
        );
        assert!(
            session.scene.pixels != first,
            "completed scroll did not present its target frame"
        );
        Box::new(session).close().unwrap();
    }
}

#[test]
fn native_gpu_config_unavailable_window_change_does_not_persist() {
    let _lock = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    title::game(root.path());
    let mut provider = MusicaProvider::default();
    let (_, mut session) = provider
        .open_session(title::title_request(root.path()))
        .unwrap();
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    session.advance(0, &click(340.0, 130.0)).unwrap();
    assert_eq!(
        session
            .advance(0, &[key(KeyCode::Enter)])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MUSICA_WINDOW_COMMAND_UNAVAILABLE"
    );
    assert!(session
        .storage
        .configuration(session.game)
        .unwrap()
        .is_none());
    assert!(!session.vm.config().fullscreen);
    Box::new(session).close().unwrap();
}
