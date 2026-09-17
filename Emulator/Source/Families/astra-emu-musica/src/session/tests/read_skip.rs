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
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::S)]).unwrap();
    opened.session.advance(100_000_000, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let storage = crate::storage::Storage::new(root.path()).unwrap();
    let saved = storage.read().unwrap();
    assert!(MusicaVm::decode_native_save(&saved.vm)
        .unwrap()
        .read_message_identities
        .is_empty());
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let read = storage.read().unwrap();
    let state = MusicaVm::decode_native_save(&read.vm).unwrap();
    assert_eq!(state.read_message_identities.len(), 1);
    assert_eq!(state.system_ui.play_mode, crate::MusicaPlayMode::Skip);
    let before = state.backlog.len();
    opened.session.advance(100_000_000, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::S)]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.advance(100_000_000, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let after = MusicaVm::decode_native_save(&storage.read().unwrap().vm).unwrap();
    assert!(after.backlog.len() > before);
    assert_eq!(after.read_message_identities.len(), 1);
    assert_eq!(after.system_ui.play_mode, crate::MusicaPlayMode::Skip);
    opened.session.close().unwrap();
}
