use super::*;

#[test]
fn native_gpu_includes_execute_on_open_chain_and_restore() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let entry = b".include first.sc\r\n.chain next.sc\r\n";
    let first = b".message 1   Included";
    let next = b".include chinese.sc\r\n.end\r\n";
    let (chinese, _, errors) = encoding_rs::GBK.encode("[e].message 2  姓名 中文剧情");
    assert!(!errors);
    fixture::game(root.path(), entry);
    fixture::assets(
        root.path(),
        "scr",
        &[
            ("test.sc", entry),
            ("first.sc", first),
            ("next.sc", next),
            ("chinese.sc", &chinese),
        ],
    );
    let (_, mut session) = MusicaProvider::default()
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Included");
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let mut capture = Capture(Vec::new());
    session.visit_frame(&mut capture).unwrap();
    let first_frame = capture.0.clone();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "中文剧情");
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    session.visit_frame(&mut capture).unwrap();
    assert!(
        capture.0 == first_frame,
        "restore must reload the expanded script and its font binding"
    );
    assert_eq!(session.message.as_ref().unwrap().0, "Included");
    Box::new(session).close().unwrap();
}

#[test]
fn include_cycle_fails_before_creating_a_family_session() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".include test.sc\r\n");
    assert_eq!(
        MusicaProvider::default()
            .open(request(root.path(), Sink::default()))
            .err()
            .unwrap()
            .code(),
        "ASTRA_EMU_MUSICA_SCRIPT_INCLUDE_CYCLE"
    );
}
