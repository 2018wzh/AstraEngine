use super::*;
#[test]
fn native_gpu_read_skip_stops_on_unread_and_restores_mode() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".label repeat\r\n.message 1  speaker First\r\n.goto repeat\r\n",
    );
    let mut provider = MusicaProvider::default();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.advance(0, &[key(KeyCode::S)]).unwrap();
    session.advance(100_000_000, &[]).unwrap();
    session.save(0).unwrap();
    let storage = crate::storage::Storage::new(root.path()).unwrap();
    let saved = storage.read(0).unwrap();
    assert!(MusicaVm::decode_native_save(&saved.vm)
        .unwrap()
        .read_message_identities
        .is_empty());
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.save(0).unwrap();
    let read = storage.read(0).unwrap();
    let state = MusicaVm::decode_native_save(&read.vm).unwrap();
    assert_eq!(state.read_message_identities.len(), 1);
    assert_eq!(state.system_ui.play_mode, crate::MusicaPlayMode::Skip);
    let before = state.backlog.len();
    session.advance(100_000_000, &[]).unwrap();
    session.advance(0, &[key(KeyCode::S)]).unwrap();
    session.load(0).unwrap();
    session.advance(100_000_000, &[]).unwrap();
    session.save(0).unwrap();
    let after = MusicaVm::decode_native_save(&storage.read(0).unwrap().vm).unwrap();
    assert!(after.backlog.len() > before);
    assert_eq!(after.read_message_identities.len(), 1);
    assert_eq!(after.system_ui.play_mode, crate::MusicaPlayMode::Skip);
    Box::new(session).close().unwrap();
}
