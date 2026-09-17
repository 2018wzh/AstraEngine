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
