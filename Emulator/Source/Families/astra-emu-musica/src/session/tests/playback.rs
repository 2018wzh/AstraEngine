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
