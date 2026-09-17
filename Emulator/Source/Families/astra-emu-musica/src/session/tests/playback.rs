use super::*;

#[test]
fn native_gpu_auto_play_waits_restores_and_pauses_in_backlog() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  speaker First\r\n.message 2  speaker Second\r\n.end\r\n",
    );
    let mut provider = MusicaProvider::default();
    let mut req = request(root.path(), Sink::default());
    req.configuration.push(ConfigEntry {
        id: "message_speed_auto_play".into(),
        value: ConfigValue::Integer(10),
    });
    let mut opened = provider.open(req).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::A)]).unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let mut capture = Capture(Vec::new());
    opened.session.visit_frame(&mut capture).unwrap();
    let first = capture.0.clone();
    opened.session.advance(70_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let second = capture.0.clone();
    assert_ne!(first, second);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, first);
    opened.session.advance(0, &[key(KeyCode::PageUp)]).unwrap();
    opened.session.advance(1_000_000_000, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, first);
    opened.session.advance(70_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, second);
    opened.session.advance(0, &[key(KeyCode::A)]).unwrap();
    assert_eq!(
        opened.session.advance(1_000_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[key(KeyCode::Enter)])
            .unwrap()
            .status,
        FamilyStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_background_preference_gates_clocks_and_preserves_explicit_suspend() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    for background in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture::game(root.path(), b".playbgm tone.ogg\r\n.wait 1000\r\n.end\r\n");
        let mut req = request(root.path(), Sink::default());
        req.configuration.push(ConfigEntry {
            id: "progress_in_background".into(),
            value: ConfigValue::Bool(background),
        });
        let (_, mut session) = MusicaProvider::default().open_session(req).unwrap();
        session.advance(16_666_667, &[]).unwrap();
        session
            .advance(
                0,
                &[
                    key(KeyCode::ControlLeft),
                    FamilyEvent::WindowFocused { focused: false },
                ],
            )
            .unwrap();
        assert_eq!(session.control_keys, 0);
        let before = session.vm.state().fixed_tick;
        let sound_before = session.audio.snapshot().unwrap();
        session.advance(200_000_000, &[]).unwrap();
        if background {
            assert!(session.vm.state().fixed_tick > before);
        } else {
            assert_eq!(session.vm.state().fixed_tick, before);
            let after = session.audio.snapshot().unwrap();
            assert_eq!(after[0].position, sound_before[0].position);
        }
        session
            .advance(0, &[FamilyEvent::WindowSuspended { suspended: true }])
            .unwrap();
        let before = session.vm.state().fixed_tick;
        session
            .advance(200_000_000, &[FamilyEvent::WindowFocused { focused: true }])
            .unwrap();
        assert_eq!(
            session.vm.state().fixed_tick,
            before,
            "focus cannot override explicit suspend"
        );
        session
            .advance(
                0,
                &[
                    key(KeyCode::F5),
                    FamilyEvent::WindowFocused { focused: false },
                ],
            )
            .unwrap();
        session.advance(0, &[key(KeyCode::F9)]).unwrap();
        assert!(session.paused(), "load keeps the live window state");
        let before = session.vm.state().fixed_tick;
        session.advance(200_000_000, &[]).unwrap();
        assert_eq!(session.vm.state().fixed_tick, before);
        session
            .advance(0, &[FamilyEvent::WindowSuspended { suspended: false }])
            .unwrap();
        assert_eq!(session.paused(), !background);
        session
            .advance(16_666_667, &[FamilyEvent::WindowFocused { focused: true }])
            .unwrap();
        assert!(session.vm.state().fixed_tick > before);
        assert!(
            session.vm.state().fixed_tick <= before + 2,
            "paused time must not accumulate"
        );
        Box::new(session).close().unwrap();
    }
}

#[test]
fn native_gpu_initial_unfocused_window_obeys_background_preference() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    for background in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture::game(root.path(), b".wait 1000\r\n.end\r\n");
        let mut req = request(root.path(), Sink::default());
        req.initial_window.focused = false;
        req.configuration.push(ConfigEntry {
            id: "progress_in_background".into(),
            value: ConfigValue::Bool(background),
        });
        let (_, mut session) = MusicaProvider::default().open_session(req).unwrap();
        session.advance(100_000_000, &[]).unwrap();
        assert_eq!(session.vm.state().fixed_tick > 0, background);
        session
            .advance(16_666_667, &[FamilyEvent::WindowFocused { focused: true }])
            .unwrap();
        assert!(session.vm.state().fixed_tick > 0);
        Box::new(session).close().unwrap();
    }
}

#[test]
fn native_gpu_text_shadow_uses_current_manager_setting_after_load() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage BG.png 0 0 * 0 0\r\n.message 1  speaker Shadow\r\n.end\r\n",
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1280, 720, image::Rgba([100, 150, 200, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    fixture::asset(root.path(), "bg", "BG.png", &png.into_inner());
    let mut provider = MusicaProvider::default();
    let mut capture = Capture(Vec::new());
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let outlined = capture.0.clone();
    opened.session.close().unwrap();
    let mut req = request(root.path(), Sink::default());
    req.configuration.push(ConfigEntry {
        id: "text_shadow".into(),
        value: ConfigValue::Bool(false),
    });
    let mut opened = provider.open(req).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let plain = capture.0.clone();
    assert!(
        outlined != plain,
        "shadow must change the rendered dialogue"
    );
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert!(
        plain == capture.0,
        "load must retain the current shadow preference"
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_manual_slots_restore_independent_story_positions() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1   First\r\n.message 2   Second\r\n.end\r\n",
    );
    let (_, mut session) = MusicaProvider::default()
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.save(20).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.save(21).unwrap();
    session.load(20).unwrap();
    assert!(session.advance(0, &[]).unwrap().reset_clock);
    assert!(!session.advance(0, &[]).unwrap().reset_clock);
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    let mut capture = Capture(Vec::new());
    session.visit_frame(&mut capture).unwrap();
    let first = capture.0.clone();
    session.load(21).unwrap();
    assert!(session.advance(0, &[]).unwrap().reset_clock);
    assert!(!session.advance(0, &[]).unwrap().reset_clock);
    assert_eq!(session.message.as_ref().unwrap().0, "Second");
    session.visit_frame(&mut capture).unwrap();
    assert!(
        capture.0 != first,
        "different slots must render their own restored message"
    );
    Box::new(session).close().unwrap();
}
