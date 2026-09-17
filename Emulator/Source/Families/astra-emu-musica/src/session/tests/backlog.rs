use super::*;

#[test]
fn native_gpu_backlog_navigation_voice_save_restore_and_return() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1 tone.ogg speaker First\r\n.message 2  speaker Second\r\n.end\r\n",
    );
    fixture::asset(root.path(), "voice", "tone.ogg", &fixture::wave());
    let sink = Sink::default();
    let mut provider = MusicaProvider::default();
    let mut opened = provider.open(request(root.path(), sink.clone())).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    let mut capture = Capture(Vec::new());
    opened.session.visit_frame(&mut capture).unwrap();
    let second = capture.0.clone();
    opened.session.advance(0, &[key(KeyCode::PageUp)]).unwrap();
    opened.session.advance(0, &[key(KeyCode::ArrowUp)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let first_history = capture.0.clone();
    assert_ne!(first_history, second);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened
        .session
        .advance(0, &[key(KeyCode::ArrowDown)])
        .unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_ne!(capture.0, first_history);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, first_history);
    sink.nonzero.store(false, Ordering::Release);
    opened.session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !sink.nonzero.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(sink.nonzero.load(Ordering::Acquire));
    assert_eq!(
        opened.session.advance(1_000_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, second);
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
fn backlog_replay_does_not_replace_pending_message_voice_duration() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1 short.ogg speaker First\r\n.message 2 long.ogg speaker Second\\v\r\n.end\r\n",
    );
    let short = fixture::wave();
    let mut long = short.clone();
    long.extend_from_slice(&short[44..]);
    let length = long.len() as u32;
    long[4..8].copy_from_slice(&(length - 8).to_le_bytes());
    long[40..44].copy_from_slice(&(length - 44).to_le_bytes());
    fixture::assets(
        root.path(),
        "voice",
        &[("short.ogg", &short), ("long.ogg", &long)],
    );
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    opened
        .session
        .advance(
            0,
            &[
                key(KeyCode::PageUp),
                key(KeyCode::ArrowUp),
                key(KeyCode::Enter),
                key(KeyCode::F5),
            ],
        )
        .unwrap();
    let saved = crate::storage::Storage::new(root.path())
        .unwrap()
        .read(0)
        .unwrap();
    let state = MusicaVm::decode_native_save(&saved.vm).unwrap();
    assert!(matches!(
        state.wait,
        Some(crate::MusicaWaitState::Voice {
            milliseconds: Some(200),
            ..
        })
    ));
    opened
        .session
        .advance(0, &[key(KeyCode::F9), key(KeyCode::Escape)])
        .unwrap();
    opened.session.close().unwrap();
}
